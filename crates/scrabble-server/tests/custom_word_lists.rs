//! Independent acceptance contract for #97, guest-owned slice: import, save,
//! reuse, share, copy, withdraw and manage custom word lists (Decisions 1.1,
//! 1.8, 1.16, 1.18, 1.20, 1.25, 1.45, 1.48, 1.50 and 1.52; ADR 0008/0009).
//! Authored before any
//! implementation by a separate acceptance agent; it exercises the real
//! production host (`hosting::serve`), its HTTP API, the configured match
//! factory and the native engine. Nothing here re-implements dictionary
//! validation, placement or scoring.
//!
//! Account linking (Decisions 1.20/1.46) is deferred by Decision 1.45 to #132
//! and deliberately not asserted here.
//!
//! # Seams this contract defines (the implementation must provide them)
//!
//! Storage: a new optional server configuration key `SCRABBLE_WORD_LIST_DIR`
//! names a durable, private library directory that survives restarts (default,
//! like `SCRABBLE_BOARD_LAYOUT_DIR`: `word-lists` next to the seed file). The
//! server stores no owner capability in clear text anywhere on disk (only a
//! verifier, e.g. a keyed hash). Started matches never read the library again:
//! their configured initialization manifest and content-addressed word store
//! (ADR 0009) pin the selected word set.
//!
//! Word-list text (`words`): exactly the existing dictionary format
//! (docs/specs/dictionary-validation.md): one uppercase alphabetic word per
//! line, any Unicode alphabet; blank lines ignored; LF or CRLF; duplicates and
//! order do not matter. Spaces, digits, punctuation and comments are rejected
//! with the 1-based line number. Lowercase letters are uppercased on import
//! when each maps to exactly one uppercase letter (Decision 1.52, e.g. `ä` ->
//! `Ä`); a line with a letter lacking such a mapping (`ß` -> `SS`, `ﬁ` -> `FI`)
//! is rejected as `invalid-word-list` naming that `line`.
//!
//! Limits (Decision 1.50) are operator-configurable with these optional keys
//! and defaults: `SCRABBLE_WORD_LIST_MAX_BYTES` (1 MiB = 1048576, per list),
//! `SCRABBLE_WORD_LIST_MAX_WORDS` (200000, per list),
//! `SCRABBLE_WORD_LIST_MAX_LISTS` (1024 originals and copies per server; when
//! full a new list or copy is 429 `{"error":"word-list-library-full"}`),
//! `SCRABBLE_WORD_LIST_MAX_VERSIONS` (8 retained versions per list; older
//! versions stop resolving for NEW matches) and
//! `SCRABBLE_WORD_LIST_MAX_STORED_BYTES` (256 MiB = 268435456 of stored words
//! per server). Versions bound to started matches stay governed by ADR 0009.
//!
//! HTTP endpoints on the existing game API origin (JSON, `version: 1`, 200 on
//! success; every mutation is a POST, matching the `/layouts` conventions):
//! - `GET /word-lists/limits` -> `{version,maxBytes,maxWords,maxLists,
//!   maxVersions,maxStoredBytes}`: the effective configured bounds.
//!   `maxBytes` counts UTF-8 bytes of `words`.
//! - `POST /word-lists` `{version,name,words}` -> `{version,wordListId,
//!   ownerCapability,name,revision:1,wordList,wordCount}`. `wordListId` =
//!   `w_<32 hex>`, `ownerCapability` = 64 lowercase hex, returned only to the
//!   creator. `wordList` is a `ContentIdentity` usable as `selection.wordList`.
//!   `wordCount` counts distinct words.
//! - `POST /word-lists/{id}/read` `{version,ownerCapability}` -> `{version,
//!   wordListId,name,revision,wordList,wordCount,words,shareId?}` where
//!   `words` holds the saved distinct words, one per line (any order).
//! - `POST /word-lists/{id}/update` `{version,ownerCapability,name,words}` ->
//!   the read shape with `revision + 1` and a NEW immutable `wordList`.
//! - `POST /word-lists/{id}/delete` `{version,ownerCapability}` -> 200 or 204;
//!   removes it from the library and ends its sharing.
//! - `POST /word-lists/{id}/share` `{version,ownerCapability}` -> `{version,
//!   shareId}`; `shareId` = `s_<32 hex>`, stable for repeated calls while
//!   shared. Sharing again after withdrawal issues a NEW share id; the
//!   withdrawn id stays dead (Decision 1.48).
//! - `POST /word-lists/{id}/unshare` `{version,ownerCapability}` -> 200 or
//!   204; the share link stops working (stop sharing, Decision 1.20).
//! - `GET /shared-word-lists/{shareId}` -> `{version,shareId,name,revision,
//!   wordList,wordCount}` of the original's CURRENT version; never
//!   `wordListId` or an owner capability.
//! - `POST /shared-word-lists/{shareId}/copy` `{version,name}` -> the
//!   `POST /word-lists` response shape for a NEW, independently owned list.
//! - `POST /games` (version 3): `selection.wordList` may be the `wordList`
//!   identity of a saved version or of a share view, while `selection.tile`
//!   and `selection.board` are chosen independently from the operator catalog
//!   (Decision 1.25). A guest list additionally carries its access proof in
//!   `selection.wordListAccess` (Decision 1.48), exactly one of
//!   `{"shareId":"s_.."}` (a LIVE share of that list) or
//!   `{"wordListId":"w_..","ownerCapability":".."}` (owner access). Catalog
//!   lists omit it. After withdrawal the identity alone, or the dead share id,
//!   no longer selects the list for a new match; owner access and copies do.
//!   This suite always sends a proof for guest lists, so it does not decide
//!   whether an identity without proof is accepted while still shared.
//!
//! Errors: 400 `{"error":"invalid-word-list","reason":..,"line"?:N}` for a
//! well-formed but invalid list or name (`line` is present whenever one line
//! is at fault); oversize content or too many words is 413
//! `{"error":"word-list-too-large",..}` or 400 `invalid-word-list` with a
//! reason (a framing-level 413 `body-too-large` is also acceptable for
//! oversize bodies); 403 `{"error":"word-list-owner-rejected"}` for a
//! missing/forged/foreign capability; 404 `{"error":"unknown-word-list"}` for
//! unknown or deleted lists; a withdrawn share answers 404 `unknown-word-list`
//! or 410 `word-list-share-withdrawn`. New matches reject deleted or forged
//! identities, and a missing, withdrawn, foreign or forged access proof, as
//! `{"error":"unknown-profile","profile":"wordList"}` and an incompatible
//! list/tile pair as the existing
//! `{"error":"incompatible-profiles","letters":[..]}`, never by substitution.
use game_server::{GameSimulation, SimulationError};
use scrabble_game::{
    identity::{Coordinate, PlayerId, ProtocolVersion, TurnId},
    protocol::{Command, CommandEnvelope, Phase, Placement, PlayerSnapshot},
    ruleset::TileFace,
};
use scrabble_server::{
    config::ServerConfig,
    factory::{self, MatchFactory},
    games::unix_seconds,
    simulation::ScrabbleSimulation,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::JoinHandle,
};

#[path = "support/hosted.rs"]
mod hosted;
#[path = "support/profile_catalog.rs"]
mod profile_catalog;

const WORD_LIST_DIR_KEY: &str = "SCRABBLE_WORD_LIST_DIR";
const UMLAUT_TILES: &str = "acceptance-umlaut-tiles";
/// Rejects every two-tile opening: no two-letter word exists in this list.
const REJECTING_WORDS: &str = "QUIZ\n";

/// Every ordered A-Z pair: any two-tile lettered opening is a valid word.
fn pairs() -> String {
    let mut text = String::new();
    for first in 'A'..='Z' {
        for second in 'A'..='Z' {
            text.push(first);
            text.push(second);
            text.push('\n');
        }
    }
    text
}

/// The shared authored catalog plus a tile profile that supports `Ö`, so a
/// separately identified non-English list has a compatible independent choice.
fn write_catalog(root: &Path) -> PathBuf {
    let catalog = root.join("profile-catalog.json");
    if catalog.exists() {
        return catalog;
    }
    profile_catalog::write_catalog(root, profile_catalog::LEGACY_WORDS).unwrap();
    let mut document: Value = serde_json::from_slice(&std::fs::read(&catalog).unwrap()).unwrap();
    let mut tiles = document["tileProfiles"][0]["tiles"].clone();
    let mut umlaut = tiles
        .as_array()
        .unwrap()
        .iter()
        .find(|tile| tile["face"]["letter"] == "O")
        .expect("legacy tiles include O")
        .clone();
    umlaut["face"]["letter"] = json!("Ö");
    umlaut["count"] = json!(2);
    tiles.as_array_mut().unwrap().push(umlaut);
    document["tileProfiles"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "identity":{"name":UMLAUT_TILES,"revision":"1"},
            "label":"Umlaut test tiles","tiles":tiles
        }));
    std::fs::write(&catalog, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    catalog
}

fn values(root: &Path, library: bool) -> BTreeMap<String, String> {
    let catalog = write_catalog(root);
    let path = |name: &str| root.join(name).to_str().unwrap().to_owned();
    let mut values = BTreeMap::from([
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
    ]);
    if library {
        values.insert(WORD_LIST_DIR_KEY.into(), path("word-lists"));
        // The key is optional and defaults to `word-lists` next to the seed file
        // (the same directory). A pre-#97 server rejects the unknown key; drop it
        // there so red tests fail on the missing HTTP routes, not on startup.
        if ServerConfig::from_values(&values).is_err_and(|error| {
            error.to_string() == format!("unknown server configuration key {WORD_LIST_DIR_KEY}")
        }) {
            values.remove(WORD_LIST_DIR_KEY);
        }
    }
    values
}
fn config(root: &Path) -> ServerConfig {
    ServerConfig::from_values(&values(root, true)).expect("valid acceptance configuration")
}

/// The production server (`hosting::serve`: recovery-prepared live host, HTTP
/// API, WebTransport and status) on free local ports. `stop` is a graceful
/// shutdown that persists recovery images, so `start` on the same directory is
/// a real restart.
struct Api {
    port: u16,
    task: Option<JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>>,
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
        Self::start_with(root, true, &[]).await
    }
    /// Starts with operator overrides (Decision 1.50 limit keys).
    async fn start_configured(root: &Path, overrides: &[(&str, &str)]) -> Self {
        Self::start_with(root, true, overrides).await
    }
    async fn start_with(root: &Path, library: bool, overrides: &[(&str, &str)]) -> Self {
        let certificate = root.join("cert.pem");
        let key = root.join("key.pem");
        if !certificate.exists() {
            let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
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
        }
        let mut values = values(root, library);
        for (name, value) in [
            ("SCRABBLE_MAX_MATCHES", "12".to_owned()),
            ("SCRABBLE_CERT_PEM", certificate.to_str().unwrap().into()),
            ("SCRABBLE_KEY_PEM", key.to_str().unwrap().into()),
        ] {
            values.insert(name.into(), value);
        }
        for (name, value) in overrides {
            values.insert((*name).into(), (*value).into());
        }
        let hosted = hosted::start(&values).await;
        Self {
            port: hosted.api_port,
            task: Some(hosted.task),
            shutdown: hosted.shutdown,
        }
    }
    /// Graceful shutdown; recovery images are written before this returns.
    async fn stop(mut self) {
        let _ = self.shutdown.send(()).await;
        let task = self.task.take().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(30), task)
            .await
            .expect("graceful shutdown")
            .unwrap()
            .expect("server shut down cleanly");
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
        let (header, body) = response
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("no HTTP response for {method} {path}"));
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
    async fn save(&self, name: &str, words: &str) -> Saved {
        let (status, body) = self
            .post(
                "/word-lists",
                json!({"version":1,"name":name,"words":words}),
            )
            .await;
        assert_eq!(status, 200, "a valid guest word list must save: {body}");
        Saved::parse(&body)
    }
    async fn owner(&self, saved: &Saved, action: &str) -> (u16, Value) {
        self.post(
            &format!("/word-lists/{}/{action}", saved.id),
            json!({"version":1,"ownerCapability":saved.capability}),
        )
        .await
    }
    async fn read(&self, saved: &Saved) -> (u16, Value) {
        self.owner(saved, "read").await
    }
    async fn update(&self, saved: &Saved, name: &str, words: &str) -> (u16, Value) {
        self.post(
            &format!("/word-lists/{}/update", saved.id),
            json!({"version":1,"ownerCapability":saved.capability,"name":name,"words":words}),
        )
        .await
    }
    async fn share(&self, saved: &Saved) -> String {
        let (status, body) = self.owner(saved, "share").await;
        assert_eq!(status, 200, "the owner can share its original: {body}");
        let share = body["shareId"].as_str().unwrap().to_owned();
        assert!(is_id(&share, "s_"), "share id format: {share}");
        share
    }
    async fn copy(&self, share: &str, name: &str) -> (u16, Value) {
        self.post(
            &format!("/shared-word-lists/{share}/copy"),
            json!({"version":1,"name":name}),
        )
        .await
    }
    async fn limits_body(&self) -> Value {
        let (status, body) = self.get("/word-lists/limits").await;
        assert_eq!(status, 200, "configured bounds are published: {body}");
        assert_eq!(body["version"], 1);
        body
    }
    async fn limits(&self) -> (usize, usize) {
        let body = self.limits_body().await;
        let bytes = body["maxBytes"].as_u64().expect("maxBytes") as usize;
        let words = body["maxWords"].as_u64().expect("maxWords") as usize;
        (bytes, words)
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
        word_list: &Value,
        access: Option<Value>,
        tile: Option<&str>,
    ) -> (u16, Value, String) {
        let nonce = format!("{:032x}", rand_nonce());
        let mut selection = catalog["defaults"]["selection"].clone();
        selection["wordList"] = word_list.clone();
        if let Some(access) = access {
            selection["wordListAccess"] = access;
        }
        if let Some(tile) = tile {
            selection["tile"] = identity_of(catalog, "tileProfiles", tile);
        }
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

/// `selection.wordListAccess` proving owner access (Decision 1.48).
fn owner_access(saved: &Saved) -> Option<Value> {
    Some(json!({"wordListId":saved.id,"ownerCapability":saved.capability}))
}
/// `selection.wordListAccess` through a live share (Decision 1.48).
fn share_access(share: &str) -> Option<Value> {
    Some(json!({"shareId":share}))
}
fn unknown_word_list() -> Value {
    json!({"error":"unknown-profile","profile":"wordList"})
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
fn word_set(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}
fn withdrawn(status: u16, body: &Value) -> bool {
    matches!(
        (status, body["error"].as_str()),
        (404, Some("unknown-word-list")) | (410, Some("word-list-share-withdrawn"))
    )
}

#[derive(Clone, Debug)]
struct Saved {
    id: String,
    capability: String,
    revision: u64,
    word_list: Value,
}
impl Saved {
    fn parse(body: &Value) -> Self {
        let id = body["wordListId"].as_str().expect("wordListId").to_owned();
        let capability = body["ownerCapability"]
            .as_str()
            .expect("ownerCapability is returned to the creator")
            .to_owned();
        assert!(is_id(&id, "w_"), "word list id format: {id}");
        assert!(is_capability(&capability), "owner capability format");
        assert_ne!(id, capability);
        let word_list = body["wordList"].clone();
        assert!(
            word_list["name"].as_str().is_some_and(|s| !s.is_empty())
                && word_list["revision"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty()),
            "each saved version exposes a ContentIdentity usable as selection.wordList: {body}"
        );
        assert!(body["wordCount"].as_u64().is_some(), "wordCount: {body}");
        Self {
            id,
            capability,
            revision: body["revision"].as_u64().expect("revision"),
            word_list,
        }
    }
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
async fn guest_saves_word_lists_owned_only_by_a_private_capability_and_durable_across_restart() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let english = api.save("English pairs", &pairs()).await;
    // A separately identified list for another game language (Decision 1.1).
    let german = api.save("Deutsche Wörter", "ÖL\nÄRGER\nSTRAẞE\n").await;
    assert_eq!((english.revision, german.revision), (1, 1));
    assert_ne!(english.id, german.id);
    assert_ne!(english.capability, german.capability);
    assert_ne!(english.word_list, german.word_list);

    for (saved, name, words, count) in [
        (&english, "English pairs", pairs(), 676),
        (&german, "Deutsche Wörter", "ÖL\nÄRGER\nSTRAẞE\n".into(), 3),
    ] {
        let (status, body) = api.read(saved).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["name"], name);
        assert_eq!(body["revision"], 1);
        assert_eq!(body["wordList"], saved.word_list);
        assert_eq!(body["wordCount"], count);
        assert_eq!(
            word_set(body["words"].as_str().expect("owner read returns words")),
            word_set(&words),
            "saved words round-trip"
        );
        assert!(body.get("ownerCapability").is_none());
    }
    // Duplicates and CRLF normalize exactly like deployment dictionaries.
    let crlf = api.save("CRLF list", "DOG\r\nCAT\r\nCAT\r\n").await;
    let (_, body) = api.read(&crlf).await;
    assert_eq!(body["wordCount"], 2);

    // Saved guest lists are private library entries, not public catalog rows.
    for path in ["/profiles/hex", "/profiles"] {
        let (_, catalog) = api.get(path).await;
        let public = catalog.to_string();
        for saved in [&english, &german] {
            assert!(!public.contains(&saved.capability));
            assert!(!public.contains(&saved.id));
            assert!(!public.contains(saved.word_list["name"].as_str().unwrap()));
        }
        assert!(!public.contains("English pairs") && !public.contains("Deutsche"));
    }
    // The server keeps a verifier, never the bearer capability itself.
    for saved in [&english, &german] {
        assert_eq!(
            files_containing(root.path(), &saved.capability),
            Vec::<PathBuf>::new(),
            "owner capabilities must not be persisted in clear text"
        );
    }
    api.stop().await;

    // A later session: the library survives a graceful server restart.
    let api = Api::start(root.path()).await;
    for saved in [&english, &german] {
        let (status, body) = api.read(saved).await;
        assert_eq!(status, 200, "saved lists survive a restart: {body}");
        assert_eq!(body["wordList"], saved.word_list);
    }
    api.stop().await;
}

#[tokio::test]
async fn invalid_or_oversized_uploads_get_precise_feedback_and_save_nothing() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let (max_bytes, max_words) = api.limits().await;
    let directory = root.path().join("word-lists");
    let before = file_count(&directory);

    for (case, words, line) in [
        ("digit", "CAT\nD0G\nTA\n", 2),
        ("space inside a line", "CAT\nTWO WORDS\n", 2),
        ("punctuation", "CAT\nDOG\n\nHELLO!\n", 4),
        ("comment", "# my list\nCAT\n", 1),
        ("replacement character", "CAT\n\u{FFFD}\n", 2),
        ("surrounding space", "CAT\n DOG\n", 2),
    ] {
        let (status, body) = api
            .post(
                "/word-lists",
                json!({"version":1,"name":case,"words":words}),
            )
            .await;
        assert_eq!(status, 400, "{case} must be rejected: {body}");
        assert_eq!(body["error"], "invalid-word-list", "{case}: {body}");
        assert_eq!(
            body["line"], line,
            "{case} names the offending line: {body}"
        );
        assert!(
            body["reason"].as_str().is_some_and(|r| !r.is_empty()),
            "{case} explains: {body}"
        );
        assert!(body.get("wordListId").is_none() && body.get("ownerCapability").is_none());
    }
    for (case, name, words) in [
        ("empty list", "Empty", ""),
        ("only blank lines", "Blank", "\n\n\r\n"),
        ("empty name", "", "CAT\n"),
        ("whitespace name", "   ", "CAT\n"),
    ] {
        let (status, body) = api
            .post(
                "/word-lists",
                json!({"version":1,"name":name,"words":words}),
            )
            .await;
        assert_eq!(status, 400, "{case} must be rejected: {body}");
        assert_eq!(body["error"], "invalid-word-list", "{case}: {body}");
        assert!(body["reason"].as_str().is_some_and(|r| !r.is_empty()));
    }
    for (case, body) in [
        ("missing words", json!({"version":1,"name":"No words"})),
        (
            "words is not text",
            json!({"version":1,"name":"Array","words":["CAT"]}),
        ),
        (
            "unsupported version",
            json!({"version":2,"name":"Future","words":"CAT\n"}),
        ),
    ] {
        let (status, response) = api.post("/word-lists", body).await;
        assert_eq!(status, 400, "{case}: {response}");
    }
    let (status, _) = api.raw("POST", "/word-lists", "{not json").await;
    assert_eq!(status, 400);

    // Oversize content: one byte above the published bound.
    let mut oversize = String::with_capacity(max_bytes + 8);
    while oversize.len() <= max_bytes {
        oversize.push_str("ABCDEFG\n");
    }
    let (status, body) = api
        .post(
            "/word-lists",
            json!({"version":1,"name":"Too large","words":oversize}),
        )
        .await;
    assert!(
        matches!(
            (status, body["error"].as_str()),
            (413, Some("word-list-too-large" | "body-too-large"))
                | (400, Some("invalid-word-list"))
        ),
        "oversize uploads are rejected: {status} {body}"
    );
    drop(oversize);

    // Too many distinct words while still within the byte bound.
    let mut length = 1;
    while 26_usize.pow(length) <= max_words {
        length += 1;
    }
    let too_many = max_words + 1;
    assert!(
        too_many * (length as usize + 1) <= max_bytes,
        "contract: the word bound ({max_words}) must be reachable within maxBytes ({max_bytes})"
    );
    let mut words = String::with_capacity(too_many * (length as usize + 1));
    for index in 0..too_many {
        let mut value = index;
        for _ in 0..length {
            words.push(char::from(b'A' + (value % 26) as u8));
            value /= 26;
        }
        words.push('\n');
    }
    let (status, body) = api
        .post(
            "/word-lists",
            json!({"version":1,"name":"Too many","words":words}),
        )
        .await;
    assert!(
        matches!(
            (status, body["error"].as_str()),
            (413, Some("word-list-too-large")) | (400, Some("invalid-word-list"))
        ),
        "too many words are rejected: {status} {body}"
    );
    assert!(body["reason"].as_str().is_some_and(|r| !r.is_empty()));
    assert_eq!(
        file_count(&directory),
        before,
        "rejected submissions save nothing, not even partial lists"
    );

    // An invalid edit is rejected without replacing the saved version.
    let saved = api.save("Valid after rejections", "CAT\nTA\n").await;
    let (status, body) = api.update(&saved, "Broken", "CAT\nD0G\n").await;
    assert_eq!(
        (status, body["error"].as_str(), body["line"].as_u64()),
        (400, Some("invalid-word-list"), Some(2)),
        "{body}"
    );
    let (_, still) = api.read(&saved).await;
    assert_eq!(still["revision"], 1);
    assert_eq!(still["name"], "Valid after rejections");
    assert_eq!(
        word_set(still["words"].as_str().unwrap()),
        word_set("CAT\nTA\n")
    );
    api.stop().await;
}

/// `count` distinct uppercase words of `length` letters, one per line.
fn distinct_words(count: usize, length: u32, offset: usize) -> String {
    let mut text = String::new();
    for index in offset..offset + count {
        let mut value = index;
        for _ in 0..length {
            text.push(char::from(b'A' + (value % 26) as u8));
            value /= 26;
        }
        text.push('\n');
    }
    text
}
fn full_or_too_large(status: u16, body: &Value) -> bool {
    matches!(
        (status, body["error"].as_str()),
        (429, Some("word-list-library-full")) | (413, Some("word-list-too-large"))
    )
}
fn per_list_rejection(status: u16, body: &Value) -> bool {
    matches!(
        (status, body["error"].as_str()),
        (413, Some("word-list-too-large" | "body-too-large")) | (400, Some("invalid-word-list"))
    )
}

#[tokio::test]
async fn lowercase_is_uppercased_only_where_the_mapping_is_one_to_one() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let catalog = api.catalog().await;
    // Decision 1.52: one-to-one lowercase letters are uppercased on import.
    let mixed = api.save("Mixed case", "cat\nTa\närger\nÖl\n").await;
    let (_, body) = api.read(&mixed).await;
    assert_eq!(body["wordCount"], 4, "{body}");
    assert_eq!(
        word_set(body["words"].as_str().unwrap()),
        word_set("CAT\nTA\nÄRGER\nÖL"),
        "saved words are the uppercased forms"
    );
    let (status, edited) = api.update(&mixed, "Mixed case", "dog\nDOG\n").await;
    assert_eq!(status, 200, "{edited}");
    assert_eq!(edited["wordCount"], 1, "an update normalizes case too");

    // A letter without a one-to-one uppercase mapping rejects its line.
    for (case, words, line) in [
        ("sharp s uppercases to SS", "CAT\nstraße\n", 2),
        ("ligature uppercases to FI", "ﬁsh\nCAT\n", 1),
        ("capital words after it", "CAT\nDOG\nMaß\n", 3),
    ] {
        let (status, body) = api
            .post(
                "/word-lists",
                json!({"version":1,"name":case,"words":words}),
            )
            .await;
        assert_eq!(status, 400, "{case}: {body}");
        assert_eq!(body["error"], "invalid-word-list", "{case}: {body}");
        assert_eq!(body["line"], line, "{case} names the line: {body}");
        let (status, body) = api.update(&mixed, case, words).await;
        assert_eq!(
            (status, body["error"].as_str(), body["line"].as_u64()),
            (400, Some("invalid-word-list"), Some(line as u64)),
            "{case} on update: {body}"
        );
    }
    let (_, still) = api.read(&mixed).await;
    assert_eq!(
        still["revision"], 2,
        "rejected edits keep the saved version"
    );

    // The normalized words are what native matches validate against.
    let lower = api.save("Lowercase pairs", &pairs().to_lowercase()).await;
    let (status, created, nonce) = api
        .create_match(&catalog, &lower.word_list, owner_access(&lower), None)
        .await;
    assert_eq!(status, 200, "{created}");
    api.stop().await;
    let mut game = Match::open(root.path(), created["matchId"].as_str().unwrap(), &nonce);
    assert_accepted(&mut game, "an uppercase opening against a lowercase upload");
}

#[tokio::test]
async fn default_limits_are_published() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let body = api.limits_body().await;
    // Decision 1.50 defaults.
    for (field, value) in [
        ("maxBytes", 1024 * 1024),
        ("maxWords", 200_000),
        ("maxLists", 1024),
        ("maxVersions", 8),
        ("maxStoredBytes", 256 * 1024 * 1024),
    ] {
        assert_eq!(body[field], value, "default {field}: {body}");
    }
    api.stop().await;
}

#[tokio::test]
async fn operator_overrides_bound_words_bytes_and_lists() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start_configured(
        root.path(),
        &[
            ("SCRABBLE_WORD_LIST_MAX_BYTES", "64"),
            ("SCRABBLE_WORD_LIST_MAX_WORDS", "3"),
            ("SCRABBLE_WORD_LIST_MAX_LISTS", "3"),
        ],
    )
    .await;
    let body = api.limits_body().await;
    assert_eq!(
        (&body["maxBytes"], &body["maxWords"], &body["maxLists"]),
        (&json!(64), &json!(3), &json!(3)),
        "operator overrides are published: {body}"
    );
    assert_eq!(body["maxVersions"], 8, "unset keys keep their defaults");
    let directory = root.path().join("word-lists");
    let first = api.save("Three words", "CAT\nTA\nAT\n").await;
    let before = file_count(&directory);
    let (status, body) = api
        .post(
            "/word-lists",
            json!({"version":1,"name":"Four words","words":"CAT\nTA\nAT\nACT\n"}),
        )
        .await;
    assert!(
        per_list_rejection(status, &body),
        "word override: {status} {body}"
    );
    assert!(body["reason"].as_str().is_some_and(|r| !r.is_empty()));
    let long = format!("{}\n{}\n", "A".repeat(40), "B".repeat(40));
    let (status, body) = api
        .post(
            "/word-lists",
            json!({"version":1,"name":"Long words","words":long}),
        )
        .await;
    assert!(
        per_list_rejection(status, &body),
        "byte override: {status} {body}"
    );
    let (status, body) = api.update(&first, "Four", "CAT\nTA\nAT\nACT\n").await;
    assert!(
        per_list_rejection(status, &body),
        "edits are bounded: {status} {body}"
    );
    assert_eq!(file_count(&directory), before, "rejections store nothing");

    // Server-wide list bound, counting originals and copies.
    let share = api.share(&first).await;
    api.save("Second", "DOG\n").await;
    let third = api.save("Third", "EEL\n").await;
    let (status, body) = api
        .post(
            "/word-lists",
            json!({"version":1,"name":"Fourth","words":"FOX\n"}),
        )
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (429, Some("word-list-library-full")),
        "{body}"
    );
    let (status, body) = api.copy(&share, "Copy when full").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (429, Some("word-list-library-full")),
        "{body}"
    );
    let (status, _) = api.owner(&third, "delete").await;
    assert!(matches!(status, 200 | 204));
    api.save("Fourth after delete", "FOX\n").await;
    api.stop().await;

    // Server-wide stored-word bound.
    let root = tempfile::tempdir().unwrap();
    let api = Api::start_configured(
        root.path(),
        &[("SCRABBLE_WORD_LIST_MAX_STORED_BYTES", "1024")],
    )
    .await;
    assert_eq!(api.limits_body().await["maxStoredBytes"], 1024);
    let large = api
        .save("Seven hundred bytes", &distinct_words(140, 4, 0))
        .await;
    let (status, body) = api
        .post(
            "/word-lists",
            json!({"version":1,"name":"Over budget","words":distinct_words(140, 4, 140)}),
        )
        .await;
    assert!(
        full_or_too_large(status, &body),
        "stored-byte bound: {status} {body}"
    );
    let (status, _) = api.owner(&large, "delete").await;
    assert!(matches!(status, 200 | 204));
    api.save("Within budget again", &distinct_words(140, 4, 140))
        .await;
    api.stop().await;
}

#[tokio::test]
async fn operator_version_retention_bounds_new_matches_but_never_started_ones() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start_configured(root.path(), &[("SCRABBLE_WORD_LIST_MAX_VERSIONS", "2")]).await;
    assert_eq!(api.limits_body().await["maxVersions"], 2);
    let catalog = api.catalog().await;
    let saved = api.save("Retained", &pairs()).await;
    let (status, pinned, nonce) = api
        .create_match(&catalog, &saved.word_list, owner_access(&saved), None)
        .await;
    assert_eq!(status, 200, "{pinned}");
    let (_, v2) = api.update(&saved, "Retained", REJECTING_WORDS).await;
    let (_, v3) = api.update(&saved, "Retained", "QUIZ\nZAX\n").await;
    assert_eq!(v3["revision"], 3);
    let (status, body, _) = api
        .create_match(&catalog, &saved.word_list, owner_access(&saved), None)
        .await;
    assert_eq!(
        (status, body),
        (400, unknown_word_list()),
        "a version beyond retention no longer resolves for new matches"
    );
    for retained in [&v2["wordList"], &v3["wordList"]] {
        let (status, body, _) = api
            .create_match(&catalog, retained, owner_access(&saved), None)
            .await;
        assert_eq!(status, 200, "retained versions stay selectable: {body}");
    }
    api.stop().await;
    let mut game = Match::open(root.path(), pinned["matchId"].as_str().unwrap(), &nonce);
    assert_accepted(&mut game, "the match started on the dropped v1");
}

#[tokio::test]
async fn only_the_private_capability_reads_edits_shares_unshares_or_deletes_an_original() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let original = api.save("Owned", "CAT\nTA\n").await;
    let other = api.save("Other guest", "DOG\n").await;
    let share = api.share(&original).await;
    let forged = "0".repeat(64);
    for capability in [
        Value::Null,
        json!(""),
        json!(forged),
        json!(other.capability),
        json!(share),
        json!(original.id),
    ] {
        for action in ["read", "update", "share", "unshare", "delete"] {
            let mut body = json!({"version":1,"ownerCapability":capability});
            if action == "update" {
                body["name"] = json!("Hijacked");
                body["words"] = json!("HIJACK\n");
            }
            let (status, response) = api
                .post(&format!("/word-lists/{}/{action}", original.id), body)
                .await;
            assert!(
                matches!(status, 400 | 403),
                "{action} with a non-owner capability must fail: {status} {response}"
            );
            if status == 403 {
                assert_eq!(response["error"], "word-list-owner-rejected");
            }
            assert!(!response.to_string().contains(&original.capability));
        }
    }
    let (status, unchanged) = api.read(&original).await;
    assert_eq!(status, 200);
    assert_eq!(unchanged["name"], "Owned");
    assert_eq!(unchanged["revision"], 1);
    assert_eq!(unchanged["wordList"], original.word_list);
    assert_eq!(
        unchanged["shareId"],
        share.as_str(),
        "sharing was not withdrawn"
    );
    let (status, _) = api.get(&format!("/shared-word-lists/{share}")).await;
    assert_eq!(status, 200, "a forged unshare left the link working");

    let (status, edited) = api.update(&original, "Owned v2", "CAT\nTA\nAT\n").await;
    assert_eq!(status, 200, "{edited}");
    assert_eq!(edited["revision"], 2);
    assert_eq!(edited["wordListId"], original.id.as_str());
    assert_eq!(edited["wordCount"], 3);
    assert_ne!(
        edited["wordList"], original.word_list,
        "an edit is a new immutable version"
    );
    let (_, current) = api.read(&original).await;
    assert_eq!(current["name"], "Owned v2");
    assert_eq!(
        word_set(current["words"].as_str().unwrap()),
        word_set("CAT\nTA\nAT")
    );

    let (status, _) = api.owner(&original, "delete").await;
    assert!(matches!(status, 200 | 204));
    let (status, body) = api.read(&original).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (404, Some("unknown-word-list"))
    );
    let (status, body) = api.get(&format!("/shared-word-lists/{share}")).await;
    assert!(
        withdrawn(status, &body),
        "deleting ends sharing: {status} {body}"
    );
    let (status, _) = api.read(&other).await;
    assert_eq!(
        status, 200,
        "deleting one guest's original leaves others intact"
    );
    // Unknown and malformed ids never become owner access.
    for id in [
        format!("w_{}", "f".repeat(32)),
        "w_short".into(),
        "../x".into(),
    ] {
        let (status, _) = api
            .post(
                &format!("/word-lists/{id}/read"),
                json!({"version":1,"ownerCapability":other.capability}),
            )
            .await;
        assert!(matches!(status, 400 | 403 | 404), "{id}: {status}");
    }
    api.stop().await;
}

#[tokio::test]
async fn share_links_grant_use_and_copy_but_never_management_and_copies_are_independent() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let original = api.save("Shared list", "CAT\nTA\n").await;
    let share = api.share(&original).await;
    assert_eq!(api.share(&original).await, share, "sharing is idempotent");
    assert_ne!(share, original.id);

    let (status, shared) = api.get(&format!("/shared-word-lists/{share}")).await;
    assert_eq!(status, 200, "{shared}");
    assert_eq!(shared["name"], "Shared list");
    assert_eq!(shared["shareId"], share.as_str());
    assert_eq!(shared["revision"], 1);
    assert_eq!(shared["wordCount"], 2);
    assert_eq!(shared["wordList"], original.word_list);
    assert!(shared.get("ownerCapability").is_none());
    assert!(
        shared.get("wordListId").is_none(),
        "share never names the original"
    );
    let text = shared.to_string();
    assert!(!text.contains(&original.capability) && !text.contains(&original.id));
    let (status, _) = api
        .get(&format!("/shared-word-lists/s_{}", "f".repeat(32)))
        .await;
    assert_eq!(status, 404);
    // No management action exists on the public share resource.
    for action in ["update", "delete", "unshare", "share", "read"] {
        let (status, body) = api
            .post(
                &format!("/shared-word-lists/{share}/{action}"),
                json!({"version":1,"name":"x","words":"HIJACK\n","ownerCapability":share}),
            )
            .await;
        assert!(
            matches!(status, 400 | 403 | 404 | 405),
            "{action} via share link: {status} {body}"
        );
    }
    let (status, _) = api
        .raw("DELETE", &format!("/shared-word-lists/{share}"), "")
        .await;
    assert!(matches!(status, 400 | 403 | 404 | 405));

    let (status, copied) = api.copy(&share, "My copy").await;
    assert_eq!(status, 200, "{copied}");
    let copy = Saved::parse(&copied);
    assert_ne!(copy.id, original.id);
    assert_ne!(copy.capability, original.capability);
    assert_eq!(copy.revision, 1);
    assert_eq!(copied["name"], "My copy");
    let (_, copy_read) = api.read(&copy).await;
    assert_eq!(
        word_set(copy_read["words"].as_str().unwrap()),
        word_set("CAT\nTA")
    );
    assert!(
        copy_read.get("shareId").is_none(),
        "a copy is not shared by default"
    );
    // Cross-capabilities never manage the other original.
    let (status, _) = api
        .update(
            &Saved {
                capability: copy.capability.clone(),
                ..original.clone()
            },
            "copy owner hijack",
            "HIJACK\n",
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
            "HIJACK\n",
        )
        .await;
    assert_eq!(
        status, 403,
        "the source owner cannot manage a recipient's copy"
    );

    // Editing the copy leaves the source and its share untouched.
    let (status, edited_copy) = api.update(&copy, "My edited copy", REJECTING_WORDS).await;
    assert_eq!(status, 200, "{edited_copy}");
    let (_, source_now) = api.read(&original).await;
    assert_eq!(source_now["revision"], 1);
    assert_eq!(source_now["name"], "Shared list");
    assert_eq!(
        word_set(source_now["words"].as_str().unwrap()),
        word_set("CAT\nTA")
    );
    let (_, shared_now) = api.get(&format!("/shared-word-lists/{share}")).await;
    assert_eq!(shared_now["wordList"], original.word_list);

    // Editing the source changes what the share shows, never the saved copy.
    let (status, source_v2) = api
        .update(&original, "Shared list v2", "CAT\nTA\nAT\n")
        .await;
    assert_eq!(status, 200);
    let (_, shared_v2) = api.get(&format!("/shared-word-lists/{share}")).await;
    assert_eq!(shared_v2["revision"], 2);
    assert_eq!(shared_v2["wordCount"], 3);
    assert_eq!(shared_v2["wordList"], source_v2["wordList"]);
    let (_, copy_now) = api.read(&copy).await;
    assert_eq!(
        word_set(copy_now["words"].as_str().unwrap()),
        word_set(REJECTING_WORDS)
    );
    // A second recipient's copy is independent of the first one.
    let (_, second) = api.copy(&share, "Second").await;
    let second = Saved::parse(&second);
    assert!(second.id != copy.id && second.capability != copy.capability);
    let (_, second_now) = api.read(&second).await;
    assert_eq!(second_now["wordCount"], 3);
    let (_, first_copy) = api.read(&copy).await;
    assert_eq!(first_copy["wordCount"], 1);
    for saved in [&original, &copy, &second] {
        assert!(files_containing(root.path(), &saved.capability).is_empty());
    }
    api.stop().await;
}

#[tokio::test]
async fn withdrawn_share_fails_visibly_while_copies_and_owner_use_continue() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let catalog = api.catalog().await;
    let original = api.save("Withdrawn", &pairs()).await;
    let share = api.share(&original).await;
    let (status, copied) = api.copy(&share, "Kept copy").await;
    assert_eq!(status, 200);
    let copy = Saved::parse(&copied);
    let (_, before) = api.read(&copy).await;
    // A recipient learns the shared identity and starts a match through the share.
    let (_, shared) = api.get(&format!("/shared-word-lists/{share}")).await;
    let shared_identity = shared["wordList"].clone();
    let (status, running, running_nonce) = api
        .create_match(&catalog, &shared_identity, share_access(&share), None)
        .await;
    assert_eq!(status, 200, "a live share selects the list: {running}");
    let running_id = running["matchId"].as_str().unwrap().to_owned();

    let (status, _) = api.owner(&original, "unshare").await;
    assert!(matches!(status, 200 | 204), "the owner can stop sharing");
    let (status, body) = api.get(&format!("/shared-word-lists/{share}")).await;
    assert!(
        withdrawn(status, &body),
        "withdrawn link fails visibly: {status} {body}"
    );
    assert!(!body.to_string().contains(&original.capability));
    let (status, body) = api.copy(&share, "Late copy").await;
    assert!(
        withdrawn(status, &body),
        "no new copies after withdrawal: {status} {body}"
    );
    let (_, owner_view) = api.read(&original).await;
    assert!(owner_view.get("shareId").is_none(), "{owner_view}");
    assert_eq!(
        owner_view["revision"], 1,
        "withdrawal does not edit the list"
    );

    // Decision 1.48: the previously obtained identity alone, or with the dead
    // share id, or with a foreign proof, no longer selects the list.
    let configurations = root.path().join("configurations");
    let created = file_count(&configurations);
    for (case, access) in [
        ("identity alone", None),
        ("withdrawn share id", share_access(&share)),
        (
            "forged owner capability",
            Some(json!({"wordListId":original.id,"ownerCapability":"0".repeat(64)})),
        ),
        (
            "share id as capability",
            Some(json!({"wordListId":original.id,"ownerCapability":share})),
        ),
    ] {
        let (status, body, _) = api
            .create_match(&catalog, &shared_identity, access, None)
            .await;
        assert_eq!(
            (status, &body),
            (400, &unknown_word_list()),
            "{case} must not select a withdrawn list for a new match"
        );
    }
    assert_eq!(file_count(&configurations), created, "no match was created");

    // The owner still uses its own unshared list for new matches.
    let (status, body, _) = api
        .create_match(
            &catalog,
            &owner_view["wordList"],
            owner_access(&original),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    // Existing copies keep working, unchanged and usable for new matches.
    let (status, after) = api.read(&copy).await;
    assert_eq!(status, 200);
    assert_eq!(after, before);
    let (status, body, _) = api
        .create_match(&catalog, &copy.word_list, owner_access(&copy), None)
        .await;
    assert_eq!(status, 200, "{body}");

    // Sharing again issues a NEW id; the withdrawn link stays dead.
    let reshared = api.share(&original).await;
    assert_ne!(reshared, share, "re-sharing never revives the withdrawn id");
    let (status, body) = api.get(&format!("/shared-word-lists/{share}")).await;
    assert!(
        withdrawn(status, &body),
        "old link stays dead: {status} {body}"
    );
    let (status, body) = api.copy(&share, "Still late").await;
    assert!(withdrawn(status, &body), "{status} {body}");
    let (status, live) = api.get(&format!("/shared-word-lists/{reshared}")).await;
    assert_eq!(status, 200, "{live}");
    let (status, body, _) = api
        .create_match(&catalog, &live["wordList"], share_access(&reshared), None)
        .await;
    assert_eq!(status, 200, "the new live share selects the list: {body}");
    let (status, body, _) = api
        .create_match(&catalog, &live["wordList"], share_access(&share), None)
        .await;
    assert_eq!((status, body), (400, unknown_word_list()));
    // A live share of ANOTHER list never proves access to this one.
    let other = api.save("Other shared", "CAT\n").await;
    let other_share = api.share(&other).await;
    api.owner(&original, "unshare").await;
    let (status, body, _) = api
        .create_match(&catalog, &shared_identity, share_access(&other_share), None)
        .await;
    assert_eq!((status, body), (400, unknown_word_list()));
    api.stop().await;

    // The match started through the share before withdrawal is unaffected,
    // including recovery after a restart and native validation.
    let api = Api::start(root.path()).await;
    let (status, body) = api.get(&format!("/games/{running_id}")).await;
    assert_eq!(status, 200, "recovered after restart: {body}");
    api.stop().await;
    let mut game = Match::open(root.path(), &running_id, &running_nonce);
    assert_accepted(&mut game, "the match started through the withdrawn share");
}

#[tokio::test]
async fn match_creation_selects_saved_lists_independently_and_rejects_incompatible_pairs() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let catalog = api.catalog().await;
    let english = api.save("Independent pairs", &pairs()).await;
    let german = api.save("Umlaut list", "ÖL\nOL\n").await;

    // The same saved list composes with independently chosen tile profiles.
    for tile in [
        None,
        Some("acceptance-legacy-tiles"),
        Some("acceptance-valued-tiles"),
    ] {
        let (status, join, _) = api
            .create_match(&catalog, &english.word_list, owner_access(&english), tile)
            .await;
        assert_eq!(status, 200, "{tile:?}: {join}");
        assert!(join["matchId"].as_str().unwrap().starts_with("p_"));
    }
    // Choosing a different board leaves the list untouched as well.
    let nonce = format!("{:032x}", rand_nonce());
    let mut selection = catalog["defaults"]["selection"].clone();
    selection["wordList"] = english.word_list.clone();
    selection["wordListAccess"] = owner_access(&english).unwrap();
    selection["board"] = identity_of(&catalog, "boardProfiles", "acceptance-compact-board");
    let (status, body) = api
        .post(
            "/games",
            json!({"version":3,"requestId":nonce,"requestedAt":unix_seconds().unwrap(),
                "selection":selection,"rules":catalog["defaults"]["rules"]}),
        )
        .await;
    assert_eq!(status, 200, "{body}");

    // Incompatible pair: rejected with the missing letters, nothing created.
    let configurations = root.path().join("configurations");
    let before = file_count(&configurations);
    let (status, body, _) = api
        .create_match(
            &catalog,
            &german.word_list,
            owner_access(&german),
            Some("acceptance-legacy-tiles"),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        body,
        json!({"error":"incompatible-profiles","letters":["Ö"]})
    );
    assert_eq!(file_count(&configurations), before, "no match was created");
    let (_, unchanged) = api.read(&german).await;
    assert_eq!(
        unchanged["wordList"], german.word_list,
        "no silent substitution"
    );
    // The same list with a compatible, separately chosen tile profile works.
    let (status, body, _) = api
        .create_match(
            &catalog,
            &german.word_list,
            owner_access(&german),
            Some(UMLAUT_TILES),
        )
        .await;
    assert_eq!(status, 200, "{body}");

    // Forged or inline identities never resolve.
    let mut forged = english.word_list.clone();
    forged["revision"] = json!(format!(
        "{}0",
        english.word_list["revision"].as_str().unwrap()
    ));
    let (status, body, _) = api
        .create_match(&catalog, &forged, owner_access(&english), None)
        .await;
    assert_eq!(
        (status, body),
        (400, json!({"error":"unknown-profile","profile":"wordList"}))
    );
    let mut inline = english.word_list.clone();
    inline["words"] = json!("ANY\n");
    let (status, _, _) = api
        .create_match(&catalog, &inline, owner_access(&english), None)
        .await;
    assert_eq!(status, 400);
    // Owner access to ANOTHER list never proves access to this one.
    let (status, body, _) = api
        .create_match(&catalog, &english.word_list, owner_access(&german), None)
        .await;
    assert_eq!((status, body), (400, unknown_word_list()));
    let mut mixed = owner_access(&english).unwrap();
    mixed["ownerCapability"] = json!(german.capability);
    let (status, body, _) = api
        .create_match(&catalog, &english.word_list, Some(mixed), None)
        .await;
    assert_eq!((status, body), (400, unknown_word_list()));
    // A recipient may use a shared list directly without copying it.
    let share = api.share(&english).await;
    let (_, shared) = api.get(&format!("/shared-word-lists/{share}")).await;
    let (status, body, _) = api
        .create_match(&catalog, &shared["wordList"], share_access(&share), None)
        .await;
    assert_eq!(status, 200, "{body}");
    // Deleted lists are unavailable for new matches.
    let (status, _) = api.owner(&german, "delete").await;
    assert!(matches!(status, 200 | 204));
    let (status, body, _) = api
        .create_match(
            &catalog,
            &german.word_list,
            owner_access(&german),
            Some(UMLAUT_TILES),
        )
        .await;
    assert_eq!(
        (status, body),
        (400, json!({"error":"unknown-profile","profile":"wordList"}))
    );
    // Catalog word lists are unchanged.
    let catalog_words = identity_of(&catalog, "wordLists", "acceptance-legacy-words");
    let (status, _, _) = api.create_match(&catalog, &catalog_words, None, None).await;
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
            simulation: factory
                .create(&id)
                .expect("a started match reconstructs from its own initialization"),
            id,
            sequences: BTreeMap::new(),
        };
        for player in 1..=3 {
            game.simulation.add_player(player).unwrap();
        }
        for (player, command) in [
            (
                1,
                Command::ClaimBoard {
                    request_id: nonce.into(),
                },
            ),
            (
                2,
                Command::SetName {
                    display_name: "Ada".into(),
                },
            ),
            (
                3,
                Command::SetName {
                    display_name: "Lin".into(),
                },
            ),
            (1, Command::Start {}),
        ] {
            game.send(player, command).unwrap();
        }
        game
    }
    fn view(&self, player: u32) -> PlayerSnapshot {
        serde_json::from_slice(&self.simulation.snapshot_for(player).unwrap().payload).unwrap()
    }
    fn active(&self) -> u32 {
        match self.view(1).public.phase {
            Phase::Playing { active_player, .. } => active_player.get() as u32,
            other => panic!("match is not playing: {other:?}"),
        }
    }
    fn send(&mut self, player: u32, command: Command) -> Result<(), SimulationError> {
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
        self.simulation.apply_command(player, *sequence, &payload)
    }
    /// The active player places its first two lettered tiles across the centre.
    /// Returns the formed word and the native result.
    fn open_two(&mut self) -> (String, Result<(), SimulationError>) {
        let player = self.active();
        let view = self.view(player);
        let centre = view.public.configuration.board_size / 2;
        let letters: Vec<_> = view
            .own_rack
            .tiles
            .iter()
            .filter_map(|tile| match tile.face {
                TileFace::Letter { letter } => Some((tile.id, letter)),
                _ => None,
            })
            .take(2)
            .collect();
        assert_eq!(letters.len(), 2, "rack has two lettered tiles");
        let word: String = letters.iter().map(|(_, letter)| *letter).collect();
        let placements = letters
            .iter()
            .enumerate()
            .map(|(offset, (tile, _))| Placement {
                tile_id: *tile,
                coordinate: Coordinate::new(centre, centre + offset as u8).unwrap(),
                blank_as: None,
            })
            .collect();
        (word, self.send(player, Command::Commit { placements }))
    }
}
fn assert_accepted(game: &mut Match, why: &str) {
    let (word, result) = game.open_two();
    assert!(result.is_ok(), "{why}: {word} must be accepted: {result:?}");
    assert_eq!(game.view(1).public.history.len(), 1);
}
fn assert_rejected(game: &mut Match, why: &str) {
    let before = game.view(1).public;
    let (word, result) = game.open_two();
    let failure = result.expect_err(why);
    let feedback: Value = serde_json::from_slice(
        failure
            .command_rejection()
            .expect("an invalid word is a private, correctable rejection"),
    )
    .unwrap();
    assert_eq!(
        feedback["error"]["kind"], "invalid_words",
        "{why}: {feedback}"
    );
    assert_eq!(feedback["error"]["words"], json!([word]), "{why}");
    let after = game.view(1).public;
    assert_eq!(after.board, before.board);
    assert_eq!(after.history, before.history);
}

#[tokio::test]
async fn original_and_edited_copy_validate_native_commits_differently() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let catalog = api.catalog().await;
    let original = api.save("Pairs original", &pairs()).await;
    let share = api.share(&original).await;
    let (_, copied) = api.copy(&share, "Rejecting copy").await;
    let copy = Saved::parse(&copied);
    let (status, edited) = api.update(&copy, "Rejecting copy", REJECTING_WORDS).await;
    assert_eq!(status, 200, "{edited}");
    let (status, from_original, original_nonce) = api
        .create_match(&catalog, &original.word_list, owner_access(&original), None)
        .await;
    assert_eq!(status, 200, "{from_original}");
    let (status, from_copy, copy_nonce) = api
        .create_match(&catalog, &edited["wordList"], owner_access(&copy), None)
        .await;
    assert_eq!(status, 200, "{from_copy}");
    api.stop().await;

    let mut original_game = Match::open(
        root.path(),
        from_original["matchId"].as_str().unwrap(),
        &original_nonce,
    );
    let mut copy_game = Match::open(
        root.path(),
        from_copy["matchId"].as_str().unwrap(),
        &copy_nonce,
    );
    assert_ne!(
        original_game.view(1).public.dictionary,
        copy_game.view(1).public.dictionary,
        "original and copy bind distinct dictionary identities"
    );
    assert_accepted(&mut original_game, "the original pairs list");
    assert_rejected(&mut copy_game, "the recipient's edited copy");
}

#[tokio::test]
async fn started_matches_keep_their_word_list_revision_after_edit_unshare_delete_restart_and_replay()
 {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let catalog = api.catalog().await;
    let saved = api.save("Pinned pairs", &pairs()).await;
    let kept = api.save("Kept list", "CAT\n").await;
    let share = api.share(&saved).await;
    let (status, pinned, pinned_nonce) = api
        .create_match(&catalog, &saved.word_list, owner_access(&saved), None)
        .await;
    assert_eq!(status, 200, "{pinned}");
    let pinned_id = pinned["matchId"].as_str().unwrap().to_owned();

    let (status, v2) = api.update(&saved, "Pinned v2", REJECTING_WORDS).await;
    assert_eq!(status, 200, "{v2}");
    let (status, edited, edited_nonce) = api
        .create_match(&catalog, &v2["wordList"], owner_access(&saved), None)
        .await;
    assert_eq!(status, 200, "{edited}");
    let edited_id = edited["matchId"].as_str().unwrap().to_owned();
    let (status, _) = api.owner(&saved, "unshare").await;
    assert!(matches!(status, 200 | 204));
    let (status, _) = api.owner(&saved, "delete").await;
    assert!(matches!(status, 200 | 204));
    for identity in [&saved.word_list, &v2["wordList"]] {
        let (status, body, _) = api
            .create_match(&catalog, identity, owner_access(&saved), None)
            .await;
        assert_eq!(
            (status, body),
            (400, json!({"error":"unknown-profile","profile":"wordList"})),
            "deleted versions are unavailable for new matches"
        );
    }
    let (status, body) = api.get(&format!("/shared-word-lists/{share}")).await;
    assert!(withdrawn(status, &body));
    api.stop().await;

    // Restart: the guest library is durable and the started matches are hosted.
    let api = Api::start(root.path()).await;
    for id in [&pinned_id, &edited_id] {
        let (status, body) = api.get(&format!("/games/{id}")).await;
        assert_eq!(status, 200, "recovered after restart: {body}");
    }
    let (status, still) = api.read(&kept).await;
    assert_eq!(status, 200, "saved originals survive a server restart");
    assert_eq!(still["name"], "Kept list");
    let (status, _) = api.read(&saved).await;
    assert_eq!(status, 404);
    api.stop().await;

    // Native canonical replay of the recovered match verifies against its own
    // initialization; its configuration is the pinned original revision.
    let factory = MatchFactory::new(&config(root.path())).unwrap();
    let id = game_server::MatchId::new(pinned_id.clone()).unwrap();
    let image = std::fs::read_dir(root.path().join("recovery"))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| game_server::RecoveryImage::read_file(&entry.path()).ok())
        .find(|image| scrabble_server::replay::inspect_replay(&factory, &id, image).is_ok())
        .expect("the pinned match has a verifiable native recovery image");
    let report = scrabble_server::replay::inspect_replay(&factory, &id, &image).unwrap();
    assert!(report.records_verified > 0 && report.checkpoints_verified > 0);
    let report = serde_json::to_value(report).unwrap();
    for player in report["game"]["players"].as_array().unwrap() {
        assert!(
            player.as_object().unwrap().keys().all(|key| matches!(
                key.as_str(),
                "id" | "display_name" | "score" | "rack_count" | "forfeited"
            )),
            "canonical result must not expose private racks: {player}"
        );
    }
    assert!(!report.to_string().contains("own_rack"));

    let mut pinned_game = Match::open(root.path(), &pinned_id, &pinned_nonce);
    assert_eq!(
        report["configuration"],
        serde_json::to_value(&pinned_game.view(1).public.configuration).unwrap(),
        "replay retains the pinned configuration"
    );
    let mut edited_game = Match::open(root.path(), &edited_id, &edited_nonce);
    assert_ne!(
        pinned_game.view(1).public.dictionary,
        edited_game.view(1).public.dictionary
    );
    // Other players' racks stay private in each player's own projection.
    let ada = pinned_game.view(2);
    assert!(ada.own_rack.tiles.len() == 7 && pinned_game.view(1).own_rack.tiles.is_empty());
    assert!(
        !serde_json::to_string(&ada.public)
            .unwrap()
            .contains("own_rack")
    );
    assert_accepted(
        &mut pinned_game,
        "the pinned v1 pairs list after edit, unshare, delete and restart",
    );
    assert_rejected(&mut edited_game, "the v2 revision selected later");
}

#[tokio::test]
async fn concurrent_lists_and_matches_stay_isolated() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path()).await;
    let catalog = api.catalog().await;
    let all_pairs = pairs();
    let (a, b, c, d) = tokio::join!(
        api.save("Concurrent A", &all_pairs),
        api.save("Concurrent B", REJECTING_WORDS),
        api.save("Concurrent C", &all_pairs),
        api.save("Concurrent D", REJECTING_WORDS),
    );
    let lists = [a, b, c, d];
    let ids: BTreeSet<_> = lists.iter().map(|saved| saved.id.clone()).collect();
    assert_eq!(ids.len(), 4);
    let (ra, rb, rc, rd) = tokio::join!(
        api.create_match(&catalog, &lists[0].word_list, owner_access(&lists[0]), None),
        api.create_match(&catalog, &lists[1].word_list, owner_access(&lists[1]), None),
        api.create_match(
            &catalog,
            &lists[2].word_list,
            owner_access(&lists[2]),
            Some("acceptance-valued-tiles")
        ),
        api.create_match(
            &catalog,
            &lists[3].word_list,
            owner_access(&lists[3]),
            Some("acceptance-valued-tiles")
        ),
    );
    let created = [ra, rb, rc, rd];
    for (status, body, _) in &created {
        assert_eq!(*status, 200, "{body}");
    }
    let matches: BTreeSet<_> = created
        .iter()
        .map(|(_, body, _)| body["matchId"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(matches.len(), 4, "four distinct concurrent matches");
    for saved in &lists {
        let (status, body) = api.read(saved).await;
        assert_eq!(status, 200, "{body}");
    }
    api.stop().await;
    for (index, (_, body, nonce)) in created.iter().enumerate() {
        let mut game = Match::open(root.path(), body["matchId"].as_str().unwrap(), nonce);
        if index % 2 == 0 {
            assert_accepted(&mut game, "a concurrent pairs-list match");
        } else {
            assert_rejected(&mut game, "a concurrent rejecting-list match");
        }
    }
}

/// Harness self-check (expected green at baseline): without the new key the
/// same real host creates and recovers a catalog match, so red library tests
/// fail on the missing #97 behavior rather than on this harness.
#[tokio::test]
async fn harness_self_check_recovers_a_catalog_match_without_the_library_key() {
    let root = tempfile::tempdir().unwrap();
    let api = Api::start_with(root.path(), false, &[]).await;
    let catalog = api.catalog().await;
    let words = identity_of(&catalog, "wordLists", "acceptance-pairs");
    let (status, join, _) = api
        .create_match(&catalog, &words, None, Some(UMLAUT_TILES))
        .await;
    assert_eq!(status, 200, "{join}");
    let match_id = join["matchId"].as_str().unwrap().to_owned();
    api.stop().await;
    let api = Api::start_with(root.path(), false, &[]).await;
    let (status, body) = api.get(&format!("/games/{match_id}")).await;
    assert_eq!(
        status, 200,
        "a recovered match is hosted after restart: {body}"
    );
    api.stop().await;
}
