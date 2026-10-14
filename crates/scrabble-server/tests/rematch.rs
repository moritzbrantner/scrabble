//! Acceptance for #33 (docs/specs/rematch.md): one-tap rematch at the real `Games`/HTTP boundary.
//! Real HTTP API, real live runtime and real loopback WebTransport; no game-rule double.
use game_server::{
    LiveMatchHost, MatchHost, MatchHostWebTransportConfig, MatchId, RejectMatchControlService,
    ReplayRecord, SnapshotReassembler, WELCOME_BYTES, Welcome, decode_welcome, encode_command,
    serve_live_match_host_with_control_and_shutdown,
};
use scrabble_game::{
    identity::{PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Phase, PlayerSnapshot},
};
use scrabble_server::{
    config::ServerConfig,
    factory::{MatchFactory, game_id},
    game_api,
    games::{Games, JoinInformation, unix_seconds},
    simulation::{MATCH_LIFETIME_SECONDS, ScrabbleSimulation},
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
    task::JoinHandle,
    time::{sleep, timeout},
};
use wtransport::{ClientConfig, Connection, Endpoint, Identity, endpoint::endpoint_side::Client};

const ORIGIN: &str = "http://localhost:5173";

struct Stack {
    _directory: tempfile::TempDir,
    host: LiveMatchHost<ScrabbleSimulation>,
    games: Arc<Games>,
    factory: MatchFactory,
    client: Endpoint<Client>,
    port: u16,
    api_port: u16,
    shutdown: mpsc::Sender<()>,
    transport: JoinHandle<()>,
    api: JoinHandle<()>,
}
impl Drop for Stack {
    fn drop(&mut self) {
        self.transport.abort();
        self.api.abort();
    }
}
impl Stack {
    async fn new(max_matches: usize, drain_grace: Duration) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let identity = Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
        let cert = directory.path().join("cert.pem");
        let key = directory.path().join("key.pem");
        identity
            .certificate_chain()
            .store_pemfile(&cert)
            .await
            .unwrap();
        identity
            .private_key()
            .store_secret_pemfile(&key)
            .await
            .unwrap();
        let config = ServerConfig::from_values(&BTreeMap::from([
            ("SCRABBLE_MATCH_IDS".into(), String::new()),
            ("SCRABBLE_TEST_SEED".into(), "07".repeat(32)),
            (
                "SCRABBLE_SEED_FILE".into(),
                directory.path().join("seed").to_str().unwrap().into(),
            ),
            (
                "SCRABBLE_RECOVERY_DIR".into(),
                directory.path().join("recovery").to_str().unwrap().into(),
            ),
        ]))
        .unwrap();
        let factory = MatchFactory::new(&config).unwrap();
        let host = LiveMatchHost::new(MatchHost::new(max_matches).unwrap());
        let games = Arc::new(Games::new(
            host.clone(),
            factory.clone(),
            config.route_prefix.clone(),
            1200,
        ));
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        let (shutdown, receiver) = mpsc::channel(1);
        let serving = host.clone();
        let route_prefix = config.route_prefix.clone();
        let transport = tokio::spawn(async move {
            let _ = serve_live_match_host_with_control_and_shutdown(
                serving,
                RejectMatchControlService,
                MatchHostWebTransportConfig {
                    port,
                    certificate_pem: cert,
                    private_key_pem: key,
                    route_prefix,
                    allowed_origins: None,
                    drain_grace,
                },
                receiver,
            )
            .await;
        });
        timeout(Duration::from_secs(10), async {
            while !host.is_serving() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("live host must start serving");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_port = listener.local_addr().unwrap().port();
        let api_games = Arc::clone(&games);
        let api = tokio::spawn(async move {
            let _ = game_api::serve(listener, api_games, ORIGIN.into()).await;
        });
        let client = Endpoint::client(
            ClientConfig::builder()
                .with_bind_default()
                .with_server_certificate_hashes([identity.certificate_chain().as_slice()[0].hash()])
                .keep_alive_interval(Some(Duration::from_secs(1)))
                .build(),
        )
        .unwrap();
        Self {
            _directory: directory,
            host,
            games,
            factory,
            client,
            port,
            api_port,
            shutdown,
            transport,
            api,
        }
    }

    /// Raw HTTP so malformed bodies and content types reach the real request boundary.
    async fn http(
        &self,
        method: &str,
        path: &str,
        content_type: Option<&str>,
        body: &str,
    ) -> (u16, serde_json::Value) {
        timeout(Duration::from_secs(5), async {
            let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", self.api_port))
                .await
                .unwrap();
            let content_type = content_type
                .map(|value| format!("Content-Type: {value}\r\n"))
                .unwrap_or_default();
            let length = if method == "POST" {
                format!("Content-Length: {}\r\n", body.len())
            } else {
                String::new()
            };
            let request = format!(
                "{method} {path} HTTP/1.1\r\nHost: localhost\r\nOrigin: {ORIGIN}\r\n{content_type}{length}Connection: close\r\n\r\n{body}"
            );
            socket.write_all(request.as_bytes()).await.unwrap();
            let mut bytes = Vec::new();
            socket.take(16 * 1024).read_to_end(&mut bytes).await.unwrap();
            let text = String::from_utf8(bytes).unwrap();
            let (headers, body) = text.split_once("\r\n\r\n").unwrap();
            let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
            let body = if body.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_str(body).unwrap()
            };
            (status, body)
        })
        .await
        .expect("HTTP request must complete")
    }
    async fn post_json(&self, path: &str, body: &serde_json::Value) -> (u16, serde_json::Value) {
        self.http("POST", path, Some("application/json"), &body.to_string())
            .await
    }
    async fn get(&self, path: &str) -> (u16, serde_json::Value) {
        self.http("GET", path, None, "").await
    }
    async fn create_board_match(&self, nonce: &str, now: u64) -> JoinInformation {
        let (status, body) = self
            .post_json(
                "/games",
                &serde_json::json!({"version": 2, "requestId": nonce, "requestedAt": now}),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        serde_json::from_value(body).unwrap()
    }
    async fn connect(&self, route: &str) -> (Connection, Welcome) {
        let connection = timeout(
            Duration::from_secs(5),
            self.client
                .connect(format!("https://127.0.0.1:{}{route}", self.port)),
        )
        .await
        .unwrap()
        .unwrap();
        let mut stream = timeout(Duration::from_secs(5), connection.accept_uni())
            .await
            .unwrap()
            .unwrap();
        let mut bytes = [0; WELCOME_BYTES];
        stream.read_exact(&mut bytes).await.unwrap();
        (connection, decode_welcome(&bytes).unwrap())
    }
    async fn hosted(&self) -> usize {
        self.host.statuses().await.len()
    }
    /// Canonical evidence of a hosted match, without its ever-advancing tick.
    async fn evidence(&self, id: &MatchId) -> Evidence {
        self.host
            .inspect(id, |runtime| {
                let canonical: serde_json::Value =
                    serde_json::from_slice(&runtime.snapshot().unwrap().payload).unwrap();
                let records = runtime
                    .replay_log()
                    .expect("hosted matches capture replay")
                    .records()
                    .iter()
                    .filter(|record| {
                        !matches!(
                            record,
                            ReplayRecord::Checkpoint { .. } | ReplayRecord::DigestCheckpoint { .. }
                        )
                    })
                    .cloned()
                    .collect();
                Evidence { canonical, records }
            })
            .await
            .unwrap()
    }
}

struct Evidence {
    canonical: serde_json::Value,
    records: Vec<ReplayRecord>,
}

async fn snapshot(
    connection: &Connection,
    condition: impl Fn(&PlayerSnapshot) -> bool,
) -> PlayerSnapshot {
    timeout(Duration::from_secs(10), async {
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
    .expect("expected authoritative snapshot")
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

struct Phone {
    connection: Connection,
    welcome: Welcome,
    sequence: u32,
}
struct Played {
    join: JoinInformation,
    id: MatchId,
    board: (Connection, Welcome),
    phones: Vec<Phone>,
}

/// Board claims with its creation nonce, two phones take named seats, the board starts,
/// and the phones pass until the native scoreless-turn limit finishes the match.
async fn board_match(stack: &Stack, nonce: &str, now: u64, finish: bool) -> Played {
    let join = stack.create_board_match(nonce, now).await;
    let id = MatchId::new(join.match_id.clone()).unwrap();
    let board = stack.connect(&join.match_path).await;
    let lobby = snapshot(&board.0, |_| true).await;
    send(
        &board.0,
        &lobby,
        &board.1,
        1,
        0,
        Command::ClaimBoard {
            request_id: nonce.into(),
        },
    );
    snapshot(&board.0, |view| {
        view.public.host.as_ref().and_then(|host| host.id)
            == Some(PlayerId::new(u64::from(board.1.player_id)))
    })
    .await;
    let mut phones = Vec::new();
    for name in ["Ada", "Lin"] {
        let (connection, welcome) = stack.connect(&join.match_path).await;
        let view = snapshot(&connection, |_| true).await;
        send(
            &connection,
            &view,
            &welcome,
            1,
            0,
            Command::SetName {
                display_name: name.into(),
            },
        );
        phones.push(Phone {
            connection,
            welcome,
            sequence: 1,
        });
    }
    let named = snapshot(&board.0, |view| view.public.players.len() == 2).await;
    send(&board.0, &named, &board.1, 2, 0, Command::Start {});
    snapshot(&board.0, |view| {
        matches!(view.public.phase, Phase::Playing { .. })
    })
    .await;
    if finish {
        for turn in 0..6_u64 {
            let current = snapshot(&board.0, |view| {
                matches!(view.public.phase, Phase::Playing { turn: current, .. } if current == TurnId::new(turn))
            })
            .await;
            let Phase::Playing { active_player, .. } = current.public.phase else {
                unreachable!()
            };
            let phone = phones
                .iter_mut()
                .find(|phone| u64::from(phone.welcome.player_id) == active_player.get())
                .expect("active player is a seated phone");
            phone.sequence += 1;
            send(
                &phone.connection,
                &current,
                &phone.welcome,
                phone.sequence,
                turn,
                Command::Pass {},
            );
        }
        snapshot(&board.0, |view| {
            matches!(view.public.phase, Phase::Finished { .. })
        })
        .await;
    }
    Played {
        join,
        id,
        board,
        phones,
    }
}

fn rematch_body(board_request: &str, request: &str, at: u64) -> serde_json::Value {
    serde_json::json!({"version": 1, "boardRequestId": board_request, "requestId": request, "requestedAt": at})
}

#[tokio::test]
async fn rematch_request_validation_authority_and_phase_rows_are_enforced_in_order() {
    let stack = Stack::new(8, Duration::ZERO).await;
    let now = unix_seconds().unwrap();
    let board_nonce = "a1".repeat(16);
    let lobby = stack.create_board_match(&board_nonce, now).await;
    let lobby_path = format!("/games/{}/rematch", lobby.match_id);
    let fresh = "b2".repeat(16);
    let valid = rematch_body(&board_nonce, &fresh, now);

    // Before any rematch, the public lookup distinguishes an existing match without a link.
    assert_eq!(
        stack.get(&lobby_path).await,
        (404, serde_json::json!({"error": "no-rematch"}))
    );

    // Wrong content type, malformed bodies and unknown fields.
    assert_eq!(
        stack
            .http("POST", &lobby_path, Some("text/plain"), &valid.to_string())
            .await,
        (415, serde_json::json!({"error": "json-required"}))
    );
    let mut unknown = valid.clone();
    unknown["displayName"] = serde_json::json!("Ada");
    let mut bad_nonce = valid.clone();
    bad_nonce["requestId"] = serde_json::json!("not-a-nonce");
    let mut bad_board = valid.clone();
    bad_board["boardRequestId"] = serde_json::json!("AB".repeat(16));
    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("boardRequestId");
    let mut future = valid.clone();
    future["requestedAt"] = serde_json::json!(now + 60);
    for body in [
        "{".to_owned(),
        unknown.to_string(),
        bad_nonce.to_string(),
        bad_board.to_string(),
        missing.to_string(),
        future.to_string(),
    ] {
        assert_eq!(
            stack
                .http("POST", &lobby_path, Some("application/json"), &body)
                .await,
            (400, serde_json::json!({"error": "invalid-request"})),
            "{body}"
        );
    }

    // A new request outside the 120-second creation window has expired.
    assert_eq!(
        stack
            .post_json(&lobby_path, &rematch_body(&board_nonce, &fresh, now - 120))
            .await,
        (410, serde_json::json!({"error": "expired-request"}))
    );

    // A well-formed but never-created board match is unknown.
    let never = stack
        .factory
        .board_creation_id(&"c3".repeat(16), now)
        .unwrap();
    assert_eq!(
        stack
            .post_json(
                &format!("/games/{}/rematch", never.as_str()),
                &rematch_body(&"c3".repeat(16), &fresh, now)
            )
            .await,
        (404, serde_json::json!({"error": "unknown-match"}))
    );

    // Board authority is checked before phase: a wrong nonce, or a legacy g_ match, is rejected.
    assert_eq!(
        stack
            .post_json(&lobby_path, &rematch_body(&"d4".repeat(16), &fresh, now))
            .await,
        (403, serde_json::json!({"error": "board-rejected"}))
    );
    let legacy_nonce = "e5".repeat(16);
    let (status, legacy) = stack
        .post_json(
            "/games",
            &serde_json::json!({"version": 1, "requestId": legacy_nonce, "requestedAt": now}),
        )
        .await;
    assert_eq!(status, 200);
    assert!(legacy["matchId"].as_str().unwrap().starts_with("g_"));
    assert_eq!(
        stack
            .post_json(
                &format!("/games/{}/rematch", legacy["matchId"].as_str().unwrap()),
                &rematch_body(&legacy_nonce, &fresh, now)
            )
            .await,
        (403, serde_json::json!({"error": "board-rejected"}))
    );

    // The real board of an unfinished match cannot start a rematch, in lobby or while playing.
    let hosted = stack.hosted().await;
    assert_eq!(
        stack.post_json(&lobby_path, &valid).await,
        (409, serde_json::json!({"error": "not-finished"}))
    );
    let playing_nonce = "f6".repeat(16);
    let playing = board_match(&stack, &playing_nonce, now, false).await;
    let playing_path = format!("/games/{}/rematch", playing.join.match_id);
    let before = stack.evidence(&playing.id).await;
    assert_eq!(
        stack
            .post_json(&playing_path, &rematch_body(&playing_nonce, &fresh, now))
            .await,
        (409, serde_json::json!({"error": "not-finished"}))
    );
    assert_eq!(
        stack.hosted().await,
        hosted + 1,
        "rejections allocate nothing"
    );
    let after = stack.evidence(&playing.id).await;
    assert_eq!(after.canonical["game"], before.canonical["game"]);
    assert!(after.records == before.records);
    for path in [&lobby_path, &playing_path] {
        assert_eq!(
            stack.get(path).await,
            (404, serde_json::json!({"error": "no-rematch"}))
        );
    }
    assert_eq!(
        stack
            .get(&format!("/games/{}/rematch", never.as_str()))
            .await,
        (404, serde_json::json!({"error": "unknown-match"}))
    );
}

#[tokio::test]
async fn board_rematch_is_one_fresh_lobby_linked_once_and_leaves_the_finished_match_immutable() {
    let stack = Stack::new(8, Duration::ZERO).await;
    let now = unix_seconds().unwrap();
    let board_nonce = "11".repeat(16);
    let finished = board_match(&stack, &board_nonce, now, true).await;
    let path = format!("/games/{}/rematch", finished.join.match_id);
    assert_eq!(
        stack.get(&path).await,
        (404, serde_json::json!({"error": "no-rematch"})),
        "a finished match without a rematch reports no link"
    );
    let finished_view = snapshot(&finished.phones[0].connection, |view| {
        matches!(view.public.phase, Phase::Finished { .. })
    })
    .await;
    let before = stack.evidence(&finished.id).await;
    let hosted = stack.hosted().await;

    // Concurrent and repeated retries of one intent yield one match and one link.
    let request = "22".repeat(16);
    let body = rematch_body(&board_nonce, &request, now);
    let (first, concurrent) =
        tokio::join!(stack.post_json(&path, &body), stack.post_json(&path, &body));
    assert_eq!(first.0, 200, "{}", first.1);
    assert_eq!(concurrent, first);
    assert_eq!(
        first.1.as_object().unwrap().len(),
        5,
        "only the five public join fields are returned"
    );
    let rematch: JoinInformation = serde_json::from_value(first.1.clone()).unwrap();
    let expected_id = stack.factory.board_creation_id(&request, now).unwrap();
    assert_eq!(rematch.match_id, expected_id.as_str());
    assert!(rematch.match_id.starts_with("b_"));
    assert_ne!(rematch.match_id, finished.join.match_id);
    assert_eq!(rematch.game_id, game_id(&expected_id));
    assert_ne!(rematch.game_id, finished.join.game_id);
    assert_eq!(rematch.expires_at, now + MATCH_LIFETIME_SECONDS);
    assert_eq!(stack.hosted().await, hosted + 1);
    assert_eq!(stack.post_json(&path, &body).await, first);
    assert_eq!(stack.hosted().await, hosted + 1);

    // Another intent cannot replace the link; board authority is still checked first.
    let other = rematch_body(&board_nonce, &"33".repeat(16), now);
    assert_eq!(
        stack.post_json(&path, &other).await,
        (409, serde_json::json!({"error": "rematch-exists"}))
    );
    assert_eq!(
        stack
            .post_json(&path, &rematch_body(&"44".repeat(16), &request, now))
            .await,
        (403, serde_json::json!({"error": "board-rejected"}))
    );
    assert_eq!(stack.hosted().await, hosted + 1);

    // Phones of the finished match discover the same public join information.
    assert_eq!(stack.get(&path).await, first);
    assert_eq!(stack.games.rematch_link_count().await, 1, "one link");
    assert_eq!(
        stack.get(&format!("/games/{}", rematch.match_id)).await,
        first
    );
    assert_eq!(
        stack
            .get(&format!("/games/{}/join", rematch.match_id))
            .await
            .0,
        200,
        "the rematch lobby admits new players"
    );

    // The rematch is a fresh lobby dealt from its own identity, not the finished match's.
    let rematch_id = MatchId::new(rematch.match_id.clone()).unwrap();
    let fresh = stack.evidence(&rematch_id).await;
    let pristine: serde_json::Value = serde_json::from_slice(
        &game_server::GameSimulation::snapshot(&stack.factory.create(&expected_id).unwrap())
            .unwrap()
            .payload,
    )
    .unwrap();
    let finished_initial: serde_json::Value = serde_json::from_slice(
        &game_server::GameSimulation::snapshot(&stack.factory.create(&finished.id).unwrap())
            .unwrap()
            .payload,
    )
    .unwrap();
    assert_eq!(fresh.canonical["game"], pristine["game"]);
    assert_eq!(
        fresh.canonical["game"]["phase"],
        serde_json::json!({"kind": "lobby"})
    );
    assert_eq!(fresh.canonical["game"]["players"], serde_json::json!([]));
    assert_ne!(
        fresh.canonical["game"]["bag"], finished_initial["game"]["bag"],
        "the rematch has its own dealing seed"
    );
    assert!(
        fresh
            .records
            .iter()
            .all(|record| !matches!(record, ReplayRecord::CommandApplied { .. })),
        "no command was submitted to the rematch"
    );

    // Its board authority is the rematch request nonce, so that board can start it later.
    let (rematch_board, rematch_board_welcome) = stack.connect(&rematch.match_path).await;
    let lobby = snapshot(&rematch_board, |_| true).await;
    assert!(matches!(lobby.public.phase, Phase::Lobby {}));
    assert!(lobby.public.players.is_empty());
    send(
        &rematch_board,
        &lobby,
        &rematch_board_welcome,
        1,
        0,
        Command::ClaimBoard {
            request_id: request.clone(),
        },
    );
    snapshot(&rematch_board, |view| {
        view.public.host.as_ref().and_then(|host| host.id)
            == Some(PlayerId::new(u64::from(rematch_board_welcome.player_id)))
    })
    .await;

    // Starting the rematch never touched the finished match's state, replay or projection.
    let after = stack.evidence(&finished.id).await;
    for field in ["game", "lifecycle", "board", "admitted", "version"] {
        assert_eq!(after.canonical[field], before.canonical[field], "{field}");
    }
    assert!(
        after.records == before.records,
        "replay evidence of the finished match is unchanged"
    );
    // This connection's earlier datagrams were never drained; skip buffered pre-finish projections.
    let still = snapshot(&finished.phones[1].connection, |view| {
        view.public.revision.get() >= finished_view.public.revision.get()
    })
    .await;
    assert_eq!(still.public.phase, finished_view.public.phase);
    assert_eq!(still.public.revision, finished_view.public.revision);
    assert_eq!(still.public.history, finished_view.public.history);
    assert_eq!(
        still
            .public
            .players
            .iter()
            .map(|player| (player.id, player.score))
            .collect::<Vec<_>>(),
        finished_view
            .public
            .players
            .iter()
            .map(|player| (player.id, player.score))
            .collect::<Vec<_>>()
    );

    // The finished match stays read-only: closed to joins and to gameplay.
    assert_eq!(
        stack
            .get(&format!("/games/{}/join", finished.join.match_id))
            .await,
        (409, serde_json::json!({"error": "join-closed"}))
    );
    let phone = &finished.phones[0];
    send(
        &phone.connection,
        &finished_view,
        &phone.welcome,
        phone.sequence + 1,
        finished_view.public.revision.get(),
        Command::Pass {},
    );
    sleep(Duration::from_millis(300)).await;
    let rejected = stack.evidence(&finished.id).await;
    assert_eq!(rejected.canonical["game"], before.canonical["game"]);
    assert!(rejected.records == before.records);

    // Retiring the finished match removes the link; the rematch itself is unaffected.
    stack
        .games
        .retire(&finished.id, finished.join.expires_at)
        .await
        .unwrap();
    assert_eq!(
        stack.games.rematch_link_count().await,
        0,
        "retirement removes the link"
    );
    assert_eq!(
        stack.get(&path).await,
        (404, serde_json::json!({"error": "unknown-match"}))
    );
    assert_eq!(
        stack.post_json(&path, &body).await,
        (404, serde_json::json!({"error": "unknown-match"})),
        "a retired match cannot replay its rematch link"
    );
    assert_eq!(
        stack.get(&format!("/games/{}", rematch.match_id)).await,
        first
    );
    drop(finished.board);
}

#[tokio::test]
async fn rematch_respects_capacity_and_drain_without_recording_a_link() {
    let stack = Stack::new(2, Duration::from_secs(3)).await;
    let now = unix_seconds().unwrap();
    let board_nonce = "55".repeat(16);
    let finished = board_match(&stack, &board_nonce, now, true).await;
    let path = format!("/games/{}/rematch", finished.join.match_id);
    let other = stack.create_board_match(&"66".repeat(16), now).await;
    assert_eq!(stack.hosted().await, 2);
    let request = "77".repeat(16);
    let body = rematch_body(&board_nonce, &request, now);
    assert_eq!(
        stack.post_json(&path, &body).await,
        (429, serde_json::json!({"error": "at-capacity"}))
    );
    assert_eq!(
        stack.get(&path).await,
        (404, serde_json::json!({"error": "no-rematch"})),
        "a refused rematch records no link"
    );
    assert_eq!(stack.games.rematch_link_count().await, 0);
    assert_eq!(stack.hosted().await, 2);

    // Freeing capacity lets the same intent succeed.
    let other_id = MatchId::new(other.match_id).unwrap();
    stack
        .games
        .retire(&other_id, other.expires_at)
        .await
        .unwrap();
    let (status, created) = stack.post_json(&path, &body).await;
    assert_eq!(status, 200, "{created}");
    assert_eq!(stack.get(&path).await, (200, created.clone()));
    assert_eq!(stack.games.rematch_link_count().await, 1);

    // A draining service refuses even an idempotent retry, like ordinary creation.
    stack.shutdown.send(()).await.unwrap();
    timeout(Duration::from_secs(2), async {
        while !stack.host.is_draining().await && stack.host.is_serving() {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let (status, error) = stack.post_json(&path, &body).await;
    assert_eq!(status, 503);
    assert!(
        error == serde_json::json!({"error": "draining"})
            || error == serde_json::json!({"error": "not-serving"}),
        "{error}"
    );
    drop(finished.board);
}

#[tokio::test]
async fn a_rematch_intent_reusing_the_finished_match_creation_request_is_rejected() {
    let stack = Stack::new(8, Duration::ZERO).await;
    let now = unix_seconds().unwrap();
    let board_nonce = "88".repeat(16);
    let finished = board_match(&stack, &board_nonce, now, true).await;
    let path = format!("/games/{}/rematch", finished.join.match_id);
    let before = stack.evidence(&finished.id).await;
    let hosted = stack.hosted().await;
    // Within the creation window, the finished match's own intent derives its own ID.
    assert_eq!(
        stack
            .factory
            .board_creation_id(&board_nonce, now)
            .unwrap()
            .as_str(),
        finished.join.match_id
    );
    assert_eq!(
        stack
            .post_json(&path, &rematch_body(&board_nonce, &board_nonce, now))
            .await,
        (400, serde_json::json!({"error": "invalid-request"})),
        "a match cannot become its own rematch"
    );
    assert_eq!(stack.hosted().await, hosted);
    assert_eq!(
        stack.get(&path).await,
        (404, serde_json::json!({"error": "no-rematch"}))
    );
    let after = stack.evidence(&finished.id).await;
    assert_eq!(after.canonical["game"], before.canonical["game"]);
    assert!(after.records == before.records);
    // A genuine new intent still succeeds afterwards.
    let (status, created) = stack
        .post_json(&path, &rematch_body(&board_nonce, &"99".repeat(16), now))
        .await;
    assert_eq!(status, 200, "{created}");
    assert_ne!(
        created["matchId"],
        serde_json::json!(finished.join.match_id)
    );
    drop(finished.board);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retirement_racing_a_rematch_never_leaves_a_link_for_a_retired_match() {
    let stack = Stack::new(8, Duration::ZERO).await;
    let now = unix_seconds().unwrap();

    // Deterministic order: once retired, a rematch is refused and nothing is recorded.
    let retired = board_match(&stack, &"a0".repeat(16), now, true).await;
    let path = format!("/games/{}/rematch", retired.join.match_id);
    stack
        .games
        .retire(&retired.id, retired.join.expires_at)
        .await
        .unwrap();
    let hosted = stack.hosted().await;
    let body = rematch_body(&"a0".repeat(16), &"a1".repeat(16), now);
    assert_eq!(
        stack.post_json(&path, &body).await,
        (404, serde_json::json!({"error": "unknown-match"}))
    );
    assert_eq!(stack.hosted().await, hosted, "no rematch was created");
    assert_eq!(
        stack.get(&path).await,
        (404, serde_json::json!({"error": "unknown-match"}))
    );
    drop(retired.board);

    assert_eq!(stack.games.rematch_link_count().await, 0);

    // Concurrent order: both operations are released together, then one is delayed by a
    // varying offset so the rounds sweep across the window where creation and retirement meet.
    // Whichever wins, the registry keeps no link for a retired match, which HTTP cannot show.
    let offsets_ms: [i64; 12] = [0, 0, 0, 0, 1, -1, 2, -2, 5, -5, 50, -50];
    let (mut linked, mut refused) = (0, 0);
    for (round, offset) in offsets_ms.into_iter().enumerate() {
        let nonce = format!("{:02x}", 0xb0 + round).repeat(16);
        let request = format!("{:02x}", 0xd0 + round).repeat(16);
        let finished = board_match(&stack, &nonce, now, true).await;
        let path = format!("/games/{}/rematch", finished.join.match_id);
        let body = rematch_body(&nonce, &request, now);
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let games = Arc::clone(&stack.games);
        let (id, expires_at) = (finished.id.clone(), finished.join.expires_at);
        let release = Arc::clone(&barrier);
        let retirement = tokio::spawn(async move {
            release.wait().await;
            if offset > 0 {
                sleep(Duration::from_millis(offset.unsigned_abs())).await;
            }
            games.retire(&id, expires_at).await
        });
        barrier.wait().await;
        if offset < 0 {
            sleep(Duration::from_millis(offset.unsigned_abs())).await;
        }
        let created = stack.post_json(&path, &body).await;
        retirement
            .await
            .unwrap()
            .expect("an expired finished match is always retirable");
        match created.0 {
            200 => linked += 1,
            _ => {
                assert_eq!(
                    created,
                    (404, serde_json::json!({"error": "unknown-match"})),
                    "round {round}"
                );
                refused += 1;
            }
        }
        assert_eq!(
            stack.games.rematch_link_count().await,
            0,
            "round {round} (offset {offset} ms, POST {}): retirement left a registry link",
            created.0
        );
        assert!(
            stack
                .host
                .statuses()
                .await
                .iter()
                .all(|status| status.id != finished.id),
            "the finished match is gone"
        );
        assert_eq!(
            stack.get(&path).await,
            (404, serde_json::json!({"error": "unknown-match"})),
            "round {round}: no link survives retirement"
        );
        assert_eq!(
            stack.post_json(&path, &body).await,
            (404, serde_json::json!({"error": "unknown-match"})),
            "round {round}: a retry cannot revive the link"
        );
        assert_eq!(stack.games.rematch_link_count().await, 0);
        drop(finished.board);
    }
    // The extreme offsets guarantee both orders were exercised, not only one side of the race.
    assert!(
        linked > 0 && refused > 0,
        "linked {linked}, refused {refused}"
    );
}
