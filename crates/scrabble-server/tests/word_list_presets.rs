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
    BTreeMap::from([
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
