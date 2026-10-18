//! #158: turn-start Web Push fan-out, privacy and persistence against a local push service.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use game_server::{ControlContext, MatchControlService, MatchId};
use ring::{aead, agreement, hkdf, rand::SystemRandom, signature};
use scrabble_server::push::{
    Observation, PushControl, PushService, Subscription, observe_payload, turn_payload,
};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex, mpsc},
};

struct Device {
    private: agreement::EphemeralPrivateKey,
    subscription: Subscription,
}

fn device(endpoint: String) -> Device {
    let rng = SystemRandom::new();
    let private = agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &rng).unwrap();
    let public = private.compute_public_key().unwrap();
    Device {
        subscription: Subscription {
            endpoint,
            p256dh: B64.encode(public.as_ref()),
            auth: B64.encode([7; 16]),
        },
        private,
    }
}

struct Length(usize);
impl hkdf::KeyType for Length {
    fn len(&self) -> usize {
        self.0
    }
}
fn expand(prk: &hkdf::Prk, info: &[&[u8]], length: usize) -> Vec<u8> {
    let mut output = vec![0; length];
    prk.expand(info, Length(length))
        .unwrap()
        .fill(&mut output)
        .unwrap();
    output
}

/// Independent RFC 8291 receiver: decrypts exactly what a browser would.
fn decrypt(device: Device, body: &[u8]) -> Vec<u8> {
    let salt = &body[..16];
    let id_length = usize::from(body[20]);
    let sender = &body[21..21 + id_length];
    let receiver = B64.decode(&device.subscription.p256dh).unwrap();
    let auth = B64.decode(&device.subscription.auth).unwrap();
    let shared = agreement::agree_ephemeral(
        device.private,
        &agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, sender),
        |secret| secret.to_vec(),
    )
    .unwrap();
    let prk_key = hkdf::Salt::new(hkdf::HKDF_SHA256, &auth).extract(&shared);
    let ikm = expand(&prk_key, &[b"WebPush: info\0", &receiver, sender], 32);
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, salt).extract(&ikm);
    let cek = expand(&prk, &[b"Content-Encoding: aes128gcm\0"], 16);
    let nonce = expand(&prk, &[b"Content-Encoding: nonce\0"], 12);
    let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_128_GCM, &cek).unwrap());
    let mut record = body[21 + id_length..].to_vec();
    let plain = key
        .open_in_place(
            aead::Nonce::try_assume_unique_for_key(&nonce).unwrap(),
            aead::Aad::empty(),
            &mut record,
        )
        .unwrap();
    assert_eq!(plain.last(), Some(&2));
    plain[..plain.len() - 1].to_vec()
}

struct Received {
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

/// A plain-HTTP stand-in for a browser push service; `status` is returned for every request.
fn push_service(status: Arc<Mutex<u16>>) -> (String, mpsc::Receiver<Received>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap().to_owned();
            let mut headers = BTreeMap::new();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                let header = header.trim_end();
                if header.is_empty() {
                    break;
                }
                let (name, value) = header.split_once(": ").unwrap();
                headers.insert(name.to_ascii_lowercase(), value.to_owned());
            }
            let length: usize = headers["content-length"].parse().unwrap();
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let code = *status.lock().unwrap();
            write!(
                stream,
                "HTTP/1.1 {code} Status\r\nContent-Length: 0\r\n\r\n"
            )
            .unwrap();
            sender
                .send(Received {
                    path,
                    headers,
                    body,
                })
                .unwrap();
        }
    });
    (origin, receiver)
}

fn control(
    service: &Arc<PushService>,
    player: u32,
    payload: serde_json::Value,
) -> Result<Vec<u8>, String> {
    PushControl(Arc::clone(service))
        .handle(
            &MatchId::new("table-1").unwrap(),
            ControlContext {
                player_id: player,
                connection_epoch: 1,
            },
            &serde_json::to_vec(&payload).unwrap(),
        )
        .map_err(|error| error.to_string())
}

fn subscribe(
    service: &Arc<PushService>,
    player: u32,
    subscription: &Subscription,
) -> Result<Vec<u8>, String> {
    control(
        service,
        player,
        serde_json::json!({ "kind": "push_subscribe", "subscription": subscription }),
    )
}

#[test]
fn every_subscribed_device_of_the_new_active_player_gets_one_encrypted_content_free_push() {
    let directory = tempfile::tempdir().unwrap();
    let status = Arc::new(Mutex::new(201));
    let (origin, received) = push_service(Arc::clone(&status));
    let service = Arc::new(
        PushService::open(directory.path(), "https://board.example".into(), true).unwrap(),
    );
    let key: serde_json::Value = serde_json::from_slice(
        &control(&service, 2, serde_json::json!({"kind":"push_key"})).unwrap(),
    )
    .unwrap();
    let public_key = B64.decode(key["public_key"].as_str().unwrap()).unwrap();

    // Two authorized devices for player 2, one for the board actor (1), none for player 3.
    let phone = device(format!("{origin}/push/phone"));
    let tablet = device(format!("{origin}/push/tablet"));
    let board = device(format!("{origin}/push/board"));
    for (player, device) in [(2, &phone), (2, &tablet), (1, &board)] {
        subscribe(&service, player, &device.subscription).unwrap();
    }
    let id = "table-1";
    let turn = |active: u32, turn: &str| Observation::Turn {
        active,
        turn: turn.into(),
    };
    // The first observation only records the current turn; opting in mid-turn sends nothing.
    assert!(service.observe(id, turn(2, "0")).is_empty());
    assert!(service.observe(id, turn(3, "1")).is_empty());
    let deliveries = service.observe(id, turn(2, "2"));
    assert_eq!(deliveries.len(), 2);
    // Repeated or stale observations of the same turn never notify twice.
    assert!(service.observe(id, turn(2, "2")).is_empty());

    for delivery in &deliveries {
        assert_eq!(service.deliver(delivery, &turn_payload()).unwrap(), 201);
    }
    let mut messages: BTreeMap<String, Received> = (0..2)
        .map(|_| {
            let message = received.recv().unwrap();
            (message.path.clone(), message)
        })
        .collect();
    for (path, device) in [("/push/phone", phone), ("/push/tablet", tablet)] {
        let message = messages.remove(path).unwrap();
        assert_eq!(message.headers["content-encoding"], "aes128gcm");
        assert!(message.headers["ttl"].parse::<u32>().unwrap() > 0);
        let authorization = &message.headers["authorization"];
        let (token, key) = authorization
            .strip_prefix("vapid t=")
            .unwrap()
            .split_once(", k=")
            .unwrap();
        assert_eq!(B64.decode(key).unwrap(), public_key);
        let (signed, signature) = token.rsplit_once('.').unwrap();
        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, &public_key)
            .verify(signed.as_bytes(), &B64.decode(signature).unwrap())
            .unwrap();
        let claims: serde_json::Value =
            serde_json::from_slice(&B64.decode(signed.split_once('.').unwrap().1).unwrap())
                .unwrap();
        assert_eq!(claims["aud"], origin);
        assert_eq!(claims["sub"], "https://board.example");
        // The payload carries no rack, draft, credential, link or move.
        assert_eq!(decrypt(device, &message.body), br#"{"kind":"turn"}"#);
    }
    assert!(received.try_recv().is_err());
}

#[test]
fn devices_unsubscribe_independently_and_state_survives_restart_without_repeats() {
    let directory = tempfile::tempdir().unwrap();
    let status = Arc::new(Mutex::new(201));
    let (origin, _received) = push_service(Arc::clone(&status));
    let open = || {
        Arc::new(PushService::open(directory.path(), "https://board.example".into(), true).unwrap())
    };
    let service = open();
    let phone = device(format!("{origin}/push/phone"));
    let tablet = device(format!("{origin}/push/tablet"));
    subscribe(&service, 2, &phone.subscription).unwrap();
    subscribe(&service, 2, &tablet.subscription).unwrap();
    let id = "table-1";
    let turn = |active: u32, turn: &str| Observation::Turn {
        active,
        turn: turn.into(),
    };
    service.observe(id, turn(3, "0"));

    // Another player cannot remove player 2's device; player 2 removes only the phone.
    control(
        &service,
        3,
        serde_json::json!({"kind":"push_unsubscribe","endpoint":phone.subscription.endpoint}),
    )
    .unwrap();
    assert_eq!(service.observe(id, turn(2, "1")).len(), 2);
    control(
        &service,
        2,
        serde_json::json!({"kind":"push_unsubscribe","endpoint":phone.subscription.endpoint}),
    )
    .unwrap();
    service.observe(id, turn(3, "2"));
    let remaining = service.observe(id, turn(2, "3"));
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining[0].subscription.endpoint,
        tablet.subscription.endpoint
    );

    // Restart: subscriptions and the last announced turn persist; nothing repeats.
    drop(service);
    let service = open();
    assert!(service.observe(id, turn(2, "3")).is_empty());
    service.observe(id, turn(3, "4"));
    let after = service.observe(id, turn(2, "5"));
    assert_eq!(after.len(), 1);
    // A push service reporting the subscription gone (410) removes it.
    *status.lock().unwrap() = 410;
    assert_eq!(service.deliver(&after[0], &turn_payload()).unwrap(), 410);
    service.observe(id, turn(3, "6"));
    assert!(service.observe(id, turn(2, "7")).is_empty());

    // Finished matches drop every subscription.
    subscribe(&service, 2, &phone.subscription).unwrap();
    service.observe(id, Observation::Ended);
    assert!(service.subscribed_matches().is_empty());
}

#[test]
fn malformed_insecure_and_excess_subscriptions_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let secure = Arc::new(
        PushService::open(directory.path(), "https://board.example".into(), false).unwrap(),
    );
    let plain = device("http://127.0.0.1:9/push".into());
    assert!(subscribe(&secure, 2, &plain.subscription).is_err());
    let mut bad = device("https://push.example/x".into()).subscription;
    bad.auth = B64.encode([1; 3]);
    assert!(subscribe(&secure, 2, &bad).is_err());
    assert!(control(&secure, 2, serde_json::json!({"kind":"push_everyone"})).is_err());
    for index in 0..scrabble_server::push::MAX_DEVICES_PER_PLAYER {
        subscribe(
            &secure,
            2,
            &device(format!("https://push.example/{index}")).subscription,
        )
        .unwrap();
    }
    assert!(
        subscribe(
            &secure,
            2,
            &device("https://push.example/extra".into()).subscription
        )
        .is_err()
    );
    // Rebinding the same endpoint replaces rather than duplicates it.
    subscribe(
        &secure,
        2,
        &device("https://push.example/0".into()).subscription,
    )
    .unwrap();
}

#[test]
fn only_the_authoritative_playing_phase_announces_a_turn() {
    let payload = |phase: serde_json::Value| {
        serde_json::to_vec(&serde_json::json!({"game":{"phase":phase}})).unwrap()
    };
    assert_eq!(
        observe_payload(&payload(
            serde_json::json!({"kind":"playing","active_player":"3","turn":"4"})
        )),
        Observation::Turn {
            active: 3,
            turn: "4".into()
        }
    );
    assert_eq!(
        observe_payload(&payload(serde_json::json!({"kind":"lobby"}))),
        Observation::Waiting
    );
    assert_eq!(
        observe_payload(&payload(
            serde_json::json!({"kind":"finished","winners":["2"]})
        )),
        Observation::Ended
    );
}
