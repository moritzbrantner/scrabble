//! Word-list presets (#153): an operator catalog with `includeBuiltin` adds word lists
//! and tile profiles to the built-in profiles of the deployment dictionary, keeps the
//! built-in boards, tiles and legacy default, and composes the German and Spanish tile
//! distributions with matching lists while rejecting incompatible pairs.
use scrabble_server::{config::ServerConfig, factory::MatchFactory, games::unix_seconds};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[path = "support/hosted.rs"]
mod hosted;

const TILES: &str = include_str!("../../../scripts/preset-tile-profiles.json");

fn write_presets(root: &Path, catalog: Value) {
    let directory = root.join("presets");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("words-de.txt"), "HAUS\nÄRGER\nSTRASSE\n").unwrap();
    std::fs::write(directory.join("words-es.txt"), "CHICO\nLLAVE\nNIÑO\n").unwrap();
    std::fs::write(directory.join("catalog.json"), catalog.to_string()).unwrap();
}
fn presets() -> Value {
    json!({
        "version": 1,
        "includeBuiltin": true,
        "builtinWordListLabel": "English (US) — test",
        "wordLists": [
            {"identity":{"name":"test-de","revision":"1"},"label":"Deutsch — test","file":"words-de.txt"},
            {"identity":{"name":"test-es","revision":"1"},"label":"Español — test","file":"words-es.txt"}
        ],
        "tileProfiles": serde_json::from_str::<Value>(TILES).unwrap(),
    })
}

fn values(root: &Path) -> BTreeMap<String, String> {
    let catalog = root.join("presets").join("catalog.json");
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
    let mut values = BTreeMap::from([
        ("SCRABBLE_MATCH_IDS".into(), String::new()),
        ("SCRABBLE_TEST_SEED".into(), "0d".repeat(32)),
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
    ]);
    // Opt-in acceptance uses the real operator-prepared US baseline, not fixture words.
    if root.join("words.txt").exists() {
        values.insert("SCRABBLE_DICTIONARY_FILE".into(), path("words.txt"));
        values.insert("SCRABBLE_DICTIONARY_NAME".into(), "scowl-en-us-60".into());
        values.insert(
            "SCRABBLE_DICTIONARY_REVISION".into(),
            "2026.02.25-recipe1".into(),
        );
    }
    values
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
}

fn rand_nonce() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    (u128::from(NEXT.fetch_add(1, Ordering::Relaxed)) << 64) | u128::from(std::process::id())
}
fn identity(catalog: &Value, kind: &str, name: &str) -> Value {
    catalog[kind]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["identity"]["name"] == name)
        .unwrap_or_else(|| panic!("{kind} lacks {name}"))["identity"]
        .clone()
}
async fn create(api: &Api, catalog: &Value, words: &str, tiles: &str) -> (u16, Value) {
    let mut selection = catalog["defaults"]["selection"].clone();
    selection["wordList"] = identity(catalog, "wordLists", words);
    selection["tile"] = identity(catalog, "tileProfiles", tiles);
    api.post(
        "/games",
        json!({"version":3,"requestId":format!("{:032x}", rand_nonce()),
            "requestedAt":unix_seconds().unwrap(),"selection":selection,
            "rules":catalog["defaults"]["rules"]}),
    )
    .await
}

#[tokio::test]
async fn presets_extend_the_builtin_catalog_and_compose_by_language() {
    let root = tempfile::tempdir().unwrap();
    write_presets(root.path(), presets());
    let api = Api::start(root.path()).await;
    let (status, catalog) = api.raw("GET", "/profiles/hex", "").await;
    assert_eq!(status, 200, "{catalog}");
    let labels = |kind: &str| -> Vec<String> {
        catalog[kind]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["label"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        labels("wordLists"),
        ["English (US) — test", "Deutsch — test", "Español — test"]
    );
    let tiles = labels("tileProfiles");
    assert!(tiles.contains(&"Default tiles".to_owned()), "{tiles:?}");
    assert!(
        tiles.contains(&"English + CH / SCH".to_owned()),
        "{tiles:?}"
    );
    assert!(tiles.iter().any(|label| label.starts_with("Deutsch")));
    assert!(tiles.iter().any(|label| label.starts_with("Español")));
    assert!(
        labels("boardProfiles").len() > 1,
        "built-in boards are kept"
    );
    assert_eq!(catalog["defaults"]["legacyCompatible"], true);

    let builtin = catalog["wordLists"][0]["identity"]["name"]
        .as_str()
        .unwrap()
        .to_owned();
    for (words, tiles, expected) in [
        ("test-de", "german-standard", 200),
        ("test-es", "spanish-standard", 200),
        (builtin.as_str(), "german-standard", 200),
        ("test-es", "fixture-en", 400),
        ("test-de", "spanish-standard", 400),
    ] {
        let tiles = if tiles == "fixture-en" {
            catalog["defaults"]["selection"]["tile"]["name"]
                .as_str()
                .unwrap()
                .to_owned()
        } else {
            tiles.to_owned()
        };
        let (status, body) = create(&api, &catalog, words, &tiles).await;
        assert_eq!(status, expected, "{words} with {tiles}: {body}");
        if expected == 400 {
            assert_eq!(body["error"], "incompatible-profiles", "{body}");
        }
    }
    api.stop().await;
}

#[test]
fn a_catalog_needs_builtin_profiles_or_its_own_default_selection() {
    let root = tempfile::tempdir().unwrap();
    let mut catalog = presets();
    catalog["includeBuiltin"] = json!(false);
    catalog
        .as_object_mut()
        .unwrap()
        .remove("builtinWordListLabel");
    write_presets(root.path(), catalog);
    let config = ServerConfig::from_values(&values(root.path())).unwrap();
    assert!(MatchFactory::new(&config).is_err());
}

/// Run after the documented preparation commands with SCRABBLE_PRESET_ACCEPTANCE_ROOT
/// containing baseline/ and presets/. Never downloads a corpus in the deterministic gate.
#[tokio::test]
#[ignore = "requires operator-prepared, checksum-verified word lists"]
async fn prepared_enable2k_creates_validates_and_recovers_an_immutable_match() {
    use game_server::{MatchRuntime, ReconnectToken, RecoveryImage};
    use scrabble_game::{
        identity::{Coordinate, PlayerId, ProtocolVersion, TurnId},
        protocol::{Command, CommandEnvelope, Placement, PlayerSnapshot},
        ruleset::TileFace,
    };

    let prepared = std::path::PathBuf::from(
        std::env::var("SCRABBLE_PRESET_ACCEPTANCE_ROOT").expect("prepared corpus root"),
    );
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("presets")).unwrap();
    for entry in std::fs::read_dir(prepared.join("presets")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(
            entry.path(),
            root.path().join("presets").join(entry.file_name()),
        )
        .unwrap();
    }
    std::fs::copy(
        prepared.join("baseline/words.txt"),
        root.path().join("words.txt"),
    )
    .unwrap();
    let text = std::fs::read_to_string(root.path().join("presets/words-enable2k-en.txt")).unwrap();
    let words: std::collections::BTreeSet<_> = text.lines().collect();
    assert_eq!(words.len(), 169266);
    assert!(words.contains("OD") && words.contains("JOB"));

    let api = Api::start(root.path()).await;
    let (status, catalog) = api.raw("GET", "/profiles/hex", "").await;
    assert_eq!(status, 200);
    assert_eq!(catalog["defaults"]["legacyCompatible"], true);
    assert_eq!(
        catalog["defaults"]["selection"]["wordList"]["name"],
        "scowl-en-us-60"
    );
    assert!(!identity(&catalog, "wordLists", "scowl-en-gb-60").is_null());
    let mut selection = catalog["defaults"]["selection"].clone();
    selection["wordList"] = identity(&catalog, "wordLists", "enable2k-en");
    let mut rules = catalog["defaults"]["rules"].clone();
    rules["rack_size"] = json!(15);
    let nonce = format!("{:032x}", rand_nonce());
    let body = json!({"version":3,"requestId":nonce,"requestedAt":unix_seconds().unwrap(),
        "selection":selection,"rules":rules});
    let (status, created) = api.post("/games", body.clone()).await;
    assert_eq!(status, 200, "{created}");
    let (status, retried) = api.post("/games", body.clone()).await;
    assert_eq!(
        (status, retried),
        (200, created.clone()),
        "creation remains idempotent"
    );
    let mut forged = body;
    forged["requestId"] = json!(format!("{:032x}", rand_nonce()));
    forged["selection"]["wordList"]["revision"] = json!("unreviewed-revision");
    let (status, rejection) = api.post("/games", forged).await;
    assert_eq!(status, 400, "{rejection}");
    api.stop().await;
    let api = Api::start(root.path()).await;
    let id = created["matchId"].as_str().unwrap();
    assert_eq!(api.raw("GET", &format!("/games/{id}"), "").await.0, 200);
    api.stop().await;

    let config = ServerConfig::from_values(&values(root.path())).unwrap();
    let factory = MatchFactory::new(&config).unwrap();
    let id = game_server::MatchId::new(id.to_owned()).unwrap();
    let mut runtime = MatchRuntime::new_with_replay_capture(factory.create(&id).unwrap(), 100);
    for player in 1..=3 {
        runtime.admit(ReconnectToken([player; 16])).unwrap();
    }
    let mut sequences = [0_u32; 3];
    let mut send = |runtime: &mut MatchRuntime<_>, player: u32, command: Command| {
        let snapshot: PlayerSnapshot =
            serde_json::from_slice(&runtime.snapshot_for(player).unwrap().payload).unwrap();
        let turn = match snapshot.public.phase {
            scrabble_game::protocol::Phase::Playing { turn, .. } => turn.get(),
            _ => 0,
        };
        let sequence = &mut sequences[(player - 1) as usize];
        *sequence += 1;
        let payload = serde_json::to_vec(&CommandEnvelope {
            version: ProtocolVersion,
            game_id: scrabble_server::factory::game_id(&id),
            player_id: PlayerId::new(u64::from(player)),
            sequence: *sequence,
            expected_turn: TurnId::new(turn),
            command,
        })
        .unwrap();
        runtime.submit_command(player, 1, *sequence, &payload)
    };
    send(&mut runtime, 1, Command::ClaimBoard { request_id: nonce }).unwrap();
    for (player, name) in [(2, "Ada"), (3, "Lin")] {
        send(
            &mut runtime,
            player,
            Command::SetName {
                display_name: name.into(),
            },
        )
        .unwrap();
    }
    send(&mut runtime, 1, Command::Start {}).unwrap();
    let view: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(2).unwrap().payload).unwrap();
    assert_eq!(view.public.dictionary.name, "enable2k-en");
    let mut valid = None;
    let mut invalid = None;
    for first in &view.own_rack.tiles {
        for second in &view.own_rack.tiles {
            if first.id == second.id {
                continue;
            }
            if let (TileFace::Letter { letter: a }, TileFace::Letter { letter: b }) =
                (&first.face, &second.face)
            {
                let word = format!("{a}{b}");
                let placements = vec![
                    Placement {
                        tile_id: first.id,
                        coordinate: Coordinate::new(7, 7).unwrap(),
                        blank_as: None,
                    },
                    Placement {
                        tile_id: second.id,
                        coordinate: Coordinate::new(7, 8).unwrap(),
                        blank_as: None,
                    },
                ];
                if words.contains(word.as_str()) {
                    valid = Some(placements);
                } else {
                    invalid = Some(placements);
                }
            }
        }
    }
    let before = runtime.snapshot().unwrap();
    let error = send(
        &mut runtime,
        2,
        Command::Commit {
            placements: invalid.expect("invalid pair in real deal"),
        },
    )
    .unwrap_err();
    let game_server::RuntimeError::Simulation(error) = error else {
        panic!("expected a private word-validation rejection: {error}");
    };
    let feedback: Value = serde_json::from_slice(error.command_rejection().unwrap()).unwrap();
    assert_eq!(feedback["error"]["kind"], "invalid_words");
    assert_eq!(
        runtime.snapshot().unwrap(),
        before,
        "invalid word cannot mutate state"
    );
    send(
        &mut runtime,
        2,
        Command::Commit {
            placements: valid.expect("valid ENABLE2K pair in real deal"),
        },
    )
    .unwrap();
    let after = runtime.snapshot().unwrap();
    let view: PlayerSnapshot =
        serde_json::from_slice(&runtime.snapshot_for(2).unwrap().payload).unwrap();
    assert_eq!(view.public.history.len(), 1);

    runtime.freeze_for_recovery();
    let image_path = root.path().join("enable2k-recovery");
    runtime
        .recovery_image()
        .unwrap()
        .write_atomic(&image_path)
        .unwrap();
    // Future catalog selections may change; the original match must use its retained words.
    let catalog_path = root.path().join("presets/catalog.json");
    let mut catalog: Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    catalog["wordLists"]
        .as_array_mut()
        .unwrap()
        .retain(|row| row["identity"]["name"] != "enable2k-en");
    std::fs::write(catalog_path, catalog.to_string()).unwrap();
    std::fs::remove_file(root.path().join("presets/words-enable2k-en.txt")).unwrap();
    let factory = MatchFactory::new(&config).unwrap();
    let image = RecoveryImage::read_file(&image_path).unwrap();
    let report = scrabble_server::replay::inspect_replay(&factory, &id, &image).unwrap();
    assert_eq!(report.configuration, view.public.configuration);
    assert_eq!(report.game.history.len(), 1);
    let recovered =
        MatchRuntime::restore_from_recovery(factory.create(&id).unwrap(), image).unwrap();
    assert_eq!(recovered.snapshot().unwrap(), after);
}
