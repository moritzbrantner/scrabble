//! Guest-owned private tile profiles (#100; Decisions 1.21, 1.25, 1.27, 1.29, 1.45).
//!
//! HTTP endpoints on the game API origin (JSON, `version: 1`, every call a POST):
//! - `POST /tile-profiles` `{version,name,tiles}` -> `{version,profileId,ownerCapability,
//!   name,revision:1,identity,tiles}`; `profileId` = `t_<32 hex>`.
//! - `POST /tile-profiles/{id}/read|delete` `{version,ownerCapability}`.
//! - `POST /tile-profiles/{id}/update` `{version,ownerCapability,name,tiles}` -> a new
//!   revision with a NEW immutable `identity`.
//! - `POST /games` (version 3): `selection.tile` may be the `identity` of any retained
//!   saved version, chosen independently of the word list and board (Decision 1.25).
//!
//! Errors: 400 `invalid-tile-profile` with a `reason`; 403 `tile-profile-owner-rejected`;
//! 404 `unknown-tile-profile`. There are no share links (Decision 1.27).
use game_server::GameSimulation;
use scrabble_game::{
    identity::{PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, PlayerSnapshot},
};
use scrabble_server::{
    config::ServerConfig,
    factory::{self, MatchFactory},
    games::unix_seconds,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[path = "support/hosted.rs"]
mod hosted;
#[path = "support/profile_catalog.rs"]
mod profile_catalog;

fn values(root: &Path) -> BTreeMap<String, String> {
    let catalog = root.join("profile-catalog.json");
    if !catalog.exists() {
        profile_catalog::write_catalog(root, profile_catalog::LEGACY_WORDS).unwrap();
    }
    let path = |name: &str| root.join(name).to_str().unwrap().to_owned();
    let certificate = root.join("cert.pem");
    let key = root.join("key.pem");
    if !certificate.exists() {
        let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
        std::fs::write(
            &certificate,
            identity.certificate_chain().as_slice()[0].to_pem(),
        )
        .unwrap();
        std::fs::write(&key, identity.private_key().to_secret_pem()).unwrap();
    }
    BTreeMap::from([
        ("SCRABBLE_MATCH_IDS".into(), String::new()),
        ("SCRABBLE_TEST_SEED".into(), "0c".repeat(32)),
        (
            "SCRABBLE_PROFILE_CATALOG_FILE".into(),
            catalog.to_str().unwrap().into(),
        ),
        ("SCRABBLE_SEED_FILE".into(), path("seed")),
        ("SCRABBLE_RECOVERY_DIR".into(), path("recovery")),
        (
            "SCRABBLE_MATCH_CONFIGURATION_DIR".into(),
            path("configurations"),
        ),
        ("SCRABBLE_BOARD_LAYOUT_DIR".into(), path("layouts")),
        ("SCRABBLE_WORD_LIST_DIR".into(), path("word-lists")),
        ("SCRABBLE_TILE_PROFILE_DIR".into(), path("tile-profiles")),
        ("SCRABBLE_MAX_MATCHES".into(), "8".into()),
        (
            "SCRABBLE_CERT_PEM".into(),
            certificate.to_str().unwrap().into(),
        ),
        ("SCRABBLE_KEY_PEM".into(), key.to_str().unwrap().into()),
    ])
}

struct Api {
    port: u16,
    task: Option<hosted::ServeTask>,
    shutdown: tokio::sync::mpsc::Sender<()>,
}
impl Drop for Api {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
impl Api {
    async fn start(root: &Path) -> Self {
        let hosted = hosted::start(&values(root)).await;
        Self {
            port: hosted.api_port,
            task: Some(hosted.task),
            shutdown: hosted.shutdown,
        }
    }
    async fn stop(mut self) {
        let _ = self.shutdown.send(()).await;
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.task.take().unwrap(),
        )
        .await
        .expect("graceful shutdown")
        .unwrap()
        .expect("server shut down cleanly");
    }
    async fn raw(&self, method: &str, path: &str, body: &str) -> (u16, Value) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).await.unwrap();
        let _ = stream
            .write_all(
                format!(
                    "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await;
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes).await;
        let response = String::from_utf8(bytes).unwrap();
        let (header, body) = response.split_once("\r\n\r\n").unwrap();
        let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = if body.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(body).unwrap_or_else(|_| json!({ "unparsed": body }))
        };
        (status, body)
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.raw("POST", path, &body.to_string()).await
    }
    async fn save(&self, name: &str, tiles: Value) -> Saved {
        let (status, body) = self
            .post(
                "/tile-profiles",
                json!({"version":1,"name":name,"tiles":tiles}),
            )
            .await;
        assert_eq!(status, 200, "a valid tile profile must save: {body}");
        Saved {
            id: body["profileId"].as_str().unwrap().to_owned(),
            capability: body["ownerCapability"].as_str().unwrap().to_owned(),
            identity: body["identity"].clone(),
        }
    }
    async fn owner(&self, saved: &Saved, action: &str, capability: &str) -> (u16, Value) {
        self.post(
            &format!("/tile-profiles/{}/{action}", saved.id),
            json!({"version":1,"ownerCapability":capability}),
        )
        .await
    }
    async fn update(&self, saved: &Saved, name: &str, tiles: Value) -> (u16, Value) {
        self.post(
            &format!("/tile-profiles/{}/update", saved.id),
            json!({"version":1,"ownerCapability":saved.capability,"name":name,"tiles":tiles}),
        )
        .await
    }
    async fn create_match(&self, words: &str, tile: &Value) -> (u16, Value, String) {
        let (status, catalog) = self.raw("GET", "/profiles/hex", "").await;
        assert_eq!(status, 200);
        let nonce = format!("{:032x}", rand_nonce());
        let mut selection = catalog["defaults"]["selection"].clone();
        selection["wordList"] = catalog["wordLists"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["identity"]["name"] == words)
            .unwrap()["identity"]
            .clone();
        selection["tile"] = tile.clone();
        let (status, body) = self
            .post(
                "/games",
                json!({"version":3,"requestId":nonce,"requestedAt":unix_seconds().unwrap(),
                    "selection":selection,"rules":catalog["defaults"]["rules"]}),
            )
            .await;
        (status, body, nonce)
    }
}
fn rand_nonce() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    (u128::from(NEXT.fetch_add(1, Ordering::Relaxed)) << 64) | u128::from(std::process::id())
}

struct Saved {
    id: String,
    capability: String,
    identity: Value,
}

/// Every Latin letter plus `extra`, `count` copies each, and two blanks.
fn tiles(count: u16, value: u16, extra: &[Value]) -> Value {
    let mut rows: Vec<Value> = ('A'..='Z')
        .map(|letter| json!({"face":{"kind":"letter","letter":letter},"count":count,"value":value}))
        .collect();
    rows.extend_from_slice(extra);
    rows.push(json!({"face":{"kind":"blank"},"count":2,"value":0}));
    Value::Array(rows)
}

/// The started match's public tile configuration, read through the native factory.
fn match_tiles(root: &Path, match_id: &str, nonce: &str) -> Value {
    let config = ServerConfig::from_values(&values(root)).unwrap();
    let factory = MatchFactory::new(&config).unwrap();
    let id = game_server::MatchId::new(match_id.to_owned()).unwrap();
    let mut simulation = factory.create(&id).unwrap();
    simulation.add_player(1).unwrap();
    let payload = serde_json::to_vec(&CommandEnvelope {
        version: ProtocolVersion,
        game_id: factory::game_id(&id),
        player_id: PlayerId::new(1),
        sequence: 1,
        expected_turn: TurnId::new(0),
        command: Command::ClaimBoard {
            request_id: nonce.into(),
        },
    })
    .unwrap();
    simulation.apply_command(1, 1, &payload).unwrap();
    let view: PlayerSnapshot =
        serde_json::from_slice(&simulation.snapshot_for(1).unwrap().payload).unwrap();
    serde_json::to_value(&view.public.configuration.tiles).unwrap()
}

#[tokio::test]
async fn only_the_owner_reads_edits_or_deletes_a_private_tile_profile() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let saved = api
        .save(
            "My tiles",
            tiles(
                3,
                1,
                &[json!({"face":{"kind":"letters","letters":"CH"},"count":2,"value":5})],
            ),
        )
        .await;
    assert!(saved.id.starts_with("t_"));
    let (status, read) = api.owner(&saved, "read", &saved.capability).await;
    assert_eq!(status, 200, "{read}");
    assert_eq!(read["name"], "My tiles");
    assert_eq!(read["revision"], 1);
    assert!(read.get("ownerCapability").is_none());
    for capability in ["0".repeat(64), String::new(), saved.id.clone()] {
        for action in ["read", "delete"] {
            let (status, _) = api.owner(&saved, action, &capability).await;
            assert_eq!(status, 403, "{action} with a forged capability");
        }
    }
    let (status, updated) = api.update(&saved, "My tiles v2", tiles(4, 2, &[])).await;
    assert_eq!(status, 200, "{updated}");
    assert_eq!(updated["revision"], 2);
    assert_ne!(
        updated["identity"], saved.identity,
        "an edit is a new version"
    );
    // No share links exist for tile profiles.
    let (status, _) = api.owner(&saved, "share", &saved.capability).await;
    assert_eq!(status, 404);

    // The capability is never stored in clear text.
    for entry in std::fs::read_dir(root.path().join("tile-profiles")).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(!text.contains(&saved.capability));
    }
    api.stop().await;
    // Durable across a restart.
    let api = Api::start(root.path()).await;
    let (status, read) = api.owner(&saved, "read", &saved.capability).await;
    assert_eq!(status, 200);
    assert_eq!(read["name"], "My tiles v2");
    let (status, _) = api.owner(&saved, "delete", &saved.capability).await;
    assert_eq!(status, 204);
    let (status, _) = api.owner(&saved, "read", &saved.capability).await;
    assert_eq!(status, 404);
    api.stop().await;
}

#[tokio::test]
async fn invalid_tile_profiles_get_a_reason_and_save_nothing() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let duplicate = tiles(
        1,
        1,
        &[json!({"face":{"kind":"letter","letter":"A"},"count":1,"value":1})],
    );
    let zero = json!([{"face":{"kind":"letter","letter":"A"},"count":0,"value":1}]);
    let valued_blank = json!([{"face":{"kind":"letter","letter":"A"},"count":1,"value":1},{"face":{"kind":"blank"},"count":1,"value":3}]);
    let too_many = tiles(8, 1, &[]);
    for (name, rows) in [
        ("Duplicate", duplicate),
        ("Zero", zero),
        ("Valued blank", valued_blank),
        ("Too many", too_many),
        ("Malformed", json!([{"face":"A"}])),
        (" padded", tiles(1, 1, &[])),
    ] {
        let (status, body) = api
            .post(
                "/tile-profiles",
                json!({"version":1,"name":name,"tiles":rows}),
            )
            .await;
        assert_eq!(status, 400, "{name}: {body}");
        assert_eq!(body["error"], "invalid-tile-profile", "{name}");
        assert!(body["reason"].as_str().is_some_and(|r| !r.is_empty()));
    }
    assert_eq!(
        std::fs::read_dir(root.path().join("tile-profiles"))
            .unwrap()
            .count(),
        0
    );
    api.stop().await;
}

#[tokio::test]
async fn saved_tiles_compose_independently_and_started_matches_keep_their_version() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let v1 = tiles(3, 7, &[]);
    let saved = api.save("Sevens", v1.clone()).await;
    let (status, join, nonce) = api.create_match("acceptance-pairs", &saved.identity).await;
    assert_eq!(status, 200, "a saved tile profile is selectable: {join}");
    let match_id = join["matchId"].as_str().unwrap().to_owned();

    let (status, body, _) = api
        .create_match("acceptance-unicode", &saved.identity)
        .await;
    assert_eq!(status, 400);
    assert_eq!(body["error"], "incompatible-profiles", "{body}");
    let mut forged = saved.identity.clone();
    forged["revision"] = json!("0".repeat(64));
    let (status, body, _) = api.create_match("acceptance-pairs", &forged).await;
    assert_eq!(status, 400);
    assert_eq!(body, json!({"error":"unknown-profile","profile":"tile"}));

    // Edit and delete the original; the started match keeps v1.
    let (status, _) = api.update(&saved, "Sevens v2", tiles(2, 1, &[])).await;
    assert_eq!(status, 200);
    let (status, _) = api.owner(&saved, "delete", &saved.capability).await;
    assert_eq!(status, 204);
    let (status, body, _) = api.create_match("acceptance-pairs", &saved.identity).await;
    assert_eq!(
        status, 400,
        "deleted profiles are unavailable for new matches"
    );
    assert_eq!(body, json!({"error":"unknown-profile","profile":"tile"}));
    api.stop().await;

    let configured: Value = match_tiles(root.path(), &match_id, &nonce);
    assert_eq!(configured, v1, "the started match retains its tile version");
}
