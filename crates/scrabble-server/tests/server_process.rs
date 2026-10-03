//! Real executable, real hosted runtime, real loopback WebTransport. No browser or game-rule double.
#![cfg(unix)]
use game_server::{SnapshotReassembler, Welcome, decode_welcome, encode_command};
use scrabble_game::{
    identity::{GameId, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Phase, PlayerSnapshot},
};
use std::{
    collections::BTreeMap,
    fs,
    net::{TcpListener, UdpSocket},
    path::PathBuf,
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command as ProcessCommand},
    time::{sleep, timeout},
};
use wtransport::{ClientConfig, Connection, Endpoint, Identity, tls::Sha256Digest};

struct Server {
    directory: tempfile::TempDir,
    values: BTreeMap<String, String>,
    certificate_hash: Sha256Digest,
    child: Option<Child>,
    status_port: u16,
    port: u16,
    log: PathBuf,
}
impl Server {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let identity = Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
        let certificate_hash = identity.certificate_chain().as_slice()[0].hash();
        let cert = directory.path().join("cert.pem");
        let key = directory.path().join("key.pem");
        identity.certificate_chain().as_slice()[0]
            .store_pemfile(&cert)
            .await
            .unwrap();
        identity
            .private_key()
            .store_secret_pemfile(&key)
            .await
            .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let status_port = listener.local_addr().unwrap().port();
        drop(listener);
        let values = BTreeMap::from([
            ("SCRABBLE_PORT".into(), port.to_string()),
            ("SCRABBLE_STATUS_PORT".into(), status_port.to_string()),
            ("SCRABBLE_CERT_PEM".into(), cert.to_str().unwrap().into()),
            ("SCRABBLE_KEY_PEM".into(), key.to_str().unwrap().into()),
            (
                "SCRABBLE_SEED_FILE".into(),
                directory.path().join("seed").to_str().unwrap().into(),
            ),
            (
                "SCRABBLE_RECOVERY_DIR".into(),
                directory.path().join("recovery").to_str().unwrap().into(),
            ),
            ("SCRABBLE_MATCH_IDS".into(), "alpha,beta".into()),
            ("SCRABBLE_TEST_SEED".into(), "07".repeat(32)),
            ("SCRABBLE_DRAIN_GRACE_MS".into(), "150".into()),
            ("SCRABBLE_RECONNECT_GRACE_TICKS".into(), "1200".into()),
        ]);
        let log = directory.path().join("server.log");
        Self {
            directory,
            values,
            certificate_hash,
            child: None,
            status_port,
            port,
            log,
        }
    }
    async fn start(&mut self) {
        self.child = Some(
            ProcessCommand::new(env!("CARGO_BIN_EXE_scrabble-server"))
                .env_clear()
                .envs(&self.values)
                .kill_on_drop(true)
                .stdout(Stdio::null())
                .stderr(fs::File::create(&self.log).unwrap())
                .spawn()
                .unwrap(),
        );
        timeout(Duration::from_secs(10), async {
            loop {
                if self.child.as_mut().unwrap().try_wait().unwrap().is_some() {
                    panic!(
                        "server exited before readiness: {}",
                        fs::read_to_string(&self.log).unwrap()
                    );
                }
                if let Ok((status, _)) = self.get("/readyz").await
                    && status == 200
                {
                    break;
                }
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "readiness timeout: {}",
                fs::read_to_string(&self.log).unwrap()
            )
        });
    }
    async fn get(
        &self,
        path: &str,
    ) -> Result<(u16, serde_json::Value), Box<dyn std::error::Error>> {
        timeout(Duration::from_secs(2), async {
            let mut socket =
                tokio::net::TcpStream::connect(("127.0.0.1", self.status_port)).await?;
            socket
                .write_all(
                    format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                )
                .await?;
            let mut bytes = Vec::new();
            socket.take(128 * 1024).read_to_end(&mut bytes).await?;
            let text = String::from_utf8(bytes)?;
            let (headers, body) = text.split_once("\r\n\r\n").ok_or("missing HTTP headers")?;
            let status: u16 = headers
                .split_whitespace()
                .nth(1)
                .ok_or("missing HTTP status")?
                .parse()?;
            Ok::<_, Box<dyn std::error::Error>>((status, serde_json::from_str(body)?))
        })
        .await?
    }
    async fn stop(&mut self) {
        let mut child = self.child.take().unwrap();
        let pid = child.id().unwrap();
        let signal = timeout(
            Duration::from_secs(2),
            ProcessCommand::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(signal.status.success());
        assert!(
            timeout(Duration::from_secs(10), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success(),
            "{}",
            fs::read_to_string(&self.log).unwrap()
        );
    }
    fn client(&self) -> Endpoint<wtransport::endpoint::endpoint_side::Client> {
        Endpoint::client(
            ClientConfig::builder()
                .with_bind_default()
                .with_server_certificate_hashes([self.certificate_hash.clone()])
                .keep_alive_interval(Some(Duration::from_secs(1)))
                .build(),
        )
        .unwrap()
    }
    async fn connect(
        &self,
        client: &Endpoint<wtransport::endpoint::endpoint_side::Client>,
        route: &str,
    ) -> (Connection, Welcome) {
        let connection = timeout(
            Duration::from_secs(5),
            client.connect(format!("https://127.0.0.1:{}{route}", self.port)),
        )
        .await
        .unwrap()
        .unwrap();
        let mut stream = timeout(Duration::from_secs(5), connection.accept_uni())
            .await
            .unwrap()
            .unwrap();
        let mut bytes = [0; 46];
        stream.read_exact(&mut bytes).await.unwrap();
        (connection, decode_welcome(&bytes).unwrap())
    }
}
async fn snapshot(
    connection: &Connection,
    condition: impl Fn(&PlayerSnapshot) -> bool,
) -> PlayerSnapshot {
    timeout(Duration::from_secs(5), async {
        let mut reassembler = SnapshotReassembler::new();
        loop {
            let bytes = connection.receive_datagram().await.unwrap();
            if let Some(snapshot) = reassembler.accept(&bytes).unwrap() {
                let snapshot: PlayerSnapshot = serde_json::from_slice(&snapshot.payload).unwrap();
                if condition(&snapshot) {
                    return snapshot;
                }
            }
        }
    })
    .await
    .unwrap()
}
fn send(
    connection: &Connection,
    snapshot: &PlayerSnapshot,
    player: &Welcome,
    sequence: u32,
    turn: u64,
    command: Command,
) {
    let payload = serde_json::to_vec(&CommandEnvelope {
        version: ProtocolVersion,
        game_id: snapshot.public.game_id,
        player_id: PlayerId::new(u64::from(player.player_id)),
        sequence,
        expected_turn: TurnId::new(turn),
        command,
    })
    .unwrap();
    connection
        .send_datagram(encode_command(sequence, &payload).unwrap())
        .unwrap();
}

#[tokio::test]
async fn two_matches_are_independent_and_shutdown_restart_preserves_reconnect_authority() {
    let mut server = Server::new().await;
    server.start().await;
    let (status, health) = server.get("/healthz").await.unwrap();
    assert_eq!(status, 200);
    assert_eq!(health["healthy"], true);
    let (status, readiness) = server.get("/readyz").await.unwrap();
    assert_eq!(status, 200);
    assert_eq!(readiness["ready"], true);
    let (_, status) = server.get("/status").await.unwrap();
    assert_eq!(status["capacity"]["hostedMatches"], 2);
    assert_eq!(status["capacity"]["playerCapacity"], 8);
    for id in ["alpha", "beta"] {
        assert_eq!(
            server
                .get(&format!("/matches/{id}/readyz"))
                .await
                .unwrap()
                .0,
            200
        );
    }
    assert_eq!(server.get("/matches/unknown/readyz").await.unwrap().0, 404);
    let client = server.client();
    let (alpha, alpha_player) = server.connect(&client, "/game/matches/alpha").await;
    let (alpha_second, _) = server.connect(&client, "/game/matches/alpha").await;
    let (beta, beta_player) = server.connect(&client, "/game/matches/beta").await;
    let (beta_second, _) = server.connect(&client, "/game/matches/beta").await;
    assert_eq!(alpha_player.player_id, 1);
    assert_eq!(beta_player.player_id, 1);
    let alpha_lobby = snapshot(&alpha, |snapshot| snapshot.public.players.len() == 2).await;
    let beta_lobby = snapshot(&beta, |snapshot| snapshot.public.players.len() == 2).await;
    assert_ne!(alpha_lobby.public.game_id, beta_lobby.public.game_id);
    assert_ne!(alpha_lobby.public.game_id, GameId::new(0));
    send(&alpha, &alpha_lobby, &alpha_player, 1, 0, Command::Start {});
    let alpha_started = snapshot(&alpha, |snapshot| {
        matches!(snapshot.public.phase, Phase::Playing { .. })
    })
    .await;
    let beta_unchanged = snapshot(&beta, |_| true).await;
    assert!(matches!(beta_unchanged.public.phase, Phase::Lobby {}));
    assert!(beta_unchanged.own_rack.tiles.is_empty());
    send(&beta, &beta_lobby, &beta_player, 1, 0, Command::Start {});
    let beta_started = snapshot(&beta, |snapshot| {
        matches!(snapshot.public.phase, Phase::Playing { .. })
    })
    .await;
    let alpha_private_second = snapshot(&alpha_second, |snapshot| {
        matches!(snapshot.public.phase, Phase::Playing { .. })
    })
    .await;
    assert_ne!(
        alpha_started.own_rack.tiles,
        alpha_private_second.own_rack.tiles
    );
    let seed_before = fs::read(server.directory.path().join("seed")).unwrap();
    server.stop().await;
    let recovery = server.directory.path().join("recovery");
    assert!(recovery.join("manifest").exists());
    assert!(recovery.join("alpha.recovery").exists());
    assert!(recovery.join("beta.recovery").exists());
    assert!(server.get("/readyz").await.is_err());
    server.start().await;
    assert!(!recovery.exists());
    assert_eq!(
        fs::read(server.directory.path().join("seed")).unwrap(),
        seed_before
    );
    let token = game_server::ReconnectToken(alpha_player.reconnect_token).encode_hex();
    let (reconnected, admission) = server
        .connect(&client, &format!("/game/matches/alpha/reconnect/{token}"))
        .await;
    assert_eq!(admission.player_id, alpha_player.player_id);
    assert_eq!(
        admission.connection_epoch,
        alpha_player.connection_epoch + 1
    );
    let restored = snapshot(&reconnected, |_| true).await;
    assert_eq!(restored, alpha_started);
    // Sequence 1 was applied before restart; it must not pass as a new command after reconnect.
    send(&reconnected, &restored, &admission, 1, 0, Command::Pass {});
    for _ in 0..3 {
        assert_eq!(
            snapshot(&reconnected, |_| true).await.public.phase,
            restored.public.phase
        );
    }
    send(&reconnected, &restored, &admission, 2, 0, Command::Pass {});
    snapshot(&reconnected, |snapshot| matches!(snapshot.public.phase, Phase::Playing { turn, .. } if turn == TurnId::new(1))).await;
    let beta_token = game_server::ReconnectToken(beta_player.reconnect_token).encode_hex();
    let (restored_beta, _) = server
        .connect(
            &client,
            &format!("/game/matches/beta/reconnect/{beta_token}"),
        )
        .await;
    assert_eq!(snapshot(&restored_beta, |_| true).await, beta_started);
    reconnected.close(0u32.into(), b"test complete");
    restored_beta.close(0u32.into(), b"test complete");
    alpha.close(0u32.into(), b"test complete");
    beta.close(0u32.into(), b"test complete");
    alpha_second.close(0u32.into(), b"test complete");
    beta_second.close(0u32.into(), b"test complete");
    server.stop().await;
}
