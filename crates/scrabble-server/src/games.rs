//! Public creation information and bounded lifecycle policy; gameplay authority stays in the runtime.
use crate::{
    factory::{MatchFactory, game_id},
    simulation::{MATCH_LIFETIME_SECONDS, ScrabbleSimulation, retirement_due},
};
use game_server::{BrowserRoutePrefix, LiveHostError, LiveMatchHost, MatchId, MatchRuntime};
use scrabble_game::identity::GameId;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
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
        if self.version != 1
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
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
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
    NotServing,
    Draining,
    AtCapacity,
    UnknownMatch,
    NotRetirable,
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
        }
    }
    pub async fn create(
        &self,
        request: &CreateGame,
        now: u64,
    ) -> Result<JoinInformation, GameOperationError> {
        request.validate(now)?;
        let _creation = self.creation.lock().await;
        let id = self
            .factory
            .creation_id(&request.request_id, request.requested_at)
            .map_err(|_| GameOperationError::Internal)?;
        let statuses = self.host.statuses().await;
        // Even a duplicate retry is rejected during drain. Placement remains the final authority.
        if !self.host.is_serving() {
            return Err(GameOperationError::NotServing);
        }
        if self.host.is_draining().await {
            return Err(GameOperationError::Draining);
        }
        if statuses.iter().any(|status| status.id == id) {
            // A read lease verifies that retirement has not already fenced this match.
            self.host
                .inspect(&id, |runtime| {
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
                .any(|status| game_id(&status.id) == game_id(&id))
            {
                return Err(GameOperationError::Internal);
            }
            let simulation = self
                .factory
                .create(&id)
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
            game_id: game_id(&id),
            match_path: self.prefix.match_path(&id),
            expires_at: request.requested_at.saturating_add(MATCH_LIFETIME_SECONDS),
        })
    }
    pub async fn retire(&self, id: &MatchId, now: u64) -> Result<(), GameOperationError> {
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
                    route_prefix: config.route_prefix, drain_grace: Duration::ZERO }, receiver).await });
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
