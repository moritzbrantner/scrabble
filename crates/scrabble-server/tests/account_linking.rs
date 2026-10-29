//! Linking guest-owned libraries to the shared Moenarch account (#132; Decision 1.46).
//!
//! A fake account service implements the session contract (`POST /v1/sessions/verify
//! {token}` -> `{account_id, display_name, session_id, expires_at}` or `401`). Scrabble
//! reads the `moenarch_session` cookie and never accepts an account id from the browser.
//! Covers all three library kinds, incorrect owner, duplicate link, a second device and
//! recovery (a magic-link session of the same account), and persistence across restart.
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[path = "support/hosted.rs"]
mod hosted;
#[path = "support/profile_catalog.rs"]
mod profile_catalog;

const ORIGIN: &str = "http://localhost:5173";
const ADA: &str = "5f0c7b8e-0d3a-4a4e-9b1f-2a6c1c4e7d10";
const BOB: &str = "0b5e4a8f-9a8b-4c0e-8a55-77b1f7c2d001";

/// Token -> (account id, display name). Removing a token models sign-out or revocation.
type Sessions = Arc<Mutex<HashMap<String, (String, String)>>>;

/// Fake account service implementing only the product verification contract.
async fn fake_accounts(sessions: Sessions) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let sessions = Arc::clone(&sessions);
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let mut chunk = [0; 1024];
                let (head, length) = loop {
                    let read = stream.read(&mut chunk).await.unwrap();
                    bytes.extend_from_slice(&chunk[..read]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
                        let length: usize = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse().unwrap())
                            })
                            .unwrap();
                        bytes.drain(..end + 4);
                        break (head, length);
                    }
                };
                while bytes.len() < length {
                    let read = stream.read(&mut chunk).await.unwrap();
                    bytes.extend_from_slice(&chunk[..read]);
                }
                assert!(
                    head.starts_with("POST /v1/sessions/verify HTTP/1.1"),
                    "{head}"
                );
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                let token = request["token"].as_str().unwrap();
                let found = sessions.lock().unwrap().get(token).cloned();
                let (status, body) = match found {
                    Some((account, name)) => (
                        "200 OK",
                        json!({"account_id":account,"display_name":name,
                            "session_id":"ses_test","expires_at":4_000_000_000u64})
                        .to_string(),
                    ),
                    None => (
                        "401 Unauthorized",
                        json!({"error":"invalid_session"}).to_string(),
                    ),
                };
                let _ = stream
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    origin
}

fn values(root: &Path, accounts: &str) -> BTreeMap<String, String> {
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
        ("SCRABBLE_ACCOUNTS_ORIGIN".into(), accounts.into()),
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
    async fn start(root: &Path, accounts: &str) -> Self {
        let hosted = hosted::start(&values(root, accounts)).await;
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
    /// A browser request from the board origin, optionally carrying a session cookie.
    async fn post(&self, path: &str, session: Option<&str>, body: Value) -> (u16, Value) {
        let body = body.to_string();
        let cookie = session
            .map(|token| format!("Cookie: theme=dark; moenarch_session={token}\r\n"))
            .unwrap_or_default();
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).await.unwrap();
        stream
            .write_all(
                format!(
                    "POST {path} HTTP/1.1\r\nHost: localhost\r\nOrigin: {ORIGIN}\r\n{cookie}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes).await;
        let response = String::from_utf8(bytes).unwrap();
        let (header, body) = response.split_once("\r\n\r\n").unwrap();
        let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = if body.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(body).unwrap()
        };
        (status, body)
    }
}

/// One guest-owned original of each kind: (kind, id, capability, owner route prefix).
async fn create_all(api: &Api) -> Vec<(&'static str, String, String, &'static str)> {
    let tiles: Vec<Value> = ('A'..='Z')
        .map(|letter| json!({"face":{"kind":"letter","letter":letter},"count":2,"value":1}))
        .collect();
    let requests = [
        (
            "wordList",
            "/word-lists",
            json!({"version":1,"name":"Ada words","words":"CAT\nDOG\n"}),
            "wordListId",
        ),
        (
            "tileProfile",
            "/tile-profiles",
            json!({"version":1,"name":"Ada tiles","tiles":tiles}),
            "profileId",
        ),
        (
            "layout",
            "/layouts",
            json!({"version":1,"name":"Ada board","layout":
                {"boardShape":"square","boardSize":5,"premiums":vec!["normal";25]}}),
            "layoutId",
        ),
    ];
    let mut created = Vec::new();
    for (kind, path, body, id_field) in requests {
        let (status, body) = api.post(path, None, body).await;
        assert_eq!(status, 200, "{kind} must save as a guest: {body}");
        created.push((
            kind,
            body[id_field].as_str().unwrap().to_owned(),
            body["ownerCapability"].as_str().unwrap().to_owned(),
            path,
        ));
    }
    created
}

async fn owner_read(api: &Api, prefix: &str, id: &str, capability: &str) -> u16 {
    api.post(
        &format!("{prefix}/{id}/read"),
        None,
        json!({"version":1,"ownerCapability":capability}),
    )
    .await
    .0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guest_libraries_link_to_one_account_and_reconnect_on_another_device() {
    let sessions: Sessions = Arc::default();
    let accounts = fake_accounts(Arc::clone(&sessions)).await;
    sessions
        .lock()
        .unwrap()
        .insert("mns_ada_laptop".into(), (ADA.into(), "Ada".into()));
    sessions
        .lock()
        .unwrap()
        .insert("mns_bob".into(), (BOB.into(), "Bob".into()));
    let root = tempfile::tempdir().unwrap();
    let api = Api::start(root.path(), &accounts).await;

    // Signed out: guest use continues and the account page is offered.
    let (status, body) = api
        .post("/account/session", None, json!({"version":1}))
        .await;
    assert_eq!(status, 401);
    assert_eq!(body["error"], "signed-out");
    assert_eq!(body["signIn"], format!("{accounts}/"));
    let (status, body) = api
        .post(
            "/account/session",
            Some("mns_ada_laptop"),
            json!({"version":1}),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        json!({"version":1,"accountId":ADA,"displayName":"Ada"})
    );

    let created = create_all(&api).await;
    for (kind, id, capability, _) in &created {
        let link = json!({"version":1,"kind":kind,"id":id,"ownerCapability":capability});
        // Linking needs the guest owner capability, not only a session.
        let (status, body) = api
            .post(
                "/account/link",
                Some("mns_ada_laptop"),
                json!({"version":1,"kind":kind,"id":id,"ownerCapability":"0".repeat(64)}),
            )
            .await;
        assert_eq!(status, 403, "{kind}: {body}");
        let (status, body) = api.post("/account/link", None, link.clone()).await;
        assert_eq!((status, &body["error"]), (401, &json!("signed-out")));
        let (status, body) = api
            .post("/account/link", Some("mns_ada_laptop"), link.clone())
            .await;
        assert_eq!(status, 200, "{kind}: {body}");
        assert_eq!(body["linked"], "new");
        // Duplicate link by the same account is idempotent.
        let (status, body) = api
            .post("/account/link", Some("mns_ada_laptop"), link.clone())
            .await;
        assert_eq!((status, &body["linked"]), (200, &json!("existing")));
        // Wrong owner: another account holding the guest capability cannot take it over.
        let (status, body) = api.post("/account/link", Some("mns_bob"), link).await;
        assert_eq!(
            (status, &body["error"]),
            (409, &json!("account-link-conflict"))
        );
        let (status, body) = api
            .post(
                "/account/reconnect",
                Some("mns_bob"),
                json!({"version":1,"kind":kind,"id":id}),
            )
            .await;
        assert_eq!((status, &body["error"]), (403, &json!("account-not-owner")));
    }
    let (status, body) = api
        .post("/account/libraries", Some("mns_bob"), json!({"version":1}))
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        json!({"version":1,"wordLists":[],"tileProfiles":[],"layouts":[]})
    );

    // A second device signs in to the same account (new session, same account id).
    sessions
        .lock()
        .unwrap()
        .insert("mns_ada_phone".into(), (ADA.into(), "Ada".into()));
    let (status, listed) = api
        .post(
            "/account/libraries",
            Some("mns_ada_phone"),
            json!({"version":1}),
        )
        .await;
    assert_eq!(status, 200, "{listed}");
    for (field, index, name) in [
        ("wordLists", 0, "Ada words"),
        ("tileProfiles", 1, "Ada tiles"),
        ("layouts", 2, "Ada board"),
    ] {
        assert_eq!(listed[field], json!([{"id":created[index].1,"name":name}]));
    }
    let mut phone = Vec::new();
    for (kind, id, capability, prefix) in &created {
        let (status, body) = api
            .post(
                "/account/reconnect",
                Some("mns_ada_phone"),
                json!({"version":1,"kind":kind,"id":id}),
            )
            .await;
        assert_eq!(status, 200, "{kind}: {body}");
        let device = body["ownerCapability"].as_str().unwrap().to_owned();
        assert_ne!(&device, capability);
        // Both the phone and the original guest browser manage the original.
        assert_eq!(owner_read(&api, prefix, id, &device).await, 200, "{kind}");
        assert_eq!(
            owner_read(&api, prefix, id, capability).await,
            200,
            "{kind}"
        );
        phone.push(device);
    }

    // Sign-out/revocation: the verifier answers 401 (after any cached answer expires,
    // which a fresh token never has), so account calls fail while guest use continues.
    sessions.lock().unwrap().remove("mns_bob");
    let (status, _) = api
        .post("/account/libraries", Some("mns_bob"), json!({"version":1}))
        .await;
    assert_eq!(
        status, 200,
        "a positive answer may be cached for up to 60 s"
    );
    let (status, body) = api
        .post(
            "/account/libraries",
            Some("mns_unknown"),
            json!({"version":1}),
        )
        .await;
    assert_eq!((status, &body["error"]), (401, &json!("signed-out")));

    // Links and device capabilities survive a restart.
    api.stop().await;
    let api = Api::start(root.path(), &accounts).await;

    // Recovery: every passkey lost, the magic link starts a new session of the SAME
    // account id, so the linked libraries reconnect on a fresh device.
    sessions
        .lock()
        .unwrap()
        .insert("mns_ada_recovered".into(), (ADA.into(), "Ada".into()));
    let (status, listed_again) = api
        .post(
            "/account/libraries",
            Some("mns_ada_recovered"),
            json!({"version":1}),
        )
        .await;
    assert_eq!((status, &listed_again), (200, &listed));
    for ((kind, id, capability, prefix), device) in created.iter().zip(&phone) {
        let (status, body) = api
            .post(
                "/account/reconnect",
                Some("mns_ada_recovered"),
                json!({"version":1,"kind":kind,"id":id}),
            )
            .await;
        assert_eq!(status, 200, "{kind}: {body}");
        let recovered = body["ownerCapability"].as_str().unwrap();
        for owner in [recovered, device.as_str(), capability.as_str()] {
            assert_eq!(owner_read(&api, prefix, id, owner).await, 200, "{kind}");
        }
    }

    // Deleting the original from any device removes it from the account listing.
    let (_, id, _, prefix) = &created[0];
    let (status, _) = api
        .post(
            &format!("{prefix}/{id}/delete"),
            None,
            json!({"version":1,"ownerCapability":phone[0]}),
        )
        .await;
    assert_eq!(status, 204);
    let (_, listed) = api
        .post(
            "/account/libraries",
            Some("mns_ada_recovered"),
            json!({"version":1}),
        )
        .await;
    assert_eq!(listed["wordLists"], json!([]));
    api.stop().await;
}

/// Raw HTTP exchange returning the status line and headers, and the body.
async fn exchange(port: u16, request: String) -> (String, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes).await;
    let response = String::from_utf8(bytes).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    (head.to_owned(), body.to_owned())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guest_board_origins_play_as_guests_but_never_reach_account_routes() {
    const GUEST: &str = "http://guest.localhost:4173";
    let root = tempfile::tempdir().unwrap();
    let sessions = Sessions::default();
    sessions
        .lock()
        .unwrap()
        .insert("mns_ada".into(), (ADA.into(), "Ada".into()));
    let accounts = fake_accounts(sessions).await;
    let mut config = values(root.path(), &accounts);
    config.insert("SCRABBLE_GUEST_BOARD_ORIGINS".into(), GUEST.into());
    let hosted = hosted::start(&config).await;
    let api = Api {
        port: hosted.api_port,
        task: Some(hosted.task),
        shutdown: hosted.shutdown,
    };

    // Guest routes answer the guest origin with its own CORS origin.
    let (head, body) = exchange(
        api.port,
        format!("GET /profiles HTTP/1.1\r\nHost: localhost\r\nOrigin: {GUEST}\r\nConnection: close\r\n\r\n"),
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}\n{body}");
    assert!(head.contains(&format!("Access-Control-Allow-Origin: {GUEST}\r\n")));

    // The account preflight succeeds so the board can read the guest-only answer...
    let (head, _) = exchange(
        api.port,
        format!("OPTIONS /account/session HTTP/1.1\r\nHost: localhost\r\nOrigin: {GUEST}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 204"), "{head}");
    // ...but a valid session cookie from the guest origin is never honoured.
    let (head, body) = exchange(
        api.port,
        format!("POST /account/session HTTP/1.1\r\nHost: localhost\r\nOrigin: {GUEST}\r\nCookie: moenarch_session=mns_ada\r\nContent-Type: application/json\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{{\"version\":1}}"),
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 403"), "{head}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["error"],
        "guest-origin"
    );

    // The board origin keeps account access, and unknown origins stay rejected.
    let (status, body) = api
        .post("/account/session", Some("mns_ada"), json!({"version":1}))
        .await;
    assert_eq!((status, &body["accountId"]), (200, &json!(ADA)));
    let (head, body) = exchange(
        api.port,
        "GET /profiles HTTP/1.1\r\nHost: localhost\r\nOrigin: https://other.example\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 403"), "{head}");
    assert!(head.contains(&format!("Access-Control-Allow-Origin: {ORIGIN}\r\n")));
    assert!(body.contains("origin-rejected"));
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_routes_need_the_board_origin_and_a_reachable_account_service() {
    let root = tempfile::tempdir().unwrap();
    // Nothing listens here: the account service is unavailable, not a signed-out answer.
    let unreachable = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port())
    };
    let api = Api::start(root.path(), &unreachable).await;
    let (status, body) = api
        .post("/account/session", Some("mns_x"), json!({"version":1}))
        .await;
    assert_eq!(
        (status, &body["error"]),
        (503, &json!("accounts-unavailable"))
    );

    // Without an Origin header (not a browser from the board), cookies are not honoured.
    let mut stream = TcpStream::connect(("127.0.0.1", api.port)).await.unwrap();
    stream
        .write_all(b"POST /account/session HTTP/1.1\r\nHost: localhost\r\nCookie: moenarch_session=mns_x\r\nContent-Type: application/json\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{\"version\":1}")
        .await
        .unwrap();
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes).await;
    assert!(bytes.starts_with(b"HTTP/1.1 403"));
    api.stop().await;
}
