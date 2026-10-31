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
/// #219: any created game with no connected player for this long retires as inactive.
pub const IDLE_RETIREMENT_SECONDS: u64 = 60 * 60;
/// #219: a never-started game with no seated player for this long retires as inactive.
pub const UNSTARTED_RETIREMENT_SECONDS: u64 = 15 * 60;
/// #219: retry hint returned with `at-capacity` when every game has a connected player.
pub const CAPACITY_RETRY_SECONDS: u64 = 30;

/// Wall-clock activity the sweeper last observed for one created game.
#[derive(Clone, Copy)]
struct Activity {
    /// Last time a player (or the board) was connected.
    connected_at: u64,
    /// Last time the game had at least one seated player.
    seated_at: u64,
}

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
    pub selection: crate::profile_catalog::RequestedSelection,
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
/// Decision 3.8. Both approved timeout consequences are supported.
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
    /// Retired early by the inactivity policy (#219).
    InactiveMatch,
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
    /// Sweeper observations per created game; in memory, so a restart starts a fresh window.
    activity: Mutex<HashMap<String, Activity>>,
    /// Games retired for inactivity -> their original expiry, so returning players learn why.
    inactive: Mutex<HashMap<String, u64>>,
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
            activity: Mutex::new(HashMap::new()),
            inactive: Mutex::new(HashMap::new()),
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
        self.place_created(&id, request.requested_at, now).await
    }
    /// Runs one guest board-layout library operation off the async executor (#101).
    pub async fn with_layouts<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&crate::layout_library::LayoutLibrary) -> T + Send + 'static,
    ) -> Result<T, GameOperationError> {
        let factory = self.factory.clone();
        tokio::task::spawn_blocking(move || operation(factory.layouts()))
            .await
            .map_err(|_| GameOperationError::Internal)
    }
    /// Runs one guest tile-profile library operation off the async executor (#100).
    pub async fn with_tile_profiles<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&crate::tile_profile_library::TileProfileLibrary) -> T + Send + 'static,
    ) -> Result<T, GameOperationError> {
        let factory = self.factory.clone();
        tokio::task::spawn_blocking(move || operation(factory.tile_profiles()))
            .await
            .map_err(|_| GameOperationError::Internal)
    }
    /// The configured guest word-list bounds (#97, Decision 1.50).
    pub fn word_list_limits(&self) -> crate::word_list_library::WordListLimits {
        *self.factory.word_lists().limits()
    }
    /// Runs one guest word-list library operation off the async executor (#97).
    pub async fn with_word_lists<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&crate::word_list_library::WordListLibrary) -> T + Send + 'static,
    ) -> Result<T, GameOperationError> {
        let factory = self.factory.clone();
        tokio::task::spawn_blocking(move || operation(factory.word_lists()))
            .await
            .map_err(|_| GameOperationError::Internal)
    }
    pub fn hex_profiles(&self) -> serde_json::Value {
        self.factory.hex_profile_catalog()
    }
    pub fn profiles(&self) -> Option<serde_json::Value> {
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
            if existing.selection != request.selection.selection()
                || crate::profiles::MatchRules::from(&existing.configuration.rules) != request.rules
                || existing.configuration.rules.time_control != request.time_control
            {
                return Err(ConfiguredError::Conflict);
            }
        } else {
            // Saved guest versions are read and fingerprinted from disk; keep that off
            // the async executor.
            let factory = self.factory.clone();
            let (selection, rules, time_control) = (
                request.selection.clone(),
                request.rules,
                request.time_control,
            );
            let prepared = tokio::task::spawn_blocking(move || {
                factory.resolve(
                    &selection.selection(),
                    selection.word_list_access.as_ref(),
                    rules,
                    time_control,
                )
            })
            .await
            .map_err(|_| GameOperationError::Internal)?
            .map_err(ConfiguredError::Selection)?;
            if !self.host.is_serving() {
                return Err(GameOperationError::NotServing.into());
            }
            if self.host.is_draining().await {
                return Err(GameOperationError::Draining.into());
            }
            self.make_room(now).await?;
            let factory = self.factory.clone();
            let initialize_id = id.clone();
            tokio::task::spawn_blocking(move || factory.initialize(&initialize_id, prepared))
                .await
                .map_err(|_| GameOperationError::Internal)?
                .map_err(|_| GameOperationError::Internal)?;
        }
        let result = self.place_created(&id, request.requested_at, now).await;
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
        now: u64,
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
            self.make_room(now).await?;
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
        let join = self.place_created(&id, request.requested_at, now).await?;
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
        if self.inactive.lock().await.contains_key(id.as_str()) {
            return Err(GameOperationError::InactiveMatch);
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
    /// Explicit retirement (`DELETE`): only lifetime-expired or finished-and-retained games.
    pub async fn retire(&self, id: &MatchId, now: u64) -> Result<(), GameOperationError> {
        // Serialized with creation so a racing rematch cannot link a match retired meanwhile.
        let _creation = self.creation.lock().await;
        if !self.observe(id, now).await?.due {
            return Err(GameOperationError::NotRetirable);
        }
        self.retire_locked(id, false).await
    }
    /// Lifecycle decode for the sweeper: hard expiry/finished grace, phase and seated players.
    async fn observe(&self, id: &MatchId, now: u64) -> Result<Observation, GameOperationError> {
        self.host
            .inspect(id, |runtime| {
                let snapshot = runtime
                    .snapshot()
                    .map_err(|_| GameOperationError::Internal)?;
                #[derive(Deserialize)]
                struct GameHeader {
                    phase: scrabble_game::protocol::Phase,
                    players: Vec<serde::de::IgnoredAny>,
                }
                #[derive(Deserialize)]
                struct Projection {
                    game: GameHeader,
                }
                let current: Projection = serde_json::from_slice(&snapshot.payload)
                    .map_err(|_| GameOperationError::Internal)?;
                Ok(Observation {
                    due: retirement_due(&snapshot.payload, now, runtime.current_tick())
                        .map_err(|_| GameOperationError::Internal)?,
                    lobby: matches!(current.game.phase, scrabble_game::protocol::Phase::Lobby {}),
                    seated: !current.game.players.is_empty(),
                })
            })
            .await?
    }
    /// Callers hold the creation lock. Recovery state goes with the runtime.
    async fn retire_locked(&self, id: &MatchId, inactive: bool) -> Result<(), GameOperationError> {
        self.host.retire(id).await?;
        self.rematches.lock().await.remove(id.as_str());
        self.activity.lock().await.remove(id.as_str());
        if inactive && let Some(created_at) = creation_time(id) {
            self.inactive.lock().await.insert(
                id.as_str().to_owned(),
                created_at.saturating_add(MATCH_LIFETIME_SECONDS),
            );
        }
        self.factory
            .retire_initialization(id)
            .map_err(|_| GameOperationError::Internal)?;
        Ok(())
    }
    /// At the match cap, evicts the least-recently-active created game with nobody connected.
    /// Callers hold the creation lock.
    async fn make_room(&self, now: u64) -> Result<(), GameOperationError> {
        let statuses = self.host.statuses().await;
        if statuses.len() < self.host.max_matches() {
            return Ok(());
        }
        let victim = {
            let activity = self.activity.lock().await;
            statuses
                .iter()
                .filter(|status| {
                    status.active_players == 0
                        && !status.draining
                        && !status.frozen
                        && creation_time(&status.id).is_some()
                })
                .min_by_key(|status| {
                    activity
                        .get(status.id.as_str())
                        .map_or(now, |seen| seen.connected_at)
                })
                .map(|status| status.id.clone())
        };
        let Some(victim) = victim else {
            return Err(GameOperationError::AtCapacity);
        };
        self.retire_locked(&victim, true).await
    }
    pub async fn sweep(&self, now: u64) {
        self.inactive
            .lock()
            .await
            .retain(|_, expires_at| now < *expires_at);
        for status in self.host.statuses().await {
            if status.draining || status.frozen || creation_time(&status.id).is_none() {
                continue;
            }
            let _creation = self.creation.lock().await;
            let observed = match self.observe(&status.id, now).await {
                Ok(observed) => observed,
                Err(GameOperationError::UnknownMatch | GameOperationError::Draining) => continue,
                Err(_) => {
                    eprintln!("game retirement failed");
                    continue;
                }
            };
            let seen = {
                let mut activity = self.activity.lock().await;
                let seen = activity
                    .entry(status.id.as_str().to_owned())
                    .or_insert(Activity {
                        connected_at: now,
                        seated_at: now,
                    });
                if status.active_players > 0 {
                    seen.connected_at = now;
                }
                if observed.seated {
                    seen.seated_at = now;
                }
                *seen
            };
            let inactive = now.saturating_sub(seen.connected_at) >= IDLE_RETIREMENT_SECONDS
                || (observed.lobby
                    && now.saturating_sub(seen.seated_at) >= UNSTARTED_RETIREMENT_SECONDS);
            if !observed.due && !inactive {
                continue;
            }
            match self.retire_locked(&status.id, !observed.due).await {
                Ok(())
                | Err(
                    GameOperationError::UnknownMatch
                    | GameOperationError::Draining
                    | GameOperationError::NotServing,
                ) => {}
                Err(_) => eprintln!("game retirement failed"),
            }
        }
    }
}

struct Observation {
    due: bool,
    lobby: bool,
    seated: bool,
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
                ("SCRABBLE_STATE_DIR".into(), directory.path().to_str().unwrap().into()),
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

    #[tokio::test]
    async fn inactive_games_retire_and_the_least_active_unconnected_game_is_evicted_at_capacity() {
        use game_server::{
            MatchHost, MatchHostWebTransportConfig, RejectMatchControlService,
            serve_live_match_host_with_control_and_shutdown,
        };
        use std::{collections::BTreeMap, time::Duration};
        use tokio::{sync::mpsc, task::JoinSet};
        use wtransport::Identity;
        tokio::time::timeout(Duration::from_secs(10), async {
            let directory = tempfile::tempdir().unwrap();
            let identity = Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
            let cert = directory.path().join("cert.pem");
            let key = directory.path().join("key.pem");
            identity.certificate_chain().store_pemfile(&cert).await.unwrap();
            identity.private_key().store_secret_pemfile(&key).await.unwrap();
            let config = crate::config::ServerConfig::from_values(&BTreeMap::from([
                ("SCRABBLE_MATCH_IDS".into(), String::new()),
                ("SCRABBLE_STATE_DIR".into(), directory.path().to_str().unwrap().into()),
            ])).unwrap();
            let host = LiveMatchHost::new(MatchHost::new(2).unwrap());
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
            let create = |byte: &str, at: u64| CreateGame { version: 2, request_id: byte.repeat(16), requested_at: at };
            let first = games.create(&create("01", now), now).await.unwrap();
            games.sweep(now).await;
            let second = games.create(&create("02", now + 60), now + 60).await.unwrap();
            games.sweep(now + 60).await;
            // At the cap of two, the least recently active unconnected game makes room.
            let third = games.create(&create("03", now + 120), now + 120).await.unwrap();
            let first_id = MatchId::new(&first.match_id).unwrap();
            assert_eq!(games.lookup(&first_id, now + 120).await, Err(GameOperationError::InactiveMatch));
            let second_id = MatchId::new(&second.match_id).unwrap();
            assert_eq!(games.lookup(&second_id, now + 120).await.unwrap(), second);
            // Unstarted with no seated player: retired after the unstarted window, not before.
            games.sweep(now + 60 + UNSTARTED_RETIREMENT_SECONDS - 1).await;
            assert_eq!(host.statuses().await.len(), 2);
            games.sweep(now + 60 + UNSTARTED_RETIREMENT_SECONDS).await;
            assert_eq!(games.lookup(&second_id, now + 60 + UNSTARTED_RETIREMENT_SECONDS).await, Err(GameOperationError::InactiveMatch));
            let third_id = MatchId::new(&third.match_id).unwrap();
            assert!(games.lookup(&third_id, now + 60 + UNSTARTED_RETIREMENT_SECONDS).await.is_ok());
            assert_eq!(host.statuses().await.len(), 1);
            shutdown.send(()).await.unwrap();
            tasks.join_next().await.unwrap().unwrap().unwrap();
        }).await.expect("inactivity retirement must finish against the live runtime");
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
