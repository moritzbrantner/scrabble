//! Independent acceptance contract for #101, first slice: GUEST-OWNED custom
//! regular square/hex board layouts (Decisions 1.3, 1.4, 1.10-1.13, 1.15, 1.23,
//! 1.31, 1.33, 1.35, 1.37 and 1.45). Authored before any implementation by a
//! separate agent; it exercises the real configured factory, `Games`, the HTTP
//! API and the native engine. Nothing here re-implements placement or scoring.
//!
//! # Seams this contract defines (the implementation must provide them)
//!
//! Storage: a new optional server configuration key `SCRABBLE_BOARD_LAYOUT_DIR`
//! names a durable, private library directory that survives restarts. The
//! server stores no owner capability in clear text anywhere on disk (only a
//! verifier, e.g. a keyed hash).
//!
//! Layout document (`Layout`), reusing the authoritative catalog field names:
//! `{"boardShape":"square"|"hexagon","boardSize":<odd side>,"premiums":[..],
//!   "premiumPolicy"?:"all_word"|"main_word_only","mainWordLength"?:"tiles"|"letters"}`.
//! `premiums` uses the existing `Premium` kinds only (Decision 1.3) and is
//! row-major over the ON-BOARD cells exactly like `Ruleset::premiums`
//! (`boardSize²` for squares, `3R²+3R+1` for a hex of side `2R+1`). Footprints
//! are always complete regular grids (Decision 1.37): there is no way to omit
//! a cell, so a wrong cell count is an irregular/holed submission. Bounds are
//! the ESTABLISHED engine limits (`validate_board`, `validate_hex_board`):
//! odd sides, square <= 15, hex 3..=15; even sides have no unique centre.
//! Hex policy fields are rejected on square boards.
//!
//! HTTP endpoints on the existing game API origin (JSON, `version: 1`, 200 on
//! success; every mutation is a POST because the API's framing only accepts
//! bodies on POST):
//! - `POST /layouts` `{version,name,layout}` -> `{version,layoutId,ownerCapability,
//!   name,revision:1,board,layout}`. `layoutId` = `l_<32 hex>`,
//!   `ownerCapability` = 64 lowercase hex, returned only to the creator.
//! - `POST /layouts/{layoutId}/read` `{version,ownerCapability}` -> `{version,
//!   layoutId,name,revision,board,layout,...}`.
//! - `POST /layouts/{layoutId}/update` `{version,ownerCapability,name,layout}` ->
//!   same shape with `revision + 1` and a NEW immutable `board` identity.
//! - `POST /layouts/{layoutId}/delete` `{version,ownerCapability}` -> 200 or 204.
//! - `POST /layouts/{layoutId}/share` `{version,ownerCapability}` -> `{version,
//!   shareId}`; `shareId` = `s_<32 hex>`, stable for repeated calls.
//! - `GET /shared-layouts/{shareId}` -> `{version,shareId,name,revision,board,
//!   layout}` of the original's CURRENT version; never an owner capability.
//! - `POST /shared-layouts/{shareId}/copy` `{version,name}` -> the `POST /layouts`
//!   response shape for a NEW, independently owned original.
//! - `POST /games` (version 3, unchanged): `selection.board` may be the `board`
//!   identity of any saved version returned above; word list and tile profile
//!   still come from the operator catalog (Decision 1.31).
//!
//! Errors: 400 `{"error":"invalid-layout","reason":..}` for well-formed but
//! invalid geometry (structurally malformed bodies may also be
//! `invalid-request`); 403 `{"error":"layout-owner-rejected"}` for a missing or
//! forged capability; 404 `{"error":"unknown-layout"}` for unknown/deleted ids.
//! Deleted layouts are rejected for NEW matches as
//! `{"error":"unknown-profile","profile":"board"}`; started matches keep their
//! version through the existing per-match initialization manifest.
//!
//! Share withdrawal (Decision 1.35) and account linking (Question 1.44, #132)
//! are successor scope and deliberately not asserted here.
use game_server::{
    GameSimulation, LiveMatchHost, MatchHost, MatchHostWebTransportConfig,
    RejectMatchControlService, serve_live_match_host_with_control_and_shutdown,
};
use scrabble_game::{
    identity::{Coordinate, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Phase, Placement, PlayerSnapshot},
    ruleset::{BoardShape, Premium, TileFace, hex_contains},
};
use scrabble_server::{
    config::ServerConfig,
    factory::{self, MatchFactory},
    game_api,
    games::{Games, unix_seconds},
    simulation::ScrabbleSimulation,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

#[path = "support/profile_catalog.rs"]
mod profile_catalog;

const LAYOUT_DIR_KEY: &str = "SCRABBLE_BOARD_LAYOUT_DIR";

fn values(root: &Path) -> BTreeMap<String, String> {
    let catalog = root.join("profile-catalog.json");
    if !catalog.exists() {
        profile_catalog::write_catalog(root, profile_catalog::LEGACY_WORDS).unwrap();
    }
    let path = |name: &str| root.join(name).to_str().unwrap().to_owned();
    BTreeMap::from([
        ("SCRABBLE_MATCH_IDS".into(), String::new()),
        ("SCRABBLE_TEST_SEED".into(), "0b".repeat(32)),
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
        (LAYOUT_DIR_KEY.into(), path("layouts")),
    ])
}
fn config(root: &Path) -> ServerConfig {
    ServerConfig::from_values(&values(root))
        .expect("#101 requires the durable guest layout library key SCRABBLE_BOARD_LAYOUT_DIR")
}

/// Real HTTP API + live host + WebTransport serving, restartable on one directory.
struct Api {
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
    async fn start(root: &Path) -> Self {
        let config = config(root);
        let host = LiveMatchHost::new(MatchHost::new(8).unwrap());
        let games = Arc::new(Games::new(
            host.clone(),
            MatchFactory::new(&config).unwrap(),
            config.route_prefix.clone(),
            1200,
        ));
        let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
        let certificate = root.join("cert.pem");
        let key = root.join("key.pem");
        let _ = std::fs::remove_file(&certificate);
        let _ = std::fs::remove_file(&key);
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
        let task = tokio::spawn(game_api::serve(listener, games, config.board_origin));
        Self {
            port,
            task,
            transport,
            _shutdown: shutdown,
        }
    }
    async fn stop(self) {
        drop(self);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    async fn raw(&self, method: &str, path: &str, body: &str) -> (u16, Value) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).await.unwrap();
        // A bounded server may answer (e.g. 413) and close before reading an
        // oversized body; keep whatever response it sent.
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
    async fn get(&self, path: &str) -> (u16, Value) {
        self.raw("GET", path, "").await
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.raw("POST", path, &body.to_string()).await
    }
    async fn save(&self, name: &str, layout: Value) -> Saved {
        let (status, body) = self
            .post("/layouts", json!({"version":1,"name":name,"layout":layout}))
            .await;
        assert_eq!(status, 200, "a valid guest layout must save: {body}");
        Saved::parse(&body)
    }
    async fn read(&self, saved: &Saved) -> (u16, Value) {
        self.post(
            &format!("/layouts/{}/read", saved.id),
            json!({"version":1,"ownerCapability":saved.capability}),
        )
        .await
    }
    async fn update(&self, saved: &Saved, name: &str, layout: Value) -> (u16, Value) {
        self.post(
            &format!("/layouts/{}/update", saved.id),
            json!({"version":1,"ownerCapability":saved.capability,"name":name,"layout":layout}),
        )
        .await
    }
    async fn share(&self, saved: &Saved) -> String {
        let (status, body) = self
            .post(
                &format!("/layouts/{}/share", saved.id),
                json!({"version":1,"ownerCapability":saved.capability}),
            )
            .await;
        assert_eq!(status, 200, "the owner can share its original: {body}");
        let share = body["shareId"].as_str().unwrap().to_owned();
        assert!(is_id(&share, "s_"), "share id format: {share}");
        share
    }
    async fn catalog(&self) -> Value {
        let (status, catalog) = self.get("/profiles/hex").await;
        assert_eq!(status, 200);
        catalog
    }
    /// Creates a v3 match; returns (status, body, creation nonce).
    async fn create_match(
        &self,
        catalog: &Value,
        words: &str,
        board: &Value,
    ) -> (u16, Value, String) {
        let nonce = format!("{:032x}", rand_nonce());
        let mut selection = catalog["defaults"]["selection"].clone();
        selection["wordList"] = identity_of(catalog, "wordLists", words);
        selection["board"] = board.clone();
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
fn identity_of(catalog: &Value, kind: &str, name: &str) -> Value {
    catalog[kind]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["identity"]["name"] == name)
        .unwrap_or_else(|| panic!("catalog {kind} lacks {name}"))["identity"]
        .clone()
}
fn is_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|rest| {
        rest.len() == 32
            && rest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn is_capability(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Clone, Debug)]
struct Saved {
    id: String,
    capability: String,
    revision: u64,
    board: Value,
}
impl Saved {
    fn parse(body: &Value) -> Self {
        let id = body["layoutId"].as_str().expect("layoutId").to_owned();
        let capability = body["ownerCapability"]
            .as_str()
            .expect("ownerCapability is returned to the creator")
            .to_owned();
        assert!(is_id(&id, "l_"), "layout id format: {id}");
        assert!(is_capability(&capability), "owner capability format");
        assert_ne!(id, capability);
        let board = body["board"].clone();
        assert!(
            board["name"].as_str().is_some_and(|s| !s.is_empty())
                && board["revision"].as_str().is_some_and(|s| !s.is_empty()),
            "each saved version exposes a ContentIdentity usable as selection.board: {body}"
        );
        Self {
            id,
            capability,
            revision: body["revision"].as_u64().expect("revision"),
            board,
        }
    }
}

fn premium_name(premium: Premium) -> &'static str {
    match premium {
        Premium::Normal => "normal",
        Premium::DoubleLetter => "double_letter",
        Premium::TripleLetter => "triple_letter",
        Premium::DoubleWord => "double_word",
        Premium::TripleWord => "triple_word",
    }
}
fn square(size: u8, marks: &[(u8, u8, Premium)]) -> Value {
    let mut premiums = vec!["normal"; usize::from(size).pow(2)];
    for &(row, column, premium) in marks {
        premiums[usize::from(row) * usize::from(size) + usize::from(column)] =
            premium_name(premium);
    }
    json!({"boardShape":"square","boardSize":size,"premiums":premiums})
}
fn hex_cells(size: u8) -> Vec<(u8, u8)> {
    (0..size)
        .flat_map(|row| (0..size).map(move |column| (row, column)))
        .filter(|&(row, column)| hex_contains(size, row, column))
        .collect()
}
fn hex(size: u8, marks: &[(u8, u8, Premium)], policy: &str) -> Value {
    let cells = hex_cells(size);
    let mut premiums = vec!["normal"; cells.len()];
    for &(row, column, premium) in marks {
        let index = cells
            .iter()
            .position(|&cell| cell == (row, column))
            .unwrap();
        premiums[index] = premium_name(premium);
    }
    json!({"boardShape":"hexagon","boardSize":size,"premiums":premiums,
        "premiumPolicy":policy,"mainWordLength":"tiles"})
}
fn files_containing(root: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if std::fs::read(&path)
                .unwrap()
                .windows(needle.len())
                .any(|window| window == needle.as_bytes())
            {
                found.push(path);
            }
        }
    }
    found
}
fn file_count(root: &Path) -> usize {
    if !root.exists() {
        return 0;
    }
    let mut count = 0;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                count += 1;
            }
        }
    }
    count
}

#[tokio::test]
async fn guest_saves_square_and_hex_originals_owned_only_by_a_private_capability() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let square_layout = square(
        9,
        &[(4, 4, Premium::TripleWord), (0, 0, Premium::DoubleLetter)],
    );
    let hex_layout = hex(7, &[(3, 3, Premium::DoubleWord)], "main_word_only");
    let first = api.save("Nine square", square_layout.clone()).await;
    let second = api.save("Seven hex", hex_layout.clone()).await;
    assert_eq!((first.revision, second.revision), (1, 1));
    assert_ne!(first.id, second.id);
    assert_ne!(first.capability, second.capability);
    assert_ne!(first.board, second.board);

    for (saved, layout, name) in [
        (&first, &square_layout, "Nine square"),
        (&second, &hex_layout, "Seven hex"),
    ] {
        let (status, body) = api.read(saved).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["name"], name);
        assert_eq!(body["revision"], 1);
        assert_eq!(body["board"], saved.board);
        for field in ["boardShape", "boardSize", "premiums"] {
            assert_eq!(body["layout"][field], layout[field], "{field} round-trips");
        }
    }
    let (_, read) = api.read(&second).await;
    assert_eq!(read["layout"]["premiumPolicy"], "main_word_only");

    // Saved guest layouts are private library entries, not public catalog rows,
    // and no response other than creation repeats the capability.
    let catalog = api.catalog().await;
    let public = catalog.to_string();
    for saved in [&first, &second] {
        assert!(!public.contains(&saved.capability));
        assert!(!public.contains(&saved.id));
    }
    // The server keeps a verifier, never the bearer capability itself.
    for saved in [&first, &second] {
        assert_eq!(
            files_containing(root.path(), &saved.capability),
            Vec::<PathBuf>::new(),
            "owner capabilities must not be persisted in clear text"
        );
    }
    api.stop().await;
}

#[tokio::test]
async fn bounded_validation_rejects_irregular_holed_out_of_bounds_and_centerless_layouts() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    // Established maxima and the smallest hex are accepted (no new bounds invented).
    api.save("Max square", square(15, &[(7, 7, Premium::DoubleWord)]))
        .await;
    api.save(
        "Max hex",
        hex(15, &[(7, 7, Premium::TripleLetter)], "all_word"),
    )
    .await;
    api.save("Min hex", hex(3, &[], "all_word")).await;
    let before = file_count(&root.path().join("layouts"));

    let mut holed = square(9, &[]);
    holed["premiums"].as_array_mut().unwrap().pop();
    let mut hex_as_square_storage = hex(7, &[], "all_word");
    hex_as_square_storage["premiums"] = json!(vec!["normal"; 49]);
    let mut irregular_extra_field = square(9, &[]);
    irregular_extra_field["omittedCells"] = json!([[0, 0]]);
    let mut invented_premium = square(9, &[]);
    invented_premium["premiums"][0] = json!("quadruple_word");
    let mut square_with_hex_policy = square(9, &[]);
    square_with_hex_policy["premiumPolicy"] = json!("main_word_only");
    let mut unknown_shape = square(9, &[]);
    unknown_shape["boardShape"] = json!("triangle");
    let geometry = [
        ("holed square", holed),
        ("hex sent as full square storage", hex_as_square_storage),
        (
            "even square has no unique centre",
            json!({"boardShape":"square","boardSize":8,"premiums":vec!["normal";64]}),
        ),
        (
            "even hex has no unique centre",
            json!({"boardShape":"hexagon","boardSize":8,"premiums":vec!["normal";37],"premiumPolicy":"all_word"}),
        ),
        (
            "square above bound",
            json!({"boardShape":"square","boardSize":17,"premiums":vec!["normal";289]}),
        ),
        (
            "hex above bound",
            json!({"boardShape":"hexagon","boardSize":17,"premiums":vec!["normal";217],"premiumPolicy":"all_word"}),
        ),
        (
            "hex below bound",
            json!({"boardShape":"hexagon","boardSize":1,"premiums":["normal"],"premiumPolicy":"all_word"}),
        ),
        (
            "empty square",
            json!({"boardShape":"square","boardSize":0,"premiums":[]}),
        ),
        ("square with hex policy", square_with_hex_policy),
    ];
    for (case, layout) in geometry {
        let (status, body) = api
            .post("/layouts", json!({"version":1,"name":case,"layout":layout}))
            .await;
        assert_eq!(status, 400, "{case} must be rejected: {body}");
        assert_eq!(body["error"], "invalid-layout", "{case}: {body}");
        assert!(
            body["reason"].as_str().is_some_and(|r| !r.is_empty()),
            "{case} explains"
        );
        assert!(body.get("layoutId").is_none() && body.get("ownerCapability").is_none());
    }
    for (case, layout) in [
        ("explicit irregular footprint field", irregular_extra_field),
        ("invented premium kind", invented_premium),
        ("unknown shape", unknown_shape),
    ] {
        let (status, body) = api
            .post("/layouts", json!({"version":1,"name":case,"layout":layout}))
            .await;
        assert_eq!(status, 400, "{case} must be rejected: {body}");
        assert!(
            matches!(
                body["error"].as_str(),
                Some("invalid-layout" | "invalid-request")
            ),
            "{case}: {body}"
        );
    }
    let (status, _) = api.raw("POST", "/layouts", "{not json").await;
    assert_eq!(status, 400);
    let (status, _) = api
        .raw(
            "POST",
            "/layouts",
            &format!("{{\"pad\":\"{}\"}}", "x".repeat(70 * 1024)),
        )
        .await;
    assert_eq!(status, 413, "layout requests stay bounded");
    assert_eq!(
        file_count(&root.path().join("layouts")),
        before,
        "rejected submissions save nothing, not even partial layouts"
    );
    api.stop().await;
}

#[tokio::test]
async fn only_the_private_capability_reads_edits_shares_or_deletes_an_original() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let original = api
        .save("Owned", hex(7, &[(3, 3, Premium::DoubleWord)], "all_word"))
        .await;
    let other = api.save("Other guest", square(5, &[])).await;
    let forged = "0".repeat(64);
    for capability in [
        Value::Null,
        json!(""),
        json!(forged),
        json!(other.capability),
    ] {
        for (action, extra) in [
            ("read", json!({})),
            ("update", json!({"name":"Hijacked","layout":square(5, &[])})),
            ("share", json!({})),
            ("delete", json!({})),
        ] {
            let mut body = json!({"version":1,"ownerCapability":capability});
            for (key, value) in extra.as_object().unwrap() {
                body[key] = value.clone();
            }
            let (status, response) = api
                .post(&format!("/layouts/{}/{action}", original.id), body)
                .await;
            assert!(
                matches!(status, 400 | 403),
                "{action} with a forged capability must fail: {status} {response}"
            );
            if status == 403 {
                assert_eq!(response["error"], "layout-owner-rejected");
            }
            assert!(!response.to_string().contains(&original.capability));
        }
    }
    let (status, unchanged) = api.read(&original).await;
    assert_eq!(status, 200);
    assert_eq!(unchanged["name"], "Owned");
    assert_eq!(unchanged["revision"], 1);
    assert_eq!(unchanged["board"], original.board);

    let edited_layout = hex(7, &[(3, 3, Premium::TripleWord)], "all_word");
    let (status, edited) = api
        .update(&original, "Owned v2", edited_layout.clone())
        .await;
    assert_eq!(status, 200, "{edited}");
    assert_eq!(edited["revision"], 2);
    assert_eq!(edited["layoutId"], original.id.as_str());
    assert_ne!(
        edited["board"], original.board,
        "an edit is a new immutable version"
    );
    let (_, current) = api.read(&original).await;
    assert_eq!(current["layout"]["premiums"], edited_layout["premiums"]);
    assert_eq!(current["name"], "Owned v2");

    // An invalid edit is rejected without replacing the saved version.
    let (status, body) = api
        .update(
            &original,
            "Broken",
            json!({"boardShape":"hexagon","boardSize":6,"premiums":[]}),
        )
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (400, Some("invalid-layout"))
    );
    let (_, still) = api.read(&original).await;
    assert_eq!(still["revision"], 2);

    let (status, _) = api
        .post(
            &format!("/layouts/{}/delete", original.id),
            json!({"version":1,"ownerCapability":original.capability}),
        )
        .await;
    assert!(matches!(status, 200 | 204));
    let (status, body) = api.read(&original).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (404, Some("unknown-layout"))
    );
    let (status, _) = api.read(&other).await;
    assert_eq!(
        status, 200,
        "deleting one guest's original leaves others intact"
    );
    api.stop().await;
}

#[tokio::test]
async fn share_links_grant_use_and_copy_but_never_edit_and_copies_are_independent() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let source_layout = square(
        9,
        &[(4, 4, Premium::DoubleWord), (1, 1, Premium::TripleLetter)],
    );
    let original = api.save("Shared square", source_layout.clone()).await;
    let share = api.share(&original).await;
    assert_eq!(api.share(&original).await, share, "sharing is idempotent");

    let (status, shared) = api.get(&format!("/shared-layouts/{share}")).await;
    assert_eq!(status, 200, "{shared}");
    assert_eq!(shared["name"], "Shared square");
    assert_eq!(shared["layout"]["premiums"], source_layout["premiums"]);
    assert_eq!(shared["board"], original.board);
    assert!(shared.get("ownerCapability").is_none());
    assert!(!shared.to_string().contains(&original.capability));
    // The share id is not an owner credential for any management action.
    for action in ["read", "update", "share", "delete"] {
        let (status, body) = api
            .post(
                &format!("/layouts/{}/{action}", original.id),
                json!({"version":1,"ownerCapability":share,"name":"x","layout":square(5,&[])}),
            )
            .await;
        assert!(
            matches!(status, 400 | 403),
            "{action} via share id: {status} {body}"
        );
    }
    let (status, _) = api
        .get(&format!("/shared-layouts/s_{}", "f".repeat(32)))
        .await;
    assert_eq!(status, 404);

    let (status, copied) = api
        .post(
            &format!("/shared-layouts/{share}/copy"),
            json!({"version":1,"name":"My copy"}),
        )
        .await;
    assert_eq!(status, 200, "{copied}");
    let copy = Saved::parse(&copied);
    assert_ne!(copy.id, original.id);
    assert_ne!(copy.capability, original.capability);
    assert_eq!(copy.revision, 1);
    assert_eq!(copied["layout"]["premiums"], source_layout["premiums"]);
    let (status, _) = api
        .update(
            &Saved {
                capability: copy.capability.clone(),
                ..original.clone()
            },
            "copy owner hijack",
            square(5, &[]),
        )
        .await;
    assert_eq!(
        status, 403,
        "the copy's capability cannot manage the source"
    );
    let (status, _) = api
        .update(
            &Saved {
                capability: original.capability.clone(),
                ..copy.clone()
            },
            "source owner hijack",
            square(5, &[]),
        )
        .await;
    assert_eq!(
        status, 403,
        "the source owner cannot manage a recipient's copy"
    );

    // Editing the copy (resize + premiums) leaves the source and its share untouched.
    let copy_layout = square(11, &[(5, 5, Premium::TripleWord)]);
    let (status, _) = api
        .update(&copy, "My bigger copy", copy_layout.clone())
        .await;
    assert_eq!(status, 200);
    let (_, source_now) = api.read(&original).await;
    assert_eq!(source_now["revision"], 1);
    assert_eq!(source_now["layout"]["boardSize"], 9);
    assert_eq!(source_now["layout"]["premiums"], source_layout["premiums"]);
    let (_, shared_now) = api.get(&format!("/shared-layouts/{share}")).await;
    assert_eq!(shared_now["layout"]["premiums"], source_layout["premiums"]);

    // Editing the source changes what the share shows, never the saved copy.
    let source_v2 = square(9, &[(4, 4, Premium::TripleWord)]);
    let (status, _) = api
        .update(&original, "Shared square v2", source_v2.clone())
        .await;
    assert_eq!(status, 200);
    let (_, shared_v2) = api.get(&format!("/shared-layouts/{share}")).await;
    assert_eq!(shared_v2["revision"], 2);
    assert_eq!(shared_v2["layout"]["premiums"], source_v2["premiums"]);
    let (_, copy_now) = api.read(&copy).await;
    assert_eq!(copy_now["layout"]["boardSize"], 11);
    assert_eq!(copy_now["layout"]["premiums"], copy_layout["premiums"]);
    // A second recipient's copy is independent of the first one.
    let (_, second) = api
        .post(
            &format!("/shared-layouts/{share}/copy"),
            json!({"version":1,"name":"Second"}),
        )
        .await;
    let second = Saved::parse(&second);
    assert_ne!(second.id, copy.id);
    let (_, second_now) = api.read(&second).await;
    assert_eq!(second_now["layout"]["premiums"], source_v2["premiums"]);
    for saved in [&original, &copy, &second] {
        assert!(files_containing(root.path(), &saved.capability).is_empty());
    }
    api.stop().await;
}

#[tokio::test]
async fn match_creation_validates_custom_layout_combinations_authoritatively() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let catalog = api.catalog().await;
    let saved = api
        .save(
            "Combo hex",
            hex(7, &[(3, 3, Premium::TripleWord)], "main_word_only"),
        )
        .await;
    let (status, join, _) = api
        .create_match(&catalog, "acceptance-pairs", &saved.board)
        .await;
    assert_eq!(
        status, 200,
        "a saved layout is selectable at match setup: {join}"
    );
    assert!(join["matchId"].as_str().unwrap().starts_with("p_"));

    let mut forged = saved.board.clone();
    forged["revision"] = json!(format!("{}0", saved.board["revision"].as_str().unwrap()));
    let (status, body, _) = api
        .create_match(&catalog, "acceptance-pairs", &forged)
        .await;
    assert_eq!(status, 400);
    assert_eq!(body, json!({"error":"unknown-profile","profile":"board"}));

    // The browser cannot author geometry at creation time; only saved versions resolve.
    let mut inline = saved.board.clone();
    inline["premiums"] = json!(vec!["triple_word"; 37]);
    let (status, _, _) = api
        .create_match(&catalog, "acceptance-pairs", &inline)
        .await;
    assert_eq!(status, 400);

    // Independent components still compose authoritatively: no silent substitution.
    let (status, body, _) = api
        .create_match(&catalog, "acceptance-unicode", &saved.board)
        .await;
    assert_eq!(status, 400);
    assert_eq!(body["error"], "incompatible-profiles");

    // A recipient may use a shared layout directly without copying it.
    let share = api.share(&saved).await;
    let (_, shared) = api.get(&format!("/shared-layouts/{share}")).await;
    let (status, _, _) = api
        .create_match(&catalog, "acceptance-pairs", &shared["board"])
        .await;
    assert_eq!(status, 200);
    // Existing default square creation is unchanged.
    let default_board = catalog["defaults"]["selection"]["board"].clone();
    let (status, _, _) = api
        .create_match(&catalog, "acceptance-legacy-words", &default_board)
        .await;
    assert_eq!(status, 200);
    api.stop().await;
}

/// Drives the real configured simulation reconstructed from the persisted match
/// initialization (the same path used by recovery): board claim, two named
/// players, board start, then native turns.
struct Match {
    simulation: ScrabbleSimulation,
    id: game_server::MatchId,
    sequences: BTreeMap<u32, u32>,
}
impl Match {
    fn open(root: &Path, match_id: &str, nonce: &str) -> Self {
        let factory = MatchFactory::new(&config(root)).unwrap();
        let id = game_server::MatchId::new(match_id.to_owned()).unwrap();
        let mut game = Self {
            simulation: factory.create(&id).unwrap(),
            id,
            sequences: BTreeMap::new(),
        };
        for player in 1..=3 {
            game.simulation.add_player(player).unwrap();
        }
        game.send(
            1,
            Command::ClaimBoard {
                request_id: nonce.into(),
            },
        )
        .unwrap();
        game.send(
            2,
            Command::SetName {
                display_name: "Ada".into(),
            },
        )
        .unwrap();
        game.send(
            3,
            Command::SetName {
                display_name: "Lin".into(),
            },
        )
        .unwrap();
        game.send(1, Command::Start {}).unwrap();
        game
    }
    fn view(&self, player: u32) -> PlayerSnapshot {
        serde_json::from_slice(&self.simulation.snapshot_for(player).unwrap().payload).unwrap()
    }
    fn turn(&self) -> (u32, u64) {
        match self.view(1).public.phase {
            Phase::Playing {
                active_player,
                turn,
            } => (active_player.get() as u32, turn.get()),
            other => panic!("match is not playing: {other:?}"),
        }
    }
    fn send(&mut self, player: u32, command: Command) -> Result<(), String> {
        let turn = match self.view(1).public.phase {
            Phase::Playing { turn, .. } => turn.get(),
            _ => 0,
        };
        let sequence = self.sequences.entry(player).or_insert(0);
        *sequence += 1;
        let payload = serde_json::to_vec(&CommandEnvelope {
            version: ProtocolVersion,
            game_id: factory::game_id(&self.id),
            player_id: PlayerId::new(u64::from(player)),
            sequence: *sequence,
            expected_turn: TurnId::new(turn),
            command,
        })
        .unwrap();
        self.simulation
            .apply_command(player, *sequence, &payload)
            .map_err(|error| format!("{error:?}"))
    }
    /// Places the player's first `cells.len()` lettered tiles; returns their values.
    fn commit(&mut self, player: u32, cells: &[(u8, u8)]) -> Result<Vec<u32>, String> {
        let view = self.view(player);
        let rules = view.public.configuration.clone();
        let letters: Vec<_> = view
            .own_rack
            .tiles
            .iter()
            .filter(|tile| matches!(tile.face, TileFace::Letter { .. }))
            .take(cells.len())
            .collect();
        assert_eq!(letters.len(), cells.len(), "rack has enough lettered tiles");
        let values = letters
            .iter()
            .map(|tile| {
                u32::from(
                    rules
                        .tiles
                        .iter()
                        .find(|definition| definition.face == tile.face)
                        .unwrap()
                        .value,
                )
            })
            .collect();
        let placements = letters
            .iter()
            .zip(cells)
            .map(|(tile, &(row, column))| Placement {
                tile_id: tile.id,
                coordinate: Coordinate::new(row, column).unwrap(),
                blank_as: None,
            })
            .collect();
        self.send(player, Command::Commit { placements })
            .map(|()| values)
    }
    fn score(&self, player: u32) -> i32 {
        self.view(1)
            .public
            .players
            .iter()
            .find(|row| row.id == PlayerId::new(u64::from(player)))
            .unwrap()
            .score
    }
}

#[tokio::test]
async fn custom_hex_layout_plays_center_opening_three_axes_and_its_premium_policy() {
    for (policy, all_word) in [("all_word", true), ("main_word_only", false)] {
        let root = tempfile::tempdir().unwrap();
        let api = Api::start(root.path()).await;
        let catalog = api.catalog().await;
        // Centre TW for the opening; DL where the second move forms two crossing words.
        let layout = hex(
            7,
            &[(3, 3, Premium::TripleWord), (2, 4, Premium::DoubleLetter)],
            policy,
        );
        let saved = api.save("Policy hex", layout.clone()).await;
        let (status, join, nonce) = api
            .create_match(&catalog, "acceptance-pairs", &saved.board)
            .await;
        assert_eq!(status, 200, "{join}");
        api.stop().await;
        let mut game = Match::open(root.path(), join["matchId"].as_str().unwrap(), &nonce);
        let rules = game.view(1).public.configuration;
        assert_eq!(rules.board_shape, BoardShape::Hexagon);
        assert_eq!(rules.board_size, 7);
        assert_eq!(
            serde_json::to_value(&rules.premiums).unwrap(),
            layout["premiums"]
        );
        assert_eq!(
            serde_json::to_value(rules.premium_policy).unwrap(),
            json!(policy)
        );
        assert!(
            !rules.contains(0, 0),
            "hex corners of the storage grid are off-board"
        );

        let (first, _) = game.turn();
        let second = if first == 2 { 3 } else { 2 };
        let before = game.view(1).public;
        assert!(
            game.commit(first, &[(0, 3), (0, 4)]).is_err(),
            "the opening must cover the centre hexagon (Decision 1.12)"
        );
        assert_eq!(game.view(1).public.board, before.board);
        let opening = game.commit(first, &[(3, 3), (3, 4)]).unwrap();
        assert_eq!(game.score(first), 3 * (opening[0] + opening[1]) as i32);
        // (2,4) touches (3,4) on the column axis and (3,3) on the diagonal axis:
        // two newly formed words, both validated and scored (Decisions 1.10/1.11).
        let crossing = game.commit(second, &[(2, 4)]).unwrap();
        let history = game.view(1).public.history;
        let words = match &history.last().unwrap().action {
            scrabble_game::protocol::TurnAction::Commit { words, .. } => words.clone(),
            other => panic!("unexpected turn {other:?}"),
        };
        assert_eq!(words.len(), 2, "both crossing words count: {words:?}");
        let v = crossing[0];
        let expected = if all_word {
            4 * v + opening[0] + opening[1]
        } else {
            // Equal lengths: only the deterministic main word gets the DL.
            3 * v + opening[0] + opening[1]
        };
        assert_eq!(
            game.score(second),
            expected as i32,
            "{policy} premium application"
        );
    }
}

#[tokio::test]
async fn custom_square_layout_plays_through_its_edited_premiums() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let catalog = api.catalog().await;
    let saved = api
        .save("Square v1", square(9, &[(4, 4, Premium::DoubleWord)]))
        .await;
    let v2 = square(
        9,
        &[(4, 4, Premium::TripleWord), (4, 5, Premium::TripleLetter)],
    );
    let (status, edited) = api.update(&saved, "Square v2", v2.clone()).await;
    assert_eq!(status, 200);
    let (status, join, nonce) = api
        .create_match(&catalog, "acceptance-pairs", &edited["board"])
        .await;
    assert_eq!(status, 200, "{join}");
    api.stop().await;
    let mut game = Match::open(root.path(), join["matchId"].as_str().unwrap(), &nonce);
    let rules = game.view(1).public.configuration;
    assert_eq!(rules.board_shape, BoardShape::Square);
    assert_eq!(rules.board_size, 9);
    assert_eq!(
        serde_json::to_value(&rules.premiums).unwrap(),
        v2["premiums"]
    );
    let (first, _) = game.turn();
    let values = game.commit(first, &[(4, 4), (4, 5)]).unwrap();
    assert_eq!(game.score(first), 3 * (values[0] + 3 * values[1]) as i32);
}

#[tokio::test]
async fn started_matches_keep_their_layout_version_after_edit_delete_and_restart() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let catalog = api.catalog().await;
    let v1 = hex(9, &[(4, 4, Premium::TripleWord)], "main_word_only");
    let saved = api.save("Pinned hex", v1.clone()).await;
    let kept = api.save("Kept square", square(7, &[])).await;
    let (status, join, nonce) = api
        .create_match(&catalog, "acceptance-pairs", &saved.board)
        .await;
    assert_eq!(status, 200, "{join}");
    let match_id = join["matchId"].as_str().unwrap().to_owned();

    let (status, _) = api
        .update(
            &saved,
            "Pinned hex v2",
            square(5, &[(2, 2, Premium::DoubleWord)]),
        )
        .await;
    assert_eq!(
        status, 200,
        "an owner may even change the geometry for future matches"
    );
    let (status, _) = api
        .post(
            &format!("/layouts/{}/delete", saved.id),
            json!({"version":1,"ownerCapability":saved.capability}),
        )
        .await;
    assert!(matches!(status, 200 | 204));
    let (status, body, _) = api
        .create_match(&catalog, "acceptance-pairs", &saved.board)
        .await;
    assert_eq!(
        status, 400,
        "deleted layouts are unavailable for new matches"
    );
    assert_eq!(body, json!({"error":"unknown-profile","profile":"board"}));
    api.stop().await;

    // Restart: the guest library is durable and the started match is unchanged.
    let api = Api::start(root.path()).await;
    let (status, body) = api.get(&format!("/games/{match_id}")).await;
    assert_eq!(status, 200, "{body}");
    let (status, still) = api.read(&kept).await;
    assert_eq!(status, 200, "saved originals survive a server restart");
    assert_eq!(still["name"], "Kept square");
    let (status, _) = api.read(&saved).await;
    assert_eq!(status, 404);
    api.stop().await;

    let mut game = Match::open(root.path(), &match_id, &nonce);
    let rules = game.view(1).public.configuration;
    assert_eq!(rules.board_shape, BoardShape::Hexagon);
    assert_eq!(rules.board_size, 9);
    assert_eq!(
        serde_json::to_value(&rules.premiums).unwrap(),
        v1["premiums"]
    );
    let (first, _) = game.turn();
    let values = game.commit(first, &[(4, 4), (4, 5)]).unwrap();
    assert_eq!(
        game.score(first),
        3 * (values[0] + values[1]) as i32,
        "the pinned v1 centre premium still scores after edit, delete and restart"
    );
}
