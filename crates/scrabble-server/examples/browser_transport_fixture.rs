//! Loopback browser compatibility fixture. Deliberately contains no Scrabble rules.
use game_server::{
    BrowserRoutePrefix, ControlContext, ControlService, ControlServiceError, GameSimulation,
    SimulationError, SimulationSnapshot, WebTransportConfig, serve_with_control_and_shutdown,
};
use serde_json::json;
use std::{error::Error, path::PathBuf, time::Duration};
use tokio::sync::mpsc;
use wtransport::Identity;

#[derive(Default)]
struct Fixture {
    tick: u64,
    sequence: u32,
}
impl GameSimulation for Fixture {
    fn tick_hz(&self) -> u16 {
        20
    }
    fn max_players(&self) -> usize {
        4
    }
    fn current_tick(&self) -> u64 {
        self.tick
    }
    fn add_player(&mut self, _id: u32) -> Result<(), SimulationError> {
        Ok(())
    }
    fn remove_player(&mut self, _id: u32) -> bool {
        true
    }
    fn apply_command(
        &mut self,
        _id: u32,
        sequence: u32,
        payload: &[u8],
    ) -> Result<(), SimulationError> {
        if payload != b"fixture-command" {
            return Err(SimulationError::new("unknown fixture command"));
        }
        self.sequence = sequence;
        Ok(())
    }
    fn advance_tick(&mut self) -> Result<(), SimulationError> {
        self.tick += 1;
        Ok(())
    }
    fn snapshot(&self) -> Result<SimulationSnapshot, SimulationError> {
        // Always above an ordinary QUIC datagram budget, through the real runtime fragmentation.
        let payload = serde_json::to_vec(
            &json!({"version": 1, "sequence": self.sequence, "padding": "x".repeat(12_000)}),
        )
        .map_err(|error| SimulationError::new(error.to_string()))?;
        Ok(SimulationSnapshot::new(self.tick, payload))
    }
}
struct Control;
impl ControlService for Control {
    fn handle(
        &self,
        context: ControlContext,
        payload: &[u8],
    ) -> Result<Vec<u8>, ControlServiceError> {
        match payload {
            b"ping" => {
                Ok(format!("pong:{}:{}", context.player_id, context.connection_epoch).into_bytes())
            }
            b"delay" => {
                std::thread::sleep(Duration::from_millis(200));
                Ok(b"done".to_vec())
            }
            b"stall" => {
                std::thread::sleep(Duration::from_secs(6));
                Ok(b"late".to_vec())
            }
            b"reject" => Err(ControlServiceError::new("fixture rejection")),
            _ => Err(ControlServiceError::new("unknown fixture control")),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let directory = PathBuf::from(std::env::args().nth(1).ok_or("missing fixture directory")?);
    let identity = Identity::self_signed(["localhost", "127.0.0.1"])?;
    let certificate = &identity.certificate_chain().as_slice()[0];
    let certificate_pem = directory.join("cert.pem");
    let private_key_pem = directory.join("key.pem");
    certificate.store_pemfile(&certificate_pem).await?;
    // Sensitive key permissions are established before writing the secret.
    let key_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&private_key_pem)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        key_file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    drop(key_file);
    identity
        .private_key()
        .store_secret_pemfile(&private_key_pem)
        .await?;
    // OS-assigned UDP port, reused immediately by the runtime listener.
    let socket = std::net::UdpSocket::bind("127.0.0.1:0")?;
    let port = socket.local_addr()?.port();
    drop(socket);
    let prefix = BrowserRoutePrefix::new("/game")?;
    let match_id = game_server::MatchId::new("transport-fixture")?;
    let metadata = json!({ "endpoint": format!("https://127.0.0.1:{port}/game"),
        "matchId": match_id.as_str(), "certificateHash": certificate.hash().as_ref() });
    std::fs::write(
        directory.join("connection.json"),
        serde_json::to_vec(&metadata)?,
    )?;
    let (shutdown_sender, shutdown_receiver) = mpsc::channel(1);
    let signal = tokio::spawn(async move {
        tokio::signal::ctrl_c().await?;
        shutdown_sender
            .send(())
            .await
            .map_err(std::io::Error::other)
    });
    let result = serve_with_control_and_shutdown(
        Fixture::default(),
        Control,
        600,
        WebTransportConfig {
            port,
            certificate_pem,
            private_key_pem,
            session_path: prefix.match_path(&match_id),
            recovery_path: None,
            drain_grace: Duration::from_millis(100),
        },
        shutdown_receiver,
    )
    .await;
    signal.abort();
    let _ = signal.await;
    result?;
    Ok(())
}
