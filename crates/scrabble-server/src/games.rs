//! Public creation information and bounded lifecycle policy; gameplay authority stays in the runtime.
use crate::{
    factory::{MatchFactory, creation_time, game_id},
    simulation::{MATCH_LIFETIME_SECONDS, MatchLifecycle, ScrabbleSimulation, retirement_due},
};
use game_server::{BrowserRoutePrefix, LiveHostError, LiveMatchHost, MatchId, MatchRuntime};
use scrabble_game::identity::GameId;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

pub const CREATE_RETRY_SECONDS: u64 = 120;
const CLOCK_SKEW_SECONDS: u64 = 10;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateGame {
    pub version: u8,
    pub request_id: String,
    pub requested_at: u64,
}
impl CreateGame {
    pub fn validate(&self, now: u64) -> Result<(), GameOperationError> {
        if !matches!(self.version, 1 | 2)
            || self.request_id.len() != 32
            || !self
                .request_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || self.requested_at > now.saturating_add(CLOCK_SKEW_SECONDS)
        {
            return Err(GameOperationError::InvalidRequest);
        }
        if now.saturating_sub(self.requested_at) >= CREATE_RETRY_SECONDS {
            return Err(GameOperationError::ExpiredRequest);
        }
        Ok(())
    }
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateConfiguredGame {
    pub version: u8,
    pub request_id: String,
    pub requested_at: u64,
    pub selection: crate::profile_catalog::Selection,
    pub rules: crate::profiles::MatchRules,
    /// Optional match clock (#114); absent or null is untimed (Decision 3.11).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_control: Option<scrabble_game::ruleset::TimeControl>,
}
#[derive(Debug)]
pub enum ConfiguredError {
    Game(GameOperationError),
    Selection(crate::profile_catalog::SelectionError),
    /// Rejected time control; the value is the public `reason`.
    TimeControl(&'static str),
    Conflict,
}

/// Validates a requested time control before any allocation: durations, then
/// Decision 3.8, then the provisional forfeit block (#99).
pub fn check_time_control(
    control: &scrabble_game::ruleset::TimeControl,
) -> Result<(), &'static str> {
    use scrabble_game::ruleset::{ClockModel, ExpiryConsequence, MAX_CLOCK_MS};
    let valid = |ms: u64| (1..=MAX_CLOCK_MS).contains(&ms);
    let durations = match control.clock {
        ClockModel::Cumulative {
            initial_ms,
            increment_ms,
        } => valid(initial_ms) && increment_ms.is_none_or(valid),
        ClockModel::PerTurn { allowance_ms } => valid(allowance_ms),
    };
    if !durations {
        return Err("invalid-duration");
    }
    if matches!(
        control.clock,
        ClockModel::Cumulative {
            increment_ms: None,
            ..
        }
    ) && control.on_expiry == ExpiryConsequence::Pass
    {
        return Err("missing-increment");
    }
    if control.on_expiry == ExpiryConsequence::Forfeit {
        return Err("forfeit-unavailable");
    }
    Ok(())
}
impl From<GameOperationError> for ConfiguredError {
    fn from(value: GameOperationError) -> Self {
        Self::Game(value)
    }
}
/// A shared board's request to follow its finished match with a fresh one (#33).
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateRematch {
    pub version: u8,
    /// The finished match's private creation nonce, proving the request comes from its board.
    pub board_request_id: String,
    pub request_id: String,
    pub requested_at: u64,
}
impl CreateRematch {
    fn creation(&self) -> CreateGame {
        CreateGame {
            version: 2,
            request_id: self.request_id.clone(),
            requested_at: self.requested_at,
        }
    }
    pub fn validate(&self, now: u64) -> Result<(), GameOperationError> {
        if self.version != 1
            || self.board_request_id.len() != 32
            || !self
                .board_request_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(GameOperationError::InvalidRequest);
        }
        self.creation().validate(now)
    }
}
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JoinInformation {
    pub version: u8,
    pub match_id: String,
    pub game_id: GameId,
    pub match_path: String,
    pub expires_at: u64,
}
#[derive(Debug, Eq, PartialEq)]
pub enum GameOperationError {
    InvalidRequest,
    ExpiredRequest,
    ExpiredMatch,
    JoinClosed,
    NotServing,
    Draining,
    AtCapacity,
    UnknownMatch,
    NotRetirable,
    BoardRejected,
    NotFinished,
    RematchExists,
    NoRematch,
    Internal,
}
impl From<LiveHostError> for GameOperationError {
    fn from(error: LiveHostError) -> Self {
        match error {
            LiveHostError::NotServing => Self::NotServing,
            LiveHostError::Draining => Self::Draining,
            LiveHostError::AtCapacity => Self::AtCapacity,
            LiveHostError::UnknownMatch => Self::UnknownMatch,
            _ => Self::Internal,
        }
    }
}
pub fn unix_seconds() -> Result<u64, GameOperationError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| GameOperationError::Internal)
}

pub struct Games {
    host: LiveMatchHost<ScrabbleSimulation>,
    factory: MatchFactory,
    prefix: BrowserRoutePrefix,
    reconnect_grace_ticks: u64,
    creation: Mutex<()>,
    /// Finished match -> its rematch. Kept beside placement, outside both simulations, their
    /// replay and recovery evidence; removed when the finished match retires.
    rematches: Mutex<HashMap<String, JoinInformation>>,
}
impl Games {
    pub fn new(
        host: LiveMatchHost<ScrabbleSimulation>,
        factory: MatchFactory,
        prefix: BrowserRoutePrefix,
        reconnect_grace_ticks: u64,
    ) -> Self {
        Self {
            host,
            factory,
            prefix,
            reconnect_grace_ticks,
            creation: Mutex::new(()),
            rematches: Mutex::new(HashMap::new()),
        }
    }
    pub async fn create(
        &self,
        request: &CreateGame,
        now: u64,
    ) -> Result<JoinInformation, GameOperationError> {
        request.validate(now)?;
        let _creation = self.creation.lock().await;
        let id = match request.version {
            1 => self
                .factory
                .creation_id(&request.request_id, request.requested_at),
            2 => self
                .factory
                .board_creation_id(&request.request_id, request.requested_at),
            _ => return Err(GameOperationError::InvalidRequest),
        }
        .map_err(|_| GameOperationError::Internal)?;
        self.place_created(&id, request.requested_at).await
    }
    pub fn profiles(&self) -> serde_json::Value {
        self.factory.catalog()
    }
    pub async fn create_configured(
        &self,
        request: &CreateConfiguredGame,
        now: u64,
    ) -> Result<JoinInformation, ConfiguredError> {
        if request.version != 3 {
            return Err(GameOperationError::InvalidRequest.into());
        }
        if let Some(control) = &request.time_control {
            check_time_control(control).map_err(ConfiguredError::TimeControl)?;
        }
        CreateGame {
            version: 2,
            request_id: request.request_id.clone(),
            requested_at: request.requested_at,
        }
        .validate(now)?;
        let _creation = self.creation.lock().await;
        let id = self
            .factory
            .configured_id(&request.request_id, request.requested_at)
            .map_err(|_| GameOperationError::Internal)?;
        let factory = self.factory.clone();
        let inspect_id = id.clone();
        let existing = tokio::task::spawn_blocking(move || factory.initialized(&inspect_id))
            .await
            .map_err(|_| GameOperationError::Internal)?
            .map_err(|_| GameOperationError::Internal)?;
        let newly_initialized = existing.is_none();
        if let Some(existing) = existing {
            if existing.selection != request.selection
                || crate::profiles::MatchRules::from(&existing.configuration.rules) != request.rules
                || existing.configuration.rules.time_control != request.time_control
            {
                return Err(ConfiguredError::Conflict);
            }
        } else {
            let prepared = self
                .factory
                .resolve(&request.selection, request.rules, request.time_control)
                .map_err(ConfiguredError::Selection)?;
            if !self.host.is_serving() {
                return Err(GameOperationError::NotServing.into());
            }
            if self.host.is_draining().await {
                return Err(GameOperationError::Draining.into());
            }
            if self.host.statuses().await.len() >= self.host.max_matches() {
                return Err(GameOperationError::AtCapacity.into());
            }
            let factory = self.factory.clone();
            let initialize_id = id.clone();
            tokio::task::spawn_blocking(move || factory.initialize(&initialize_id, prepared))
                .await
                .map_err(|_| GameOperationError::Internal)?
                .map_err(|_| GameOperationError::Internal)?;
        }
        let result = self.place_created(&id, request.requested_at).await;
        if result.is_err() && newly_initialized {
            let factory = self.factory.clone();
            tokio::task::spawn_blocking(move || factory.retire_initialization(&id))
                .await
                .map_err(|_| GameOperationError::Internal)?
                .map_err(|_| GameOperationError::Internal)?;
        }
        result.map_err(ConfiguredError::Game)
    }
    /// Places a newly derived match, or confirms an earlier placement of the same ID.
    /// Callers hold the creation lock.
    async fn place_created(
        &self,
        id: &MatchId,
        requested_at: u64,
    ) -> Result<JoinInformation, GameOperationError> {
        let statuses = self.host.statuses().await;
        // Even a duplicate retry is rejected during drain. Placement remains the final authority.
        if !self.host.is_serving() {
            return Err(GameOperationError::NotServing);
        }
        if self.host.is_draining().await {
            return Err(GameOperationError::Draining);
        }
        if statuses.iter().any(|status| &status.id == id) {
            // A read lease verifies that retirement has not already fenced this match.
            self.host
                .inspect(id, |runtime| {
                    if runtime.is_draining() || runtime.is_frozen() {
                        Err(GameOperationError::Draining)
                    } else {
                        Ok(())
                    }
                })
                .await??;
        } else {
            if statuses
                .iter()
                .any(|status| game_id(&status.id) == game_id(id))
            {
                return Err(GameOperationError::Internal);
            }
            let factory = self.factory.clone();
            let initialize_id = id.clone();
            let simulation = tokio::task::spawn_blocking(move || factory.create(&initialize_id))
                .await
                .map_err(|_| GameOperationError::Internal)?
                .map_err(|_| GameOperationError::Internal)?;
            self.host
                .place(
                    id.clone(),
                    MatchRuntime::new_with_replay_capture(simulation, self.reconnect_grace_ticks),
                )
                .await
                .map_err(|failure| GameOperationError::from(failure.into_parts().0))?;
        }
        Ok(JoinInformation {
            version: 1,
            match_id: id.as_str().to_owned(),
            game_id: game_id(id),
            match_path: self.prefix.match_path(id),
            expires_at: requested_at.saturating_add(MATCH_LIFETIME_SECONDS),
        })
    }
    /// Starts (or idempotently confirms) the one rematch of a finished board match. Nothing is
    /// submitted to the finished runtime; the link lives only in this registry.
    pub async fn rematch(
        &self,
        finished: &MatchId,
        request: &CreateRematch,
        now: u64,
    ) -> Result<JoinInformation, GameOperationError> {
        request.validate(now)?;
        let _creation = self.creation.lock().await;
        if !self.host.is_serving() {
            return Err(GameOperationError::NotServing);
        }
        if self.host.is_draining().await {
            return Err(GameOperationError::Draining);
        }
        let phase = self.phase(finished).await?;
        let authorized = self
            .factory
            .board_authority(finished)
            .is_ok_and(|authority| authority.verify(&request.board_request_id));
        if !authorized {
            return Err(GameOperationError::BoardRejected);
        }
        if !matches!(phase, scrabble_game::protocol::Phase::Finished { .. }) {
            return Err(GameOperationError::NotFinished);
        }
        let id = self
            .factory
            .board_creation_id(&request.request_id, request.requested_at)
            .map_err(|_| GameOperationError::Internal)?;
        if &id == finished {
            // Reusing the finished match's own creation intent would link it to itself.
            return Err(GameOperationError::InvalidRequest);
        }
        if let Some(existing) = self.rematches.lock().await.get(finished.as_str()) {
            return if existing.match_id == id.as_str() {
                Ok(existing.clone())
            } else {
                Err(GameOperationError::RematchExists)
            };
        }
        let join = self.place_created(&id, request.requested_at).await?;
        self.rematches
            .lock()
            .await
            .insert(finished.as_str().to_owned(), join.clone());
        Ok(join)
    }
    /// Number of recorded finished-to-rematch links. Diagnostic and test observation only.
    #[doc(hidden)]
    pub async fn rematch_link_count(&self) -> usize {
        self.rematches.lock().await.len()
    }
    /// Public discovery of a finished match's rematch for its phones.
    pub async fn rematch_of(
        &self,
        finished: &MatchId,
        now: u64,
    ) -> Result<JoinInformation, GameOperationError> {
        self.lookup_checked(finished, now, false).await?;
        self.rematches
            .lock()
            .await
            .get(finished.as_str())
            .cloned()
            .ok_or(GameOperationError::NoRematch)
    }
    async fn phase(
        &self,
        id: &MatchId,
    ) -> Result<scrabble_game::protocol::Phase, GameOperationError> {
        self.host
            .inspect(id, |runtime| {
                if runtime.is_draining() || runtime.is_frozen() {
                    return Err(GameOperationError::Draining);
                }
                let snapshot = runtime
                    .snapshot()
                    .map_err(|_| GameOperationError::Internal)?;
                #[derive(Deserialize)]
                struct GameHeader {
                    phase: scrabble_game::protocol::Phase,
                }
                #[derive(Deserialize)]
                struct Projection {
                    game: GameHeader,
                }
                serde_json::from_slice::<Projection>(&snapshot.payload)
                    .map(|projection| projection.game.phase)
                    .map_err(|_| GameOperationError::Internal)
            })
            .await?
    }
    pub async fn lookup(
        &self,
        id: &MatchId,
        now: u64,
    ) -> Result<JoinInformation, GameOperationError> {
        self.lookup_checked(id, now, false).await
    }
    pub async fn check_join(
        &self,
        id: &MatchId,
        now: u64,
    ) -> Result<JoinInformation, GameOperationError> {
        self.lookup_checked(id, now, true).await
    }
    async fn lookup_checked(
        &self,
        id: &MatchId,
        now: u64,
        new_player: bool,
    ) -> Result<JoinInformation, GameOperationError> {
        let created_at = creation_time(id).ok_or(GameOperationError::UnknownMatch)?;
        if now >= created_at.saturating_add(MATCH_LIFETIME_SECONDS) {
            return Err(GameOperationError::ExpiredMatch);
        }
        if !self.host.is_serving() {
            return Err(GameOperationError::NotServing);
        }
        if self.host.is_draining().await {
            return Err(GameOperationError::Draining);
        }
        self.host
            .inspect(id, |runtime| {
                if runtime.is_draining() || runtime.is_frozen() {
                    return Err(GameOperationError::Draining);
                }
                let snapshot = runtime
                    .snapshot()
                    .map_err(|_| GameOperationError::Internal)?;
                #[derive(Deserialize)]
                struct GameHeader {
                    phase: scrabble_game::protocol::Phase,
                }
                #[derive(Deserialize)]
                struct Projection {
                    lifecycle: Option<MatchLifecycle>,
                    game: GameHeader,
                }
                let current: Projection = serde_json::from_slice(&snapshot.payload)
                    .map_err(|_| GameOperationError::Internal)?;
                if current
                    .lifecycle
                    .is_some_and(|lifecycle| lifecycle.can_retire(now, runtime.current_tick()))
                {
                    return Err(GameOperationError::ExpiredMatch);
                }
                if new_player {
                    if !matches!(current.game.phase, scrabble_game::protocol::Phase::Lobby {}) {
                        return Err(GameOperationError::JoinClosed);
                    }
                    if runtime.slot_count() >= runtime.max_players() {
                        return Err(GameOperationError::AtCapacity);
                    }
                }
                Ok(())
            })
            .await??;
        Ok(JoinInformation {
            version: 1,
            match_id: id.as_str().to_owned(),
            game_id: game_id(id),
            match_path: self.prefix.match_path(id),
            expires_at: created_at.saturating_add(MATCH_LIFETIME_SECONDS),
        })
    }
    pub async fn retire(&self, id: &MatchId, now: u64) -> Result<(), GameOperationError> {
        // Serialized with creation so a racing rematch cannot link a match retired meanwhile.
        let _creation = self.creation.lock().await;
        let due = self
            .host
            .inspect(id, |runtime| {
                let snapshot = runtime
                    .snapshot()
                    .map_err(|_| GameOperationError::Internal)?;
                retirement_due(&snapshot.payload, now, runtime.current_tick())
                    .map_err(|_| GameOperationError::Internal)
            })
            .await??;
        if !due {
            return Err(GameOperationError::NotRetirable);
        }
        self.host.retire(id).await?;
        self.rematches.lock().await.remove(id.as_str());
        self.factory
            .retire_initialization(id)
            .map_err(|_| GameOperationError::Internal)?;
        Ok(())
    }
    pub async fn sweep(&self, now: u64) {
        for status in self.host.statuses().await {
            match self.retire(&status.id, now).await {
                Ok(())
                | Err(
                    GameOperationError::NotRetirable
                    | GameOperationError::UnknownMatch
                    | GameOperationError::Draining
                    | GameOperationError::NotServing,
                ) => {}
                Err(_) => eprintln!("game retirement failed"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn lifetime_retirement_fences_real_scrabble_sessions_and_cannot_be_recreated_by_retry() {
        use game_server::{
            MatchHost, MatchHostWebTransportConfig, RejectMatchControlService, WELCOME_BYTES,
            decode_welcome, serve_live_match_host_with_control_and_shutdown,
        };
        use std::{collections::BTreeMap, time::Duration};
        use tokio::{sync::mpsc, task::JoinSet};
        use wtransport::{ClientConfig, Endpoint, Identity};
        tokio::time::timeout(Duration::from_secs(10), async {
            let directory = tempfile::tempdir().unwrap();
            let identity = Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
            let cert = directory.path().join("cert.pem");
            let key = directory.path().join("key.pem");
            identity.certificate_chain().store_pemfile(&cert).await.unwrap();
            identity.private_key().store_secret_pemfile(&key).await.unwrap();
            let config = crate::config::ServerConfig::from_values(&BTreeMap::from([
                ("SCRABBLE_MATCH_IDS".into(), String::new()),
                ("SCRABBLE_SEED_FILE".into(), directory.path().join("seed").to_str().unwrap().into()),
                ("SCRABBLE_RECOVERY_DIR".into(), directory.path().join("recovery").to_str().unwrap().into()),
            ])).unwrap();
            let host = LiveMatchHost::new(MatchHost::new(1).unwrap());
            let games = Games::new(host.clone(), MatchFactory::new(&config).unwrap(), config.route_prefix.clone(), 1200);
            let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            let port = socket.local_addr().unwrap().port();
            drop(socket);
            let (shutdown, receiver) = mpsc::channel(1);
            let mut tasks = JoinSet::new();
            let serving = host.clone();
            tasks.spawn(async move { serve_live_match_host_with_control_and_shutdown(serving, RejectMatchControlService,
                MatchHostWebTransportConfig { port, certificate_pem: cert, private_key_pem: key,
                    route_prefix: config.route_prefix, allowed_origins: None, drain_grace: Duration::ZERO }, receiver).await });
            while !host.is_serving() {
                tokio::select! { result = tasks.join_next() => panic!("server exited before readiness: {result:?}"), _ = tokio::task::yield_now() => {} }
            }
            let now = unix_seconds().unwrap();
            let request = CreateGame { version: 1, request_id: "07".repeat(16), requested_at: now };
            let join = games.create(&request, now).await.unwrap();
            let client = Endpoint::client(ClientConfig::builder().with_bind_default()
                .with_server_certificate_hashes([identity.certificate_chain().as_slice()[0].hash()]).build()).unwrap();
            let url = format!("https://127.0.0.1:{port}{}", join.match_path);
            let connection = client.connect(&url).await.unwrap();
            let mut welcome = [0; WELCOME_BYTES];
            connection.accept_uni().await.unwrap().read_exact(&mut welcome).await.unwrap();
            let welcome = decode_welcome(&welcome).unwrap();
            let id = MatchId::new(&join.match_id).unwrap();
            assert_eq!(games.lookup(&id, now).await.unwrap(), join);
            assert_eq!(games.lookup(&id, join.expires_at).await, Err(GameOperationError::ExpiredMatch));
            assert_eq!(games.retire(&id, now).await, Err(GameOperationError::NotRetirable));
            games.retire(&id, join.expires_at).await.unwrap();
            connection.closed().await;
            assert!(host.statuses().await.is_empty());
            assert!(client.connect(&url).await.is_err());
            let reconnect = format!("{url}/reconnect/{}", game_server::ReconnectToken(welcome.reconnect_token).encode_hex());
            assert!(client.connect(reconnect).await.is_err());
            assert_eq!(games.create(&request, join.expires_at).await, Err(GameOperationError::ExpiredRequest));
            assert_eq!(games.retire(&id, join.expires_at).await, Err(GameOperationError::UnknownMatch));
            shutdown.send(()).await.unwrap();
            tasks.join_next().await.unwrap().unwrap().unwrap();
        }).await.expect("lifetime retirement must finish against the live runtime");
    }

    #[test]
    fn retry_window_has_explicit_expiry_and_clock_skew_bounds() {
        let request = CreateGame {
            version: 1,
            request_id: "01".repeat(16),
            requested_at: 1000,
        };
        assert_eq!(request.validate(990), Ok(()));
        assert_eq!(
            request.validate(989),
            Err(GameOperationError::InvalidRequest)
        );
        assert_eq!(request.validate(1119), Ok(()));
        assert_eq!(
            request.validate(1120),
            Err(GameOperationError::ExpiredRequest)
        );
    }
}
