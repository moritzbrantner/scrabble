//! Persisted initialization corruption at the real executable/replay boundary (#94).
#![cfg(unix)]
use game_server::{
    MatchId, RecoveryImage, SnapshotReassembler, Welcome, decode_welcome, verify_replay,
};
use scrabble_game::protocol::PlayerSnapshot;
use scrabble_server::{config::ServerConfig, factory::MatchFactory};
use std::{
    collections::BTreeMap,
    fs,
    net::{TcpListener, UdpSocket},
    path::Path,
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    time::{sleep, timeout},
};
use wtransport::{ClientConfig, Connection, Endpoint, Identity};
#[path = "support/profile_catalog.rs"]
mod profile_catalog;
fn spawn(values: &BTreeMap<String, String>, log: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_scrabble-server"))
        .env_clear()
        .envs(values)
        .stdout(Stdio::null())
        .stderr(fs::File::create(log).unwrap())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}
async fn ready(child: &mut Child, port: u16) {
    timeout(Duration::from_secs(5), async {
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "native server terminated before readiness"
            );
            if let Ok(mut socket) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
                socket
                    .write_all(
                        b"GET /readyz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                    )
                    .await
                    .unwrap();
                let mut bytes = Vec::new();
                socket.read_to_end(&mut bytes).await.unwrap();
                if String::from_utf8_lossy(&bytes).starts_with("HTTP/1.1 200") {
                    return;
                }
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}
async fn stop(child: &mut Child) {
    assert!(
        Command::new("kill")
            .arg("-TERM")
            .arg(child.id().unwrap().to_string())
            .status()
            .await
            .unwrap()
            .success()
    );
    assert!(
        timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}
async fn api(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> serde_json::Value {
    let (status, body) = api_response(port, method, path, body).await;
    assert_eq!(status, 200, "{body}");
    body
}
async fn api_response(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> (u16, serde_json::Value) {
    timeout(Duration::from_secs(5), async {
        let body = body.map(serde_json::Value::to_string).unwrap_or_default();
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nOrigin: http://localhost:5173\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        stream.take(128 * 1024).read_to_end(&mut bytes).await.unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let (headers, body) = text.split_once("\r\n\r\n").unwrap();
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, serde_json::from_str(body).unwrap())
    })
    .await
    .unwrap()
}
async fn connect(
    client: &Endpoint<wtransport::endpoint::endpoint_side::Client>,
    port: u16,
    path: &str,
) -> (Connection, Welcome, PlayerSnapshot) {
    timeout(Duration::from_secs(5), async {
        let connection = client
            .connect(format!("https://127.0.0.1:{port}{path}"))
            .await
            .unwrap();
        let mut stream = connection.accept_uni().await.unwrap();
        let mut bytes = [0; 46];
        stream.read_exact(&mut bytes).await.unwrap();
        let welcome = decode_welcome(&bytes).unwrap();
        let mut reassembler = SnapshotReassembler::new();
        loop {
            let datagram = connection.receive_datagram().await.unwrap();
            if let Some(frame) = reassembler.accept(&datagram).unwrap() {
                let snapshot = serde_json::from_slice(&frame.payload).unwrap();
                return (connection, welcome, snapshot);
            }
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn configured_initialization_corruption_never_falls_back_and_exact_restoration_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let identity = Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
    identity
        .certificate_chain()
        .store_pemfile(root.join("cert.pem"))
        .await
        .unwrap();
    identity
        .private_key()
        .store_secret_pemfile(root.join("key.pem"))
        .await
        .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(root.join("key.pem"), fs::Permissions::from_mode(0o600)).unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = udp.local_addr().unwrap().port();
    let status = TcpListener::bind("127.0.0.1:0").unwrap();
    let status_port = status.local_addr().unwrap().port();
    let api_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let api_port = api_listener.local_addr().unwrap().port();
    drop((udp, status, api_listener));
    let catalog = profile_catalog::write_catalog(root, profile_catalog::LEGACY_WORDS).unwrap();
    let values = BTreeMap::from([
        ("SCRABBLE_PORT".into(), port.to_string()),
        ("SCRABBLE_API_PORT".into(), api_port.to_string()),
        ("SCRABBLE_STATUS_PORT".into(), status_port.to_string()),
        ("SCRABBLE_MATCH_IDS".into(), String::new()),
        (
            "SCRABBLE_CERT_PEM".into(),
            root.join("cert.pem").display().to_string(),
        ),
        (
            "SCRABBLE_KEY_PEM".into(),
            root.join("key.pem").display().to_string(),
        ),
        (
            "SCRABBLE_SEED_FILE".into(),
            root.join("seed").display().to_string(),
        ),
        (
            "SCRABBLE_RECOVERY_DIR".into(),
            root.join("recovery").display().to_string(),
        ),
        (
            "SCRABBLE_PROFILE_CATALOG_FILE".into(),
            catalog.display().to_string(),
        ),
        (
            "SCRABBLE_MATCH_CONFIGURATION_DIR".into(),
            root.join("configurations").display().to_string(),
        ),
        ("SCRABBLE_DRAIN_GRACE_MS".into(), "100".into()),
    ]);
    let log = root.join("native.log");
    let mut child = spawn(&values, &log);
    ready(&mut child, status_port).await;
    let catalog = api(api_port, "GET", "/profiles", None).await;
    let now = scrabble_server::games::unix_seconds().unwrap();
    let request = serde_json::json!({"version":3,"requestId":"97".repeat(16),"requestedAt":now,"selection":catalog["defaults"]["selection"],"rules":catalog["defaults"]["rules"]});
    let configured = api(api_port, "POST", "/games", Some(&request)).await;
    let legacy = api(
        api_port,
        "POST",
        "/games",
        Some(&serde_json::json!({"version":2,"requestId":"98".repeat(16),"requestedAt":now})),
    )
    .await;
    let mut different = request.clone();
    different["requestId"] = "99".repeat(16).into();
    different["rules"]["bingo_bonus"] = 17.into();
    let different_join = api(api_port, "POST", "/games", Some(&different)).await;
    let foreign_manifest = fs::read(root.join("configurations").join(format!(
        "{}.json",
        different_join["matchId"].as_str().unwrap()
    )))
    .unwrap();
    let client = Endpoint::client(
        ClientConfig::builder()
            .with_bind_default()
            .with_server_certificate_hashes([identity.certificate_chain().as_slice()[0].hash()])
            .build(),
    )
    .unwrap();
    let (original, credential, before) =
        connect(&client, port, configured["matchPath"].as_str().unwrap()).await;
    let live_manifest = root
        .join("configurations")
        .join(format!("{}.json", configured["matchId"].as_str().unwrap()));
    let original_initialization = fs::read(&live_manifest).unwrap();
    fs::remove_file(&live_manifest).unwrap();
    let mut changed_retry = request.clone();
    changed_retry["rules"]["bingo_bonus"] = 17.into();
    let (status, rejected) = api_response(api_port, "POST", "/games", Some(&changed_retry)).await;
    assert!(
        (400..600).contains(&status),
        "changed retry must fail closed"
    );
    assert!(
        rejected["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
    assert!(
        !live_manifest.exists(),
        "missing initialization cannot be replaced with new rules"
    );
    assert_eq!(
        api(status_port, "GET", "/status", None).await["capacity"]["hostedMatches"],
        3,
        "a failed changed retry cannot allocate another match"
    );
    let (other, _, still_live) =
        connect(&client, port, configured["matchPath"].as_str().unwrap()).await;
    assert_eq!(still_live.public.configuration, before.public.configuration);
    fs::write(&live_manifest, &original_initialization).unwrap();
    assert_eq!(
        api(api_port, "POST", "/games", Some(&request)).await,
        configured
    );
    drop(other);
    stop(&mut child).await;
    drop(original);
    let id = MatchId::new(configured["matchId"].as_str().unwrap()).unwrap();
    let legacy_id = MatchId::new(legacy["matchId"].as_str().unwrap()).unwrap();
    let manifest = root.join("configurations").join(format!("{id}.json"));
    let manifest_bytes = fs::read(&manifest).unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
    let words = root.join("configurations/words").join(format!(
        "{}.txt",
        parsed["dictionary_hash"].as_str().unwrap()
    ));
    let word_bytes = fs::read(&words).unwrap();
    let image =
        RecoveryImage::read_file(&root.join("recovery").join(format!("{id}.recovery"))).unwrap();
    for scenario in 0..6 {
        match scenario {
            0 => fs::remove_file(&manifest).unwrap(),
            1 => fs::write(&manifest, b"{").unwrap(),
            2 => {
                let mut changed = parsed.clone();
                changed["rules"]["bingo_bonus"] = 31.into();
                changed["rules"]["identity"]["revision"] = "syntactically-valid-but-changed".into();
                fs::write(&manifest, serde_json::to_vec(&changed).unwrap()).unwrap();
            }
            3 => fs::remove_file(&words).unwrap(),
            4 => fs::write(&words, b"ZZ\n").unwrap(),
            5 => fs::write(&manifest, &foreign_manifest).unwrap(),
            _ => unreachable!(),
        }
        let factory = MatchFactory::new(&ServerConfig::from_values(&values).unwrap()).unwrap();
        assert!(
            factory.create(&id).is_err(),
            "corruption scenario {scenario} must not reconstruct defaults"
        );
        assert!(
            factory.create(&legacy_id).is_ok(),
            "legacy v2 needs no configured manifest"
        );
        let mut failed = spawn(&values, &log);
        let status = timeout(Duration::from_secs(5), failed.wait())
            .await
            .expect("corrupt initialization must refuse startup")
            .unwrap();
        assert!(!status.success(), "scenario {scenario}");
        fs::write(&manifest, &manifest_bytes).unwrap();
        fs::write(&words, &word_bytes).unwrap();
        let restored = MatchFactory::new(&ServerConfig::from_values(&values).unwrap()).unwrap();
        verify_replay(restored.create(&id).unwrap(), &image.replay).unwrap();
    }
    // A reused factory must validate durable inputs even when it already holds the
    // composed match or another match sharing the same parsed dictionary.
    let reused = MatchFactory::new(&ServerConfig::from_values(&values).unwrap()).unwrap();
    let other_id = MatchId::new(different_join["matchId"].as_str().unwrap()).unwrap();
    assert!(reused.create(&id).is_ok());
    fs::remove_file(&words).unwrap();
    assert!(
        reused.create(&id).is_err(),
        "cached match must revalidate words"
    );
    assert!(
        reused.create(&other_id).is_err(),
        "shared parsed words must revalidate storage"
    );
    fs::write(&words, &word_bytes).unwrap();
    assert!(reused.create(&other_id).is_ok());
    fs::remove_file(&manifest).unwrap();
    assert!(
        reused.create(&id).is_err(),
        "cached match cannot be reinitialized after its manifest disappears"
    );
    fs::write(&manifest, b"{").unwrap();
    assert!(
        reused.create(&id).is_err(),
        "cached match must revalidate its manifest"
    );
    fs::write(&manifest, &manifest_bytes).unwrap();
    assert!(reused.create(&id).is_ok());
    let mut restored = spawn(&values, &log);
    ready(&mut restored, status_port).await;
    let token = game_server::ReconnectToken(credential.reconnect_token).encode_hex();
    let (_connection, _, after) = connect(
        &client,
        port,
        &format!(
            "{}/reconnect/{token}",
            configured["matchPath"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(after.public.configuration, before.public.configuration);
    assert_eq!(after.own_rack, before.own_rack);
    assert_eq!(after.public.game_id, before.public.game_id);
    stop(&mut restored).await;
}
