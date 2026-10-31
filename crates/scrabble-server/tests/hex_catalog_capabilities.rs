//! Retrospective #96 capability projection acceptance. Real configured factory,
//! Games and HTTP API; no replacement profile resolver or invented Classic mode.
use game_server::{
    LiveMatchHost, MatchHost, MatchHostWebTransportConfig, RejectMatchControlService,
    serve_live_match_host_with_control_and_shutdown,
};
use scrabble_game::ruleset::{Premium, english_fixture};
use scrabble_server::{
    api,
    config::ServerConfig,
    factory::MatchFactory,
    games::{Games, unix_seconds},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
struct Api {
    _directory: tempfile::TempDir,
    port: u16,
    task: JoinHandle<std::io::Result<()>>,
    transport: JoinHandle<()>,
    _shutdown: tokio::sync::mpsc::Sender<()>,
}
impl Drop for Api {
    fn drop(&mut self) {
        self.task.abort();
        self.transport.abort();
    }
}
impl Api {
    async fn configured(square: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let words = directory.path().join("words.txt");
        std::fs::write(
            &words,
            include_str!("../../scrabble-game/fixtures/words.txt"),
        )
        .unwrap();
        let original = english_fixture();
        let mut boards = vec![
            json!({"identity":{"name":"configured-hex","revision":"1"},"label":"Configured Hex","boardSize":7,"boardShape":"hexagon","premiumPolicy":"main_word_only","mainWordLength":"letters","premiums":vec![Premium::Normal;37]}),
        ];
        if square {
            boards.push(json!({"identity":{"name":"retained-square","revision":"1"},"label":"Retained Classic","boardSize":original.board_size,"premiums":original.premiums}));
        }
        let file = directory.path().join("catalog.json");
        std::fs::write(&file,serde_json::to_vec(&json!({"version":1,
   "wordLists":[{"identity":{"name":"configured-words","revision":"1"},"label":"Words","file":"words.txt"}],
   "tileProfiles":[{"identity":{"name":"configured-tiles","revision":"1"},"label":"Tiles","tiles":original.tiles}],
   "boardProfiles":boards,"defaultSelection":{"wordList":"configured-words","tile":"configured-tiles","board":"configured-hex"}
  })).unwrap()).unwrap();
        let config = ServerConfig::from_values(&BTreeMap::from([
            ("SCRABBLE_MATCH_IDS".into(), String::new()),
            (
                "SCRABBLE_PROFILE_CATALOG_FILE".into(),
                file.to_str().unwrap().into(),
            ),
            (
                "SCRABBLE_STATE_DIR".into(),
                directory.path().to_str().unwrap().into(),
            ),
        ]))
        .unwrap();
        let host = LiveMatchHost::new(MatchHost::new(4).unwrap());
        let games = Arc::new(Games::new(
            host.clone(),
            MatchFactory::new(&config).unwrap(),
            config.route_prefix.clone(),
            1200,
        ));
        let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
        let certificate = directory.path().join("cert.pem");
        let key = directory.path().join("key.pem");
        identity
            .certificate_chain()
            .store_pemfile(&certificate)
            .await
            .unwrap();
        identity
            .private_key()
            .store_secret_pemfile(&key)
            .await
            .unwrap();
        let (shutdown, receiver) = tokio::sync::mpsc::channel(1);
        let serving = host.clone();
        let prefix = config.route_prefix.clone();
        let transport = tokio::spawn(async move {
            serve_live_match_host_with_control_and_shutdown(
                serving,
                RejectMatchControlService,
                MatchHostWebTransportConfig {
                    port: 0,
                    certificate_pem: certificate,
                    private_key_pem: key,
                    route_prefix: prefix,
                    allowed_origins: None,
                    drain_grace: std::time::Duration::ZERO,
                },
                receiver,
            )
            .await
            .unwrap();
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !host.is_serving() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(api::serve(
            listener,
            games,
            config.board_origin.into(),
            None,
        ));
        Self {
            _directory: directory,
            port,
            task,
            transport,
            _shutdown: shutdown,
        }
    }
    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let body = body.map(|value| value.to_string()).unwrap_or_default();
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).await.unwrap();
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await.unwrap();
        let response = String::from_utf8(bytes).unwrap();
        let (header, body) = response.split_once("\r\n\r\n").unwrap();
        (
            header.split_whitespace().nth(1).unwrap().parse().unwrap(),
            serde_json::from_str(body).unwrap(),
        )
    }
    async fn create_default(&self, catalog: &Value, nonce: u8) {
        let (status,_)=self.request("POST","/games",Some(json!({"version":3,"requestId":format!("{nonce:032x}"),"requestedAt":unix_seconds().unwrap(),"selection":catalog["defaults"]["selection"],"rules":catalog["defaults"]["rules"]}))).await;
        assert_eq!(
            status, 200,
            "advertised default must resolve through real configured creation"
        );
    }
}
fn selected_board(catalog: &Value) -> &Value {
    let boards = catalog["boardProfiles"].as_array().unwrap();
    assert!(
        !boards.is_empty(),
        "a successful catalog cannot have no usable boards"
    );
    boards
        .iter()
        .find(|row| row["identity"] == catalog["defaults"]["selection"]["board"])
        .expect("successful capability projection must advertise its selected default")
}
#[tokio::test]
async fn configured_hex_default_keeps_a_usable_legacy_square_projection() {
    let api = Api::configured(true).await;
    let (status, modern) = api.request("GET", "/profiles/hex", None).await;
    assert_eq!(status, 200);
    assert_eq!(
        selected_board(&modern)["identity"]["name"],
        "configured-hex"
    );
    assert_eq!(selected_board(&modern)["boardShape"], "hexagon");
    let (status, legacy) = api.request("GET", "/profiles", None).await;
    assert_eq!(status, 200);
    assert_eq!(
        selected_board(&legacy)["identity"]["name"],
        "retained-square"
    );
    assert!(
        legacy["boardProfiles"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("boardShape").is_none()
                && row.get("premiumPolicy").is_none()
                && row.get("mainWordLength").is_none())
    );
    assert_eq!(
        legacy["defaults"]["selection"]["wordList"],
        modern["defaults"]["selection"]["wordList"]
    );
    assert_eq!(
        legacy["defaults"]["selection"]["tile"],
        modern["defaults"]["selection"]["tile"]
    );
    assert_eq!(legacy["defaults"]["rules"], modern["defaults"]["rules"]);
    // Retained original board+tiles+authored dictionary are exactly legacy-compatible.
    assert_eq!(legacy["defaults"]["legacyCompatible"], true);
    assert_eq!(modern["defaults"]["legacyCompatible"], false);
    api.create_default(&legacy, 1).await;
    api.create_default(&modern, 2).await;
}
#[tokio::test]
async fn configured_hex_only_is_explicitly_unsupported_for_legacy_geometry() {
    let api = Api::configured(false).await;
    let (status, modern) = api.request("GET", "/profiles/hex", None).await;
    assert_eq!(status, 200);
    assert_eq!(
        selected_board(&modern)["identity"]["name"],
        "configured-hex"
    );
    assert_eq!(selected_board(&modern)["boardShape"], "hexagon");
    api.create_default(&modern, 3).await;
    let (status, legacy) = api.request("GET", "/profiles", None).await;
    assert_eq!(
        status, 406,
        "a Hex-only configuration cannot return an empty/dangling successful legacy catalog"
    );
    assert!(
        legacy["error"]
            .as_str()
            .is_some_and(|message| !message.is_empty())
    );
}
