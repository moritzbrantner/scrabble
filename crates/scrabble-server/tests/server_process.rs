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

#[path = "support/profile_catalog.rs"]
mod profile_catalog;

struct Server {
    directory: tempfile::TempDir,
    values: BTreeMap<String, String>,
    certificate_hash: Sha256Digest,
    child: Option<Child>,
    status_port: u16,
    api_port: u16,
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
        let api = TcpListener::bind("127.0.0.1:0").unwrap();
        let api_port = api.local_addr().unwrap().port();
        drop(api);
        let values = BTreeMap::from([
            ("SCRABBLE_PORT".into(), port.to_string()),
            ("SCRABBLE_STATUS_PORT".into(), status_port.to_string()),
            ("SCRABBLE_API_PORT".into(), api_port.to_string()),
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
            api_port,
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
async fn forged_wire_commands_close_only_the_attacker_and_do_not_disclose_private_state() {
    let mut server = Server::new().await;
    server.start().await;
    let client = server.client();
    let (mut attacker, mut lease) = server.connect(&client, "/game/matches/alpha").await;
    let (peer, peer_lease) = server.connect(&client, "/game/matches/alpha").await;
    let lobby = snapshot(&attacker, |view| view.public.players.len() == 2).await;
    send(&attacker, &lobby, &lease, 1, 0, Command::Start {});
    let initial = snapshot(&attacker, |view| {
        matches!(view.public.phase, Phase::Playing { .. })
    })
    .await;
    let other = snapshot(&peer, |view| {
        matches!(view.public.phase, Phase::Playing { .. })
    })
    .await;
    assert_eq!(initial.own_rack.player_id.get(), u64::from(lease.player_id));
    assert!(
        initial.own_rack.tiles.iter().all(|tile| !other
            .own_rack
            .tiles
            .iter()
            .any(|peer| peer.id == tile.id))
    );
    let marker = "private-command-marker-abcdef0123456789";
    let mut tokens = Vec::new();
    for attack in 0..6 {
        let selector = game_server::encode_control_request(
            format!(
                "{{\"scope\":\"canonical\",\"player_id\":{},\"marker\":\"{marker}\"}}",
                peer_lease.player_id
            )
            .as_bytes(),
        )
        .unwrap();
        let (mut send_stream, mut receive) = timeout(Duration::from_secs(5), attacker.open_bi())
            .await
            .unwrap()
            .unwrap()
            .await
            .unwrap();
        send_stream.write_all(&selector).await.unwrap();
        send_stream.finish().await.unwrap();
        let mut response = [0; game_server::CONTROL_HEADER_BYTES];
        timeout(Duration::from_secs(5), receive.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        let response = game_server::decode_control_response(&response).unwrap();
        assert!(!response.accepted);
        assert!(response.payload.is_empty());
        let token = game_server::ReconnectToken(lease.reconnect_token).encode_hex();
        tokens.push(token.clone());
        let mut envelope = serde_json::to_value(CommandEnvelope {
            version: ProtocolVersion,
            game_id: initial.public.game_id,
            player_id: PlayerId::new(u64::from(lease.player_id)),
            sequence: 2,
            expected_turn: TurnId::new(0),
            command: Command::Pass {},
        })
        .unwrap();
        match attack {
            0 => {
                envelope["player_id"] = serde_json::json!(peer_lease.player_id.to_string());
            }
            1 => {
                envelope["command"] = serde_json::json!({"kind":"commit","placements":[{
                    "tile_id":other.own_rack.tiles[0].id,"coordinate":{"row":7,"column":7},"blank_as":null
                }]});
            }
            2 => {
                envelope["command"] = serde_json::json!({"kind":"preview","placements":[{
                    "tile_id":initial.own_rack.tiles[0].id,"coordinate":{"row":255,"column":7},"blank_as":null
                }]});
            }
            3 => {
                envelope["command"] =
                    serde_json::json!({"kind":"pass", marker:true, token.clone():true});
            }
            4 => {
                envelope["command"] = serde_json::json!({"kind":marker});
            }
            5 => {}
            _ => unreachable!(),
        }
        let frame = if attack == 5 {
            // Deliberately bypass the native encoder's bound, as a modified client can.
            let mut frame = vec![game_server::PROTOCOL_VERSION, 1];
            frame.extend_from_slice(&2_u32.to_be_bytes());
            frame.extend_from_slice(&1025_u16.to_be_bytes());
            frame.extend_from_slice(&vec![b'x'; 1025]);
            frame
        } else {
            encode_command(2, &serde_json::to_vec(&envelope).unwrap()).unwrap()
        };
        attacker.send_datagram(frame).unwrap();
        timeout(Duration::from_secs(5), attacker.closed())
            .await
            .unwrap();
        let unchanged = snapshot(&peer, |view| {
            view.public.players.iter().any(|player| {
                player.id.get() == u64::from(lease.player_id) && player.connected == Some(false)
            })
        })
        .await;
        assert_eq!(unchanged.own_rack, other.own_rack);
        assert_eq!(unchanged.public.phase, initial.public.phase);
        assert_eq!(unchanged.public.board, initial.public.board);
        assert_eq!(
            unchanged.public.remaining_tiles,
            initial.public.remaining_tiles
        );
        assert_eq!(unchanged.public.revision, initial.public.revision);
        assert_eq!(
            unchanged
                .public
                .players
                .iter()
                .map(|player| (player.id, player.score, player.rack_count))
                .collect::<Vec<_>>(),
            initial
                .public
                .players
                .iter()
                .map(|player| (player.id, player.score, player.rack_count))
                .collect::<Vec<_>>()
        );
        let (resumed, next) = server
            .connect(&client, &format!("/game/matches/alpha/reconnect/{token}"))
            .await;
        assert_eq!(next.player_id, lease.player_id);
        assert_eq!(next.connection_epoch, lease.connection_epoch + 1);
        let restored = snapshot(&resumed, |_| true).await;
        assert_eq!(restored.own_rack, initial.own_rack);
        assert_eq!(restored.public.phase, initial.public.phase);
        attacker = resumed;
        lease = next;
        assert_eq!(server.get("/readyz").await.unwrap().0, 200);
    }
    // Every failed request left sequence 2 available; the same player can still pass legally.
    send(&attacker, &initial, &lease, 2, 0, Command::Pass {});
    snapshot(
        &peer,
        |view| matches!(view.public.phase, Phase::Playing { turn, .. } if turn == TurnId::new(1)),
    )
    .await;
    server.stop().await;
    let diagnostic = fs::read_to_string(&server.log).unwrap();
    assert!(!diagnostic.contains(marker));
    for token in tokens {
        assert!(!diagnostic.contains(&token));
    }
    assert!(!diagnostic.contains("own_rack"));
    assert!(!diagnostic.contains("RackTile"));
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
    let mut expected = alpha_started.clone();
    for player in &mut expected.public.players {
        player.connected = Some(player.id.get() == u64::from(admission.player_id));
    }
    assert_eq!(restored, expected);
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
    let mut expected_beta = beta_started.clone();
    for player in &mut expected_beta.public.players {
        player.connected = Some(player.id.get() == u64::from(beta_player.player_id));
    }
    assert_eq!(snapshot(&restored_beta, |_| true).await, expected_beta);
    reconnected.close(0u32.into(), b"test complete");
    restored_beta.close(0u32.into(), b"test complete");
    alpha.close(0u32.into(), b"test complete");
    beta.close(0u32.into(), b"test complete");
    alpha_second.close(0u32.into(), b"test complete");
    beta_second.close(0u32.into(), b"test complete");
    server.stop().await;
}

impl Server {
    async fn api(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
        origin: &str,
    ) -> (u16, serde_json::Value) {
        timeout(Duration::from_secs(5), async {
            let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", self.api_port)).await.unwrap();
            let body = body.map(serde_json::Value::to_string).unwrap_or_default();
            let request = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nOrigin: {origin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(request.as_bytes()).await.unwrap();
            let mut bytes = Vec::new();
            // Catalog metadata includes full tile/premium arrays; retain the status helper's bounded 128 KiB envelope.
            socket.take(128 * 1024).read_to_end(&mut bytes).await.unwrap();
            let text = String::from_utf8(bytes).unwrap();
            let (headers, body) = text.split_once("\r\n\r\n").unwrap();
            assert!(headers.contains("Access-Control-Allow-Origin: http://localhost:5173"));
            let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
            (status, if body.is_empty() { serde_json::Value::Null } else { serde_json::from_str(body).unwrap() })
        }).await.unwrap()
    }
}

#[tokio::test]
async fn creation_retries_capacity_origin_and_restart_keep_one_authoritative_game_per_request() {
    let mut server = Server::new().await;
    server.values.insert("SCRABBLE_MATCH_IDS".into(), "".into());
    server
        .values
        .insert("SCRABBLE_MAX_MATCHES".into(), "2".into());
    server
        .values
        .insert("SCRABBLE_DRAIN_GRACE_MS".into(), "1000".into());
    server.start().await;
    assert_eq!(
        server.get("/status").await.unwrap().1["capacity"]["hostedMatches"],
        0
    );
    let now = scrabble_server::games::unix_seconds().unwrap();
    let request = serde_json::json!({"version":1, "requestId":"01".repeat(16), "requestedAt":now});
    let origin = "http://localhost:5173";
    assert_eq!(server.api("OPTIONS", "/games", None, origin).await.0, 204);
    assert_eq!(
        server
            .api("POST", "/games", Some(&request), "http://untrusted.example")
            .await
            .0,
        403
    );
    let (first, concurrent) = tokio::join!(
        server.api("POST", "/games", Some(&request), origin),
        server.api("POST", "/games", Some(&request), origin)
    );
    assert_eq!(first.0, 200);
    assert_eq!(concurrent, first);
    let join: scrabble_server::games::JoinInformation =
        serde_json::from_value(first.1.clone()).unwrap();
    assert_eq!(
        first.1.as_object().unwrap().len(),
        5,
        "only public join information is returned"
    );
    assert_eq!(
        server.get("/status").await.unwrap().1["capacity"]["hostedMatches"],
        1
    );
    let client = server.client();
    let (connection, welcome) = server.connect(&client, &join.match_path).await;
    let initial = snapshot(&connection, |_| true).await;
    assert_eq!(initial.public.game_id, join.game_id);
    assert!(matches!(initial.public.phase, Phase::Lobby {}));
    assert_eq!(
        server
            .api("DELETE", &format!("/games/{}", join.match_id), None, origin)
            .await
            .0,
        409
    );
    let second = serde_json::json!({"version":2, "requestId":"02".repeat(16), "requestedAt":now});
    let second_join = server.api("POST", "/games", Some(&second), origin).await;
    assert_eq!(second_join.0, 200);
    assert!(second_join.1["matchId"].as_str().unwrap().starts_with("b_"));
    assert_ne!(second_join.1["matchId"], first.1["matchId"]);
    let excess = serde_json::json!({"version":1, "requestId":"03".repeat(16), "requestedAt":now});
    assert_eq!(
        server.api("POST", "/games", Some(&excess), origin).await.0,
        429
    );
    assert_eq!(
        server.api("POST", "/games", Some(&request), origin).await,
        first,
        "retries succeed even at capacity"
    );
    let expired =
        serde_json::json!({"version":1, "requestId":"04".repeat(16), "requestedAt":now-120});
    assert_eq!(
        server.api("POST", "/games", Some(&expired), origin).await.0,
        410
    );
    let malformed =
        serde_json::json!({"version":1, "requestId":"secret-invalid", "requestedAt":now});
    assert_eq!(
        server
            .api("POST", "/games", Some(&malformed), origin)
            .await
            .0,
        400
    );
    server.stop().await;
    server.start().await;
    assert_eq!(
        server.get("/status").await.unwrap().1["capacity"]["hostedMatches"],
        2
    );
    assert_eq!(
        server.api("POST", "/games", Some(&request), origin).await,
        first
    );
    assert_eq!(
        server.api("POST", "/games", Some(&second), origin).await,
        second_join
    );
    let board_join: scrabble_server::games::JoinInformation =
        serde_json::from_value(second_join.1.clone()).unwrap();
    let (pending_board, _) = server.connect(&client, &board_join.match_path).await;
    let pending = snapshot(&pending_board, |_| true).await;
    assert!(pending.public.players.is_empty());
    assert!(pending.own_rack.tiles.is_empty());
    assert_eq!(pending.public.host.unwrap().id, None);
    let route = format!(
        "{}/reconnect/{}",
        join.match_path,
        game_server::ReconnectToken(welcome.reconnect_token).encode_hex()
    );
    let (restored, recovered) = server.connect(&client, &route).await;
    assert_eq!(recovered.player_id, welcome.player_id);
    assert!(recovered.connection_epoch > welcome.connection_epoch);
    assert_eq!(
        snapshot(&restored, |_| true).await.public.game_id,
        join.game_id
    );
    let mut child = server.child.take().unwrap();
    let signal = ProcessCommand::new("kill")
        .arg("-TERM")
        .arg(child.id().unwrap().to_string())
        .status()
        .await
        .unwrap();
    assert!(signal.success());
    timeout(Duration::from_millis(800), async {
        while server.get("/readyz").await.unwrap().0 != 503 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        server.api("POST", "/games", Some(&excess), origin).await.0,
        503
    );
    assert_eq!(
        server.api("POST", "/games", Some(&request), origin).await.0,
        503
    );
    assert!(
        timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

// Independent #94 acceptance: real executable, HTTP creation, authenticated WebTransport
// and native recovery/replay. No catalog implementation or replacement simulation here.
async fn profile_server(compact_default: bool, maximum_names: bool) -> Server {
    let mut server = Server::new().await;
    let catalog =
        profile_catalog::write_catalog(server.directory.path(), profile_catalog::LEGACY_WORDS)
            .unwrap();
    if compact_default || maximum_names {
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
        if compact_default {
            value["defaultSelection"]["board"] = "acceptance-compact-board".into();
        }
        if maximum_names {
            for (kind, field, letter) in [
                ("wordLists", "wordList", "W"),
                ("tileProfiles", "tile", "T"),
                ("boardProfiles", "board", "B"),
            ] {
                value[kind][0]["identity"]["name"] = letter.repeat(64).into();
                value["defaultSelection"][field] = letter.repeat(64).into();
            }
        }
        fs::write(&catalog, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    server
        .values
        .insert("SCRABBLE_MATCH_IDS".into(), String::new());
    server
        .values
        .insert("SCRABBLE_MAX_MATCHES".into(), "16".into());
    server.values.insert(
        "SCRABBLE_PROFILE_CATALOG_FILE".into(),
        catalog.to_str().unwrap().into(),
    );
    server.values.insert(
        "SCRABBLE_MATCH_CONFIGURATION_DIR".into(),
        server
            .directory
            .path()
            .join("configurations")
            .to_str()
            .unwrap()
            .into(),
    );
    server.start().await;
    server
}
fn catalog_identity(catalog: &serde_json::Value, kind: &str, name: &str) -> serde_json::Value {
    catalog[kind]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["identity"]["name"] == name)
        .unwrap()["identity"]
        .clone()
}
fn profile_request(catalog: &serde_json::Value, nonce: u8) -> serde_json::Value {
    serde_json::json!({"version":3,"requestId":format!("{nonce:02x}").repeat(16),"requestedAt":scrabble_server::games::unix_seconds().unwrap(),
        "selection":catalog["defaults"]["selection"], "rules":catalog["defaults"]["rules"]})
}
async fn hosted_profiles(server: &Server) -> u64 {
    server.get("/status").await.unwrap().1["capacity"]["hostedMatches"]
        .as_u64()
        .unwrap()
}

#[tokio::test]
async fn profiles_catalog_rejections_retry_conflicts_and_legacy_defaults_do_not_allocate_unwanted_games()
 {
    let mut server = profile_server(false, false).await;
    let origin = "http://localhost:5173";
    let (code, catalog) = server.api("GET", "/profiles", None, origin).await;
    assert_eq!(
        code, 200,
        "the actual configured server must expose its catalog"
    );
    assert_eq!(catalog["version"], 1);
    assert_eq!(catalog["defaults"]["legacyCompatible"], true);
    assert_eq!(catalog["wordLists"].as_array().unwrap().len(), 3);
    assert_eq!(catalog["tileProfiles"].as_array().unwrap().len(), 2);
    assert_eq!(catalog["boardProfiles"].as_array().unwrap().len(), 2);
    let operator: serde_json::Value = serde_json::from_slice(
        &fs::read(server.directory.path().join("profile-catalog.json")).unwrap(),
    )
    .unwrap();
    for index in 0..2 {
        assert_eq!(
            catalog["tileProfiles"][index]["tiles"],
            operator["tileProfiles"][index]["tiles"]
        );
        assert_eq!(
            catalog["boardProfiles"][index]["boardSize"],
            operator["boardProfiles"][index]["boardSize"]
        );
        assert_eq!(
            catalog["boardProfiles"][index]["premiums"],
            operator["boardProfiles"][index]["premiums"]
        );
    }
    let public = catalog.to_string();
    for private in [
        "profile-pairs.txt",
        "profile-legacy-words.txt",
        "requestId",
        "reconnect",
        "AA\\nAB",
        "seed",
    ] {
        assert!(
            !public.contains(private),
            "catalog leaked private/operator content: {private}"
        );
    }
    assert_eq!(hosted_profiles(&server).await, 0);
    let valid = profile_request(&catalog, 0x41);
    assert!(
        serde_json::to_vec(&valid).unwrap().len() > 512,
        "the actual full triple exceeds the old request limit"
    );
    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("selection");
    let mut unknown = valid.clone();
    unknown["selection"]["tile"]["name"] = "absent-profile".into();
    let mut stale = valid.clone();
    stale["selection"]["board"]["revision"] = "stale-revision".into();
    let mut incompatible = valid.clone();
    incompatible["selection"]["wordList"] =
        catalog_identity(&catalog, "wordLists", "acceptance-unicode");
    let mut invalid_rack = valid.clone();
    invalid_rack["rules"]["rack_size"] = 0.into();
    let mut invalid_players = valid.clone();
    invalid_players["rules"]["minimum_players"] = 1.into();
    for (body, reason) in [
        (&missing, "invalid-request"),
        (&unknown, "unknown-profile"),
        (&stale, "unknown-profile"),
        (&incompatible, "incompatible-profiles"),
        (&invalid_rack, "invalid-match-rules"),
        (&invalid_players, "invalid-match-rules"),
    ] {
        let response = server.api("POST", "/games", Some(body), origin).await;
        assert_eq!(response.0, 400, "{reason}: {}", response.1);
        assert_eq!(response.1["error"], reason);
        if reason == "incompatible-profiles" {
            assert_eq!(response.1["letters"], serde_json::json!(["Ö"]));
        }
        assert_eq!(
            hosted_profiles(&server).await,
            0,
            "a rejection must not allocate"
        );
    }
    let mut oversized = valid.clone();
    oversized["padding"] = "x".repeat(2048).into();
    let too_large = server.api("POST", "/games", Some(&oversized), origin).await;
    assert_eq!(too_large.0, 413);
    assert_eq!(too_large.1["error"], "body-too-large");
    assert_eq!(hosted_profiles(&server).await, 0);
    let mut extra_field = valid.clone();
    extra_field["padding"] = "not an allowed creation field".into();
    let extra = server
        .api("POST", "/games", Some(&extra_field), origin)
        .await;
    assert_eq!(extra.0, 400);
    assert_eq!(extra.1["error"], "invalid-request");
    assert_eq!(hosted_profiles(&server).await, 0);
    let (first, retry) = tokio::join!(
        server.api("POST", "/games", Some(&valid), origin),
        server.api("POST", "/games", Some(&valid), origin)
    );
    assert_eq!(first.0, 200);
    assert_eq!(retry, first);
    assert_eq!(hosted_profiles(&server).await, 1);
    for field in ["wordList", "tile", "board", "rules"] {
        let mut conflict = valid.clone();
        match field {
            "wordList" => {
                conflict["selection"][field] =
                    catalog_identity(&catalog, "wordLists", "acceptance-pairs")
            }
            "tile" => {
                conflict["selection"][field] =
                    catalog_identity(&catalog, "tileProfiles", "acceptance-valued-tiles")
            }
            "board" => {
                conflict["selection"][field] =
                    catalog_identity(&catalog, "boardProfiles", "acceptance-compact-board")
            }
            _ => conflict["rules"]["bingo_bonus"] = 17.into(),
        }
        let response = server.api("POST", "/games", Some(&conflict), origin).await;
        assert_eq!(response.0, 409);
        assert_eq!(response.1["error"], "creation-conflict");
        assert_eq!(hosted_profiles(&server).await, 1);
    }
    // Each source can vary on its own; the other two identities remain exactly selected.
    for (index, (field, kind, name)) in [
        ("wordList", "wordLists", "acceptance-pairs"),
        ("tile", "tileProfiles", "acceptance-valued-tiles"),
        ("board", "boardProfiles", "acceptance-compact-board"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut changed = profile_request(&catalog, 0x50 + u8::try_from(index).unwrap());
        changed["selection"][field] = catalog_identity(&catalog, kind, name);
        assert_eq!(
            server.api("POST", "/games", Some(&changed), origin).await.0,
            200
        );
    }
    // Untouched v2 retains its actual original complete identity, not composed provenance.
    let legacy = serde_json::json!({"version":2,"requestId":"61".repeat(16),"requestedAt":valid["requestedAt"]});
    let legacy_join = server.api("POST", "/games", Some(&legacy), origin).await;
    assert_eq!(legacy_join.0, 200);
    let client = server.client();
    let (connection, _) = server
        .connect(&client, legacy_join.1["matchPath"].as_str().unwrap())
        .await;
    let legacy_view = snapshot(&connection, |_| true).await;
    assert_eq!(
        legacy_view.public.configuration,
        scrabble_game::ruleset::english_fixture()
    );
    server.stop().await;
}

struct ConfiguredPlayers {
    board: (Connection, Welcome),
    phones: Vec<(Connection, Welcome)>,
}
async fn start_profile_match(
    server: &Server,
    client: &Endpoint<wtransport::endpoint::endpoint_side::Client>,
    join: &serde_json::Value,
    nonce: &str,
) -> ConfiguredPlayers {
    let route = join["matchPath"].as_str().unwrap();
    let board = server.connect(client, route).await;
    let view = snapshot(&board.0, |_| true).await;
    send(
        &board.0,
        &view,
        &board.1,
        1,
        0,
        Command::ClaimBoard {
            request_id: nonce.into(),
        },
    );
    snapshot(&board.0, |s| {
        s.public.host.as_ref().and_then(|h| h.id).is_some()
    })
    .await;
    let mut phones = Vec::new();
    for name in ["Ada", "Lin"] {
        let phone = server.connect(client, route).await;
        let view = snapshot(&phone.0, |_| true).await;
        send(
            &phone.0,
            &view,
            &phone.1,
            1,
            0,
            Command::SetName {
                display_name: name.into(),
            },
        );
        phones.push(phone);
    }
    let named = snapshot(&board.0, |s| {
        s.public.players.len() == 2 && s.public.players.iter().all(|p| !p.display_name.is_empty())
    })
    .await;
    if named.public.configuration.maximum_players == 2 {
        let extra = timeout(
            Duration::from_secs(5),
            client.connect(format!(
                "https://127.0.0.1:{}{}",
                server.port,
                join["matchPath"].as_str().unwrap()
            )),
        )
        .await
        .unwrap();
        if let Ok(extra) = extra {
            assert!(
                timeout(Duration::from_secs(2), extra.closed())
                    .await
                    .is_ok(),
                "selected two-player maximum must reject a third playing seat"
            );
        }
    }
    send(&board.0, &named, &board.1, 2, 0, Command::Start {});
    snapshot(&board.0, |s| {
        matches!(s.public.phase, Phase::Playing { .. })
    })
    .await;
    ConfiguredPlayers { board, phones }
}

fn nonlegacy_pair(snapshot: &PlayerSnapshot) -> Vec<scrabble_game::protocol::Placement> {
    use scrabble_game::{identity::Coordinate, protocol::Placement, ruleset::TileFace};
    let mut tiles: Vec<_> = snapshot.own_rack.tiles.iter().collect();
    let spelling = |tiles: &[&scrabble_game::protocol::RackTile]| -> String {
        tiles
            .iter()
            .map(|tile| match tile.face {
                TileFace::Letter { letter } => letter,
                TileFace::Blank {} => 'A',
            })
            .collect()
    };
    if spelling(&tiles) == "AT" {
        tiles.reverse();
    }
    assert!(
        !profile_catalog::LEGACY_WORDS
            .lines()
            .any(|word| word == spelling(&tiles)),
        "the actual dictionary probe must distinguish the two word sets"
    );
    let center = snapshot.public.configuration.board_size / 2;
    tiles
        .into_iter()
        .enumerate()
        .map(|(index, tile)| Placement {
            tile_id: tile.id,
            coordinate: Coordinate::new(center, center + u8::try_from(index).unwrap()).unwrap(),
            blank_as: matches!(tile.face, TileFace::Blank {}).then_some('A'),
        })
        .collect()
}

#[tokio::test]
async fn profiles_simultaneous_native_rules_scoring_restart_and_canonical_replay_keep_original_sources()
 {
    let mut server = profile_server(false, false).await;
    let origin = "http://localhost:5173";
    let (code, catalog) = server.api("GET", "/profiles", None, origin).await;
    assert_eq!(
        code, 200,
        "real catalog is required before lifecycle acceptance"
    );
    let mut first = profile_request(&catalog, 0x71);
    first["selection"]["wordList"] = catalog_identity(&catalog, "wordLists", "acceptance-pairs");
    first["selection"]["tile"] =
        catalog_identity(&catalog, "tileProfiles", "acceptance-valued-tiles");
    first["selection"]["board"] =
        catalog_identity(&catalog, "boardProfiles", "acceptance-compact-board");
    first["rules"] = serde_json::json!({"rack_size":2,"bingo_bonus":17,"exchange_minimum_bag":1000,"scoreless_turn_limit":2,"minimum_players":2,"maximum_players":2});
    let mut second = profile_request(&catalog, 0x72);
    second["selection"]["wordList"] = first["selection"]["wordList"].clone();
    second["rules"]["rack_size"] = 3.into();
    let (a, b) = tokio::join!(
        server.api("POST", "/games", Some(&first), origin),
        server.api("POST", "/games", Some(&second), origin)
    );
    assert_eq!((a.0, b.0), (200, 200));
    assert_ne!(a.1["matchId"], b.1["matchId"]);
    let client = server.client();
    let mut dictionary_probe = first.clone();
    dictionary_probe["requestId"] = "76".repeat(16).into();
    dictionary_probe["selection"]["wordList"] =
        catalog["defaults"]["selection"]["wordList"].clone();
    let probe_join = server
        .api("POST", "/games", Some(&dictionary_probe), origin)
        .await;
    assert_eq!(probe_join.0, 200);
    let probe = start_profile_match(
        &server,
        &client,
        &probe_join.1,
        dictionary_probe["requestId"].as_str().unwrap(),
    )
    .await;
    let probe_view = snapshot(&probe.phones[0].0, |s| {
        matches!(s.public.phase, Phase::Playing { .. })
    })
    .await;
    send(
        &probe.phones[0].0,
        &probe_view,
        &probe.phones[0].1,
        2,
        0,
        Command::Commit {
            placements: nonlegacy_pair(&probe_view),
        },
    );
    for _ in 0..3 {
        let rejected = snapshot(&probe.phones[0].0, |_| true).await;
        assert_eq!(
            rejected.public.history.len(),
            0,
            "the selected legacy dictionary rejects a word accepted by the pairs dictionary"
        );
        assert_eq!(rejected.public.phase, probe_view.public.phase);
        assert_eq!(rejected.own_rack, probe_view.own_rack);
    }
    let one =
        start_profile_match(&server, &client, &a.1, first["requestId"].as_str().unwrap()).await;
    let two = start_profile_match(
        &server,
        &client,
        &b.1,
        second["requestId"].as_str().unwrap(),
    )
    .await;
    let initial = snapshot(&one.phones[0].0, |s| {
        matches!(s.public.phase, Phase::Playing { .. })
    })
    .await;
    let other = snapshot(&two.phones[0].0, |s| {
        matches!(s.public.phase, Phase::Playing { .. })
    })
    .await;
    assert_eq!(
        serde_json::to_value(&initial.public.dictionary).unwrap(),
        first["selection"]["wordList"]
    );
    assert_eq!(
        serde_json::to_value(&initial.public.configuration.tiles).unwrap(),
        catalog["tileProfiles"][1]["tiles"]
    );
    assert_eq!(initial.own_rack.tiles.len(), 2);
    assert_eq!(other.own_rack.tiles.len(), 3);
    assert_eq!(initial.public.configuration.board_size, 7);
    assert_eq!(other.public.configuration.board_size, 15);
    assert_eq!(
        initial.public.configuration.premiums[24],
        scrabble_game::ruleset::Premium::TripleWord
    );
    assert_eq!(initial.public.remaining_tiles, 98);
    assert_eq!(other.public.remaining_tiles, 94);
    assert_ne!(initial.public.ruleset, other.public.ruleset);
    assert_eq!(initial.public.dictionary, other.public.dictionary);
    assert_eq!(initial.public.configuration.bingo_bonus, 17);
    assert_eq!(initial.public.configuration.maximum_players, 2);
    // Selected exchange threshold actually rejects, without replacing rack or advancing turn.
    send(
        &one.phones[0].0,
        &initial,
        &one.phones[0].1,
        2,
        0,
        Command::Exchange {
            tile_ids: vec![initial.own_rack.tiles[0].id],
        },
    );
    for _ in 0..3 {
        let unchanged = snapshot(&one.phones[0].0, |_| true).await;
        assert_eq!(unchanged.own_rack, initial.own_rack);
        assert_eq!(unchanged.public.phase, initial.public.phase);
    }
    let points: i32 = initial
        .own_rack
        .tiles
        .iter()
        .map(|tile| {
            i32::from(
                initial
                    .public
                    .configuration
                    .tiles
                    .iter()
                    .find(|definition| definition.face == tile.face)
                    .unwrap()
                    .value,
            )
        })
        .sum();
    let placements = nonlegacy_pair(&initial);
    send(
        &one.phones[0].0,
        &initial,
        &one.phones[0].1,
        3,
        0,
        Command::Commit { placements },
    );
    let committed = snapshot(&one.phones[0].0, |s| s.public.history.len() == 1).await;
    assert_eq!(
        committed.public.players[0].score,
        points * 3 + 17,
        "selected premiums, tile values and bingo bonus must all reach native scoring"
    );
    assert_eq!(committed.public.board.len(), 2);
    assert_eq!(
        snapshot(&two.phones[0].0, |_| true).await,
        other,
        "another live match remains unchanged"
    );
    let next = snapshot(&one.phones[1].0, |s| s.public.history.len() == 1).await;
    send(
        &one.phones[1].0,
        &next,
        &one.phones[1].1,
        2,
        1,
        Command::Pass {},
    );
    let last = snapshot(&one.phones[0].0, |s| s.public.history.len() == 2).await;
    send(
        &one.phones[0].0,
        &last,
        &one.phones[0].1,
        4,
        2,
        Command::Pass {},
    );
    let finished = snapshot(&one.phones[0].0, |s| {
        matches!(s.public.phase, Phase::Finished { .. })
    })
    .await;
    assert_eq!(
        finished.public.history.len(),
        3,
        "selected scoreless limit finishes after two passes"
    );
    server.stop().await;
    // Operator edits after shutdown cannot silently replace immutable match dictionaries/layouts.
    fs::write(server.directory.path().join("profile-pairs.txt"), "ÖL\n").unwrap();
    let path = server.directory.path().join("profile-catalog.json");
    let mut edited: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    edited["boardProfiles"][1]["boardSize"] = 9.into();
    edited["boardProfiles"][1]["premiums"] = serde_json::json!(vec!["normal"; 81]);
    fs::write(path, serde_json::to_vec(&edited).unwrap()).unwrap();
    // Recovery's canonical replay is verified with the same public factory boundary used at startup.
    let config = scrabble_server::config::ServerConfig::from_values(&server.values).unwrap();
    let factory = scrabble_server::factory::MatchFactory::new(&config).unwrap();
    for join in [&a.1, &b.1] {
        let id = game_server::MatchId::new(join["matchId"].as_str().unwrap()).unwrap();
        let image = game_server::RecoveryImage::read_file(
            &server
                .directory
                .path()
                .join("recovery")
                .join(format!("{}.recovery", id.as_str())),
        )
        .unwrap();
        game_server::verify_replay(factory.create(&id).unwrap(), &image.replay).unwrap();
    }
    server.start().await;
    for (join, phone, expected) in [
        (&a.1, &one.phones[0], &finished),
        (&b.1, &two.phones[0], &other),
    ] {
        let token = game_server::ReconnectToken(phone.1.reconnect_token).encode_hex();
        let (connection, _) = server
            .connect(
                &client,
                &format!("{}/reconnect/{token}", join["matchPath"].as_str().unwrap()),
            )
            .await;
        let restored = snapshot(&connection, |_| true).await;
        assert_eq!(restored.public.configuration, expected.public.configuration);
        assert_eq!(restored.public.dictionary, expected.public.dictionary);
        assert_eq!(restored.public.ruleset, expected.public.ruleset);
        assert_eq!(restored.public.phase, expected.public.phase);
        assert_eq!(restored.public.history, expected.public.history);
        assert_eq!(restored.public.board, expected.public.board);
        assert_eq!(restored.own_rack, expected.own_rack);
    }
    // Keep both board transports owned until the actual process lifecycle completes.
    drop((one.board, two.board));
    server.stop().await;
}

#[tokio::test]
async fn profiles_non_equivalent_catalog_defaults_are_declared_and_created_without_legacy_substitution()
 {
    let mut server = profile_server(true, false).await;
    let origin = "http://localhost:5173";
    let (code, catalog) = server.api("GET", "/profiles", None, origin).await;
    assert_eq!(code, 200);
    assert_eq!(
        catalog["defaults"]["legacyCompatible"], false,
        "a compact catalog default is not the deployment's original board"
    );
    let selected = profile_request(&catalog, 0x73);
    let join = server.api("POST", "/games", Some(&selected), origin).await;
    assert_eq!(join.0, 200);
    let client = server.client();
    let (connection, _) = server
        .connect(&client, join.1["matchPath"].as_str().unwrap())
        .await;
    let view = snapshot(&connection, |_| true).await;
    assert_eq!(view.public.configuration.board_size, 7);
    assert_ne!(
        view.public.configuration.identity,
        scrabble_game::ruleset::english_fixture().identity
    );
    server.stop().await;
}

#[tokio::test]
async fn profiles_full_identity_request_shape_fits_the_declared_bounded_http_envelope() {
    let mut server = profile_server(false, true).await;
    let origin = "http://localhost:5173";
    let (status, catalog) = server.api("GET", "/profiles", None, origin).await;
    assert_eq!(status, 200);
    let valid = profile_request(&catalog, 0x74);
    for field in ["wordList", "tile", "board"] {
        assert_eq!(
            valid["selection"][field]["name"].as_str().unwrap().len(),
            64
        );
    }
    let bytes = serde_json::to_vec(&valid).unwrap().len();
    assert!(
        bytes > 512 && bytes < 2048,
        "actual full identity request {bytes} must fit the bounded 2KiB contract"
    );
    assert_eq!(
        server.api("POST", "/games", Some(&valid), origin).await.0,
        200
    );
    let mut maximum_shape = profile_request(&catalog, 0x75);
    for field in ["wordList", "tile", "board"] {
        maximum_shape["selection"][field]["revision"] = "R".repeat(192).into();
    }
    let response = server
        .api("POST", "/games", Some(&maximum_shape), origin)
        .await;
    assert_eq!(
        response.0, 400,
        "bounded, syntactically valid identities must reach source validation instead of the obsolete 512-byte cap"
    );
    assert_eq!(response.1["error"], "unknown-profile");
    assert_eq!(hosted_profiles(&server).await, 1);
    server.stop().await;
}

// Independent review regressions at 702ba34, before persistence-cache repair.
async fn assert_live_profile_publication_is_durable(fault: &str, retry: bool) {
    use game_server::MatchId;
    use scrabble_server::{config::ServerConfig, factory::MatchFactory};
    let mut server = profile_server(false, false).await;
    let origin = "http://localhost:5173";
    let catalog = server.api("GET", "/profiles", None, origin).await.1;
    let initial_request = profile_request(&catalog, 0x81);
    let first = server
        .api("POST", "/games", Some(&initial_request), origin)
        .await;
    assert_eq!(first.0, 200);
    let directory = server.directory.path().join("configurations");
    let first_id = MatchId::new(first.1["matchId"].as_str().unwrap()).unwrap();
    let manifest = directory.join(format!("{first_id}.json"));
    let original_manifest = fs::read(&manifest).unwrap();
    let initialization: serde_json::Value = serde_json::from_slice(&original_manifest).unwrap();
    let words = directory.join("words").join(format!(
        "{}.txt",
        initialization["dictionary_hash"].as_str().unwrap()
    ));
    let original_words = fs::read(&words).unwrap();
    match fault {
        "missing words" => fs::remove_file(&words).unwrap(),
        "corrupt words" => fs::write(&words, b"ZZ\n").unwrap(),
        "missing manifest" => fs::remove_file(&manifest).unwrap(),
        "corrupt manifest" => fs::write(&manifest, b"{").unwrap(),
        _ => unreachable!(),
    }
    let request = if retry {
        initial_request.clone()
    } else {
        profile_request(&catalog, 0x82)
    };
    let result = server.api("POST", "/games", Some(&request), origin).await;
    if result.0 == 200 {
        let factory =
            MatchFactory::new(&ServerConfig::from_values(&server.values).unwrap()).unwrap();
        let returned_id = MatchId::new(result.1["matchId"].as_str().unwrap()).unwrap();
        assert!(
            factory.create(&first_id).is_ok(),
            "successful acknowledgment must retain durable original initialization: {fault}, retry={retry}"
        );
        assert!(
            factory.create(&returned_id).is_ok(),
            "successful acknowledgment must be reconstructible with a fresh factory: {fault}, retry={retry}"
        );
        assert_eq!(fs::read(&words).unwrap(), original_words);
        assert_eq!(fs::read(&manifest).unwrap(), original_manifest);
        assert_eq!(hosted_profiles(&server).await, if retry { 1 } else { 2 });
    } else {
        assert!((400..600).contains(&result.0));
        assert!(
            result.1["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty()),
            "useful typed rejection: {:?}",
            result
        );
        assert_eq!(
            hosted_profiles(&server).await,
            1,
            "failed creation must not allocate"
        );
        assert!(
            fs::read_dir(&directory)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json"))
                .count()
                <= 1,
            "failed creation must not publish another initialization"
        );
    }
    fs::write(&manifest, &original_manifest).unwrap();
    fs::write(&words, &original_words).unwrap();
    let restored = server.api("POST", "/games", Some(&request), origin).await;
    assert_eq!(
        restored.0, 200,
        "restored durable inputs permit the same request"
    );
    if result.0 == 200 {
        assert_eq!(restored.1, result.1);
    }
    let factory = MatchFactory::new(&ServerConfig::from_values(&server.values).unwrap()).unwrap();
    assert!(
        factory
            .create(&MatchId::new(restored.1["matchId"].as_str().unwrap()).unwrap())
            .is_ok()
    );
    server.stop().await;
}

#[tokio::test]
async fn profiles_new_creation_revalidates_missing_shared_words() {
    assert_live_profile_publication_is_durable("missing words", false).await;
}
#[tokio::test]
async fn profiles_new_creation_revalidates_corrupt_shared_words() {
    assert_live_profile_publication_is_durable("corrupt words", false).await;
}
#[tokio::test]
async fn profiles_retry_revalidates_missing_shared_words() {
    assert_live_profile_publication_is_durable("missing words", true).await;
}
#[tokio::test]
async fn profiles_retry_revalidates_corrupt_shared_words() {
    assert_live_profile_publication_is_durable("corrupt words", true).await;
}
#[tokio::test]
async fn profiles_retry_revalidates_missing_initialization() {
    assert_live_profile_publication_is_durable("missing manifest", true).await;
}
#[tokio::test]
async fn profiles_retry_revalidates_corrupt_initialization() {
    assert_live_profile_publication_is_durable("corrupt manifest", true).await;
}
