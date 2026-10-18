//! Opt-in Web Push turn notifications (#158; Decision Atlas 2.11 B, 2.14 A).
//!
//! Players opt in over their transport-authenticated reliable-control stream, so a
//! subscription is bound to the native player identity, never a claimed name or link.
//! Subscriptions live in a private file beside other server state, separate from canonical
//! match state and replay. A watcher reads authoritative match phase and pushes a
//! content-free "your turn" message to every subscribed device of the newly active player
//! (RFC 8291 aes128gcm payload, RFC 8292 VAPID).
use crate::simulation::ScrabbleSimulation;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use game_server::{
    ControlContext, ControlServiceError, LiveMatchHost, MatchControlService, MatchId,
};
use ring::{
    aead, agreement, hkdf,
    rand::{SecureRandom, SystemRandom},
    signature::{self, EcdsaKeyPair, KeyPair},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Independently subscribed devices per player and match (2.14 A fans out to each).
pub const MAX_DEVICES_PER_PLAYER: usize = 4;
pub const MAX_SUBSCRIPTIONS: usize = 4096;
const MAX_ENDPOINT_BYTES: usize = 1024;
const RECORD_SIZE: u32 = 4096;
const TTL_SECONDS: u32 = 6 * 60 * 60;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subscription {
    pub endpoint: String,
    /// Base64url uncompressed P-256 user-agent public key (65 bytes).
    pub p256dh: String,
    /// Base64url 16-byte authentication secret.
    pub auth: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u8,
    /// match id → player id → devices.
    subscriptions: BTreeMap<String, BTreeMap<u32, Vec<Subscription>>>,
    /// match id → last turn already announced (or observed), so restarts never repeat it.
    notified: BTreeMap<String, String>,
}

/// One delivery to one device.
#[derive(Clone, Debug)]
pub struct Delivery {
    pub match_id: String,
    pub player: u32,
    pub subscription: Subscription,
}

pub struct PushService {
    file: PathBuf,
    key: EcdsaKeyPair,
    subject: String,
    allow_insecure: bool,
    state: Mutex<Stored>,
    rng: SystemRandom,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ControlCommand {
    #[serde(rename = "push_key")]
    Key {},
    #[serde(rename = "push_subscribe")]
    Subscribe { subscription: Subscription },
    #[serde(rename = "push_unsubscribe")]
    Unsubscribe { endpoint: String },
}

impl PushService {
    /// Loads (or creates) the VAPID key and subscription file under `directory`.
    pub fn open(
        directory: &Path,
        subject: String,
        allow_insecure: bool,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        std::fs::create_dir_all(directory)?;
        let rng = SystemRandom::new();
        let key_file = directory.join("vapid.pk8");
        let pkcs8 = match std::fs::read(&key_file) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let generated =
                    EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
                        .map_err(|_| "VAPID key generation failed")?;
                write_private(&key_file, generated.as_ref())?;
                generated.as_ref().to_vec()
            }
            Err(error) => return Err(error.into()),
        };
        let key =
            EcdsaKeyPair::from_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &pkcs8, &rng)
                .map_err(|_| "invalid VAPID key file")?;
        let file = directory.join("subscriptions.json");
        let state = match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Stored {
                version: 1,
                ..Stored::default()
            },
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            file,
            key,
            subject,
            allow_insecure,
            state: Mutex::new(state),
            rng,
        })
    }

    pub fn public_key(&self) -> String {
        B64.encode(self.key.public_key().as_ref())
    }

    fn persist(&self, state: &Stored) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(state).map_err(std::io::Error::other)?;
        write_private(&self.file, &bytes)
    }

    fn validate(&self, subscription: &Subscription) -> Result<(), &'static str> {
        if subscription.endpoint.len() > MAX_ENDPOINT_BYTES {
            return Err("push endpoint too long");
        }
        let url = url::Url::parse(&subscription.endpoint).map_err(|_| "invalid push endpoint")?;
        let secure = url.scheme() == "https";
        if !(secure || (self.allow_insecure && url.scheme() == "http")) || url.host_str().is_none()
        {
            return Err("push endpoint must be HTTPS");
        }
        let public = B64
            .decode(&subscription.p256dh)
            .map_err(|_| "invalid p256dh")?;
        let auth = B64.decode(&subscription.auth).map_err(|_| "invalid auth")?;
        if public.len() != 65 || public[0] != 4 || auth.len() != 16 {
            return Err("invalid subscription keys");
        }
        Ok(())
    }

    /// Adds or refreshes one device for the authenticated player; other devices are untouched.
    pub fn subscribe(
        &self,
        match_id: &str,
        player: u32,
        subscription: Subscription,
    ) -> Result<(), &'static str> {
        self.validate(&subscription)?;
        let mut state = self.state.lock().map_err(|_| "push state unavailable")?;
        // An endpoint identifies one browser installation: rebinding it moves the device.
        for players in state.subscriptions.values_mut() {
            for devices in players.values_mut() {
                devices.retain(|device| device.endpoint != subscription.endpoint);
            }
        }
        let total: usize = state
            .subscriptions
            .values()
            .flat_map(BTreeMap::values)
            .map(Vec::len)
            .sum();
        if total >= MAX_SUBSCRIPTIONS {
            return Err("push subscription capacity reached");
        }
        let devices = state
            .subscriptions
            .entry(match_id.to_owned())
            .or_default()
            .entry(player)
            .or_default();
        if devices.len() >= MAX_DEVICES_PER_PLAYER {
            return Err("too many devices for this player");
        }
        devices.push(subscription);
        prune(&mut state);
        self.persist(&state)
            .map_err(|_| "push state could not be saved")
    }

    /// Removes only this player's device with the given endpoint.
    pub fn unsubscribe(
        &self,
        match_id: &str,
        player: u32,
        endpoint: &str,
    ) -> Result<(), &'static str> {
        let mut state = self.state.lock().map_err(|_| "push state unavailable")?;
        if let Some(devices) = state
            .subscriptions
            .get_mut(match_id)
            .and_then(|players| players.get_mut(&player))
        {
            devices.retain(|device| device.endpoint != endpoint);
        }
        prune(&mut state);
        self.persist(&state)
            .map_err(|_| "push state could not be saved")
    }

    pub fn subscribed_matches(&self) -> Vec<String> {
        self.state
            .lock()
            .map(|state| state.subscriptions.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Records one authoritative observation of a match and returns the deliveries it causes.
    /// The first observation of a match only records the current turn, so opting in mid-turn
    /// never announces a turn that had already begun, and restarts never repeat one.
    pub fn observe(&self, match_id: &str, observation: Observation) -> Vec<Delivery> {
        let Ok(mut state) = self.state.lock() else {
            return vec![];
        };
        let deliveries = match observation {
            Observation::Ended => {
                state.subscriptions.remove(match_id);
                state.notified.remove(match_id);
                vec![]
            }
            Observation::Waiting => vec![],
            Observation::Turn { active, turn } => {
                let previous = state.notified.insert(match_id.to_owned(), turn.clone());
                match previous {
                    Some(previous) if previous != turn => state
                        .subscriptions
                        .get(match_id)
                        .and_then(|players| players.get(&active))
                        .map(|devices| {
                            devices
                                .iter()
                                .map(|subscription| Delivery {
                                    match_id: match_id.to_owned(),
                                    player: active,
                                    subscription: subscription.clone(),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    _ => vec![],
                }
            }
        };
        prune(&mut state);
        let _ = self.persist(&state);
        deliveries
    }

    /// Encrypts and sends one notification. Returns the push service's HTTP status.
    pub fn deliver(
        &self,
        delivery: &Delivery,
        payload: &[u8],
    ) -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
        let body = encrypt(&self.rng, &delivery.subscription, payload)?;
        let url = url::Url::parse(&delivery.subscription.endpoint)?;
        let audience = url.origin().ascii_serialization();
        let jwt = self.vapid_token(&audience)?;
        let status = post(
            &url,
            &body,
            &format!("vapid t={jwt}, k={}", self.public_key()),
            self.allow_insecure,
        )?;
        if matches!(status, 404 | 410) {
            // The device unsubscribed at its push service; forget it.
            let _ = self.unsubscribe(
                &delivery.match_id,
                delivery.player,
                &delivery.subscription.endpoint,
            );
        }
        Ok(status)
    }

    fn vapid_token(
        &self,
        audience: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let expires = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 12 * 60 * 60;
        let header = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = B64.encode(serde_json::to_vec(&serde_json::json!({
            "aud": audience,
            "exp": expires,
            "sub": self.subject,
        }))?);
        let signing_input = format!("{header}.{claims}");
        let signature = self
            .key
            .sign(&self.rng, signing_input.as_bytes())
            .map_err(|_| "VAPID signing failed")?;
        Ok(format!(
            "{signing_input}.{}",
            B64.encode(signature.as_ref())
        ))
    }
}

/// What the watcher saw in one authoritative match snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Observation {
    /// Lobby: nothing to announce yet.
    Waiting,
    Turn {
        active: u32,
        turn: String,
    },
    /// Finished or retired: subscriptions for the match are dropped.
    Ended,
}

/// Reads only the phase from a canonical snapshot payload; nothing private leaves this call.
pub fn observe_payload(payload: &[u8]) -> Observation {
    #[derive(Deserialize)]
    struct Canonical {
        game: Game,
    }
    #[derive(Deserialize)]
    struct Game {
        phase: scrabble_game::protocol::Phase,
    }
    match serde_json::from_slice::<Canonical>(payload).map(|value| value.game.phase) {
        Ok(scrabble_game::protocol::Phase::Playing {
            active_player,
            turn,
        }) => match u32::try_from(active_player.get()) {
            Ok(active) => Observation::Turn {
                active,
                turn: serde_json::to_value(turn)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default(),
            },
            Err(_) => Observation::Waiting,
        },
        Ok(scrabble_game::protocol::Phase::Lobby {}) => Observation::Waiting,
        Ok(scrabble_game::protocol::Phase::Finished { .. }) | Err(_) => Observation::Ended,
    }
}

/// The notification body. It carries no rack, draft, credential, link or move.
pub fn turn_payload() -> Vec<u8> {
    br#"{"kind":"turn"}"#.to_vec()
}

fn prune(state: &mut Stored) {
    for players in state.subscriptions.values_mut() {
        players.retain(|_, devices| !devices.is_empty());
    }
    state.subscriptions.retain(|_, players| !players.is_empty());
    let subscribed: Vec<String> = state.subscriptions.keys().cloned().collect();
    state.notified.retain(|id, _| subscribed.contains(id));
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(temporary, path)
}

struct Length(usize);
impl hkdf::KeyType for Length {
    fn len(&self) -> usize {
        self.0
    }
}
fn expand(prk: &hkdf::Prk, info: &[&[u8]], length: usize) -> Result<Vec<u8>, &'static str> {
    let mut output = vec![0; length];
    prk.expand(info, Length(length))
        .and_then(|okm| okm.fill(&mut output))
        .map_err(|_| "HKDF failed")?;
    Ok(output)
}

/// RFC 8291 aes128gcm encryption of one single-record payload.
pub fn encrypt(
    rng: &dyn SecureRandom,
    subscription: &Subscription,
    payload: &[u8],
) -> Result<Vec<u8>, &'static str> {
    let receiver = B64
        .decode(&subscription.p256dh)
        .map_err(|_| "invalid p256dh")?;
    let auth = B64.decode(&subscription.auth).map_err(|_| "invalid auth")?;
    let ephemeral = agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, rng)
        .map_err(|_| "ECDH key generation failed")?;
    let sender = ephemeral
        .compute_public_key()
        .map_err(|_| "ECDH public key failed")?;
    let sender = sender.as_ref().to_vec();
    let shared = agreement::agree_ephemeral(
        ephemeral,
        &agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, &receiver),
        |secret| secret.to_vec(),
    )
    .map_err(|_| "ECDH agreement failed")?;
    let mut salt = [0; 16];
    rng.fill(&mut salt).map_err(|_| "random salt failed")?;
    seal(&shared, &auth, &receiver, &sender, &salt, payload)
}

/// Key derivation and sealing, separated so tests can check RFC 8291's published vector.
pub fn seal(
    shared: &[u8],
    auth: &[u8],
    receiver: &[u8],
    sender: &[u8],
    salt: &[u8; 16],
    payload: &[u8],
) -> Result<Vec<u8>, &'static str> {
    let prk_key = hkdf::Salt::new(hkdf::HKDF_SHA256, auth).extract(shared);
    let ikm = expand(&prk_key, &[b"WebPush: info\0", receiver, sender], 32)?;
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, salt).extract(&ikm);
    let cek = expand(&prk, &[b"Content-Encoding: aes128gcm\0"], 16)?;
    let nonce = expand(&prk, &[b"Content-Encoding: nonce\0"], 12)?;
    let key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_128_GCM, &cek).map_err(|_| "AES key failed")?,
    );
    let mut record = payload.to_vec();
    record.push(2); // final-record delimiter, no padding
    key.seal_in_place_append_tag(
        aead::Nonce::try_assume_unique_for_key(&nonce).map_err(|_| "nonce failed")?,
        aead::Aad::empty(),
        &mut record,
    )
    .map_err(|_| "AES-GCM failed")?;
    let mut body = Vec::with_capacity(16 + 4 + 1 + sender.len() + record.len());
    body.extend_from_slice(salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(u8::try_from(sender.len()).map_err(|_| "sender key too long")?);
    body.extend_from_slice(sender);
    body.extend_from_slice(&record);
    Ok(body)
}

fn post(
    url: &url::Url,
    body: &[u8],
    authorization: &str,
    allow_insecure: bool,
) -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
    let host = url.host_str().ok_or("push endpoint has no host")?;
    let port = url
        .port_or_known_default()
        .ok_or("push endpoint has no port")?;
    let address = (host, port)
        .to_socket_addrs()?
        .next()
        .ok_or("push endpoint did not resolve")?;
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(10))?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    let path = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    };
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nTTL: {TTL_SECONDS}\r\nUrgency: high\r\nContent-Encoding: aes128gcm\r\nContent-Type: application/octet-stream\r\nAuthorization: {authorization}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    let mut response = Vec::new();
    match url.scheme() {
        "https" => {
            let mut roots = rustls::RootCertStore::empty();
            for certificate in rustls_native_certs::load_native_certs().certs {
                let _ = roots.add(certificate);
            }
            let config = rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();
            let name = rustls::pki_types::ServerName::try_from(host.to_owned())?;
            let connection = rustls::ClientConnection::new(Arc::new(config), name)?;
            let mut tls = rustls::StreamOwned::new(connection, stream);
            tls.write_all(&request)?;
            tls.flush()?;
            read_head(&mut tls, &mut response)?;
        }
        "http" if allow_insecure => {
            let mut stream = stream;
            stream.write_all(&request)?;
            read_head(&mut stream, &mut response)?;
        }
        _ => return Err("push endpoint must be HTTPS".into()),
    }
    let line = String::from_utf8_lossy(&response);
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or("malformed push service response")?;
    Ok(status)
}

fn read_head(stream: &mut impl Read, response: &mut Vec<u8>) -> std::io::Result<()> {
    let mut buffer = [0; 512];
    while !response.windows(2).any(|window| window == b"\r\n") && response.len() < 4096 {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..read]);
    }
    Ok(())
}

/// Reliable-control endpoint: the transport supplies the authenticated player.
pub struct PushControl(pub Arc<PushService>);
impl MatchControlService for PushControl {
    fn handle(
        &self,
        match_id: &MatchId,
        context: ControlContext,
        payload: &[u8],
    ) -> Result<Vec<u8>, ControlServiceError> {
        let command: ControlCommand = serde_json::from_slice(payload)
            .map_err(|_| ControlServiceError::new("unsupported control request"))?;
        let result = match command {
            ControlCommand::Key {} => {
                return Ok(serde_json::to_vec(
                    &serde_json::json!({ "public_key": self.0.public_key() }),
                )
                .unwrap_or_default());
            }
            ControlCommand::Subscribe { subscription } => {
                self.0
                    .subscribe(match_id.as_str(), context.player_id, subscription)
            }
            ControlCommand::Unsubscribe { endpoint } => {
                self.0
                    .unsubscribe(match_id.as_str(), context.player_id, &endpoint)
            }
        };
        result
            .map(|()| br#"{"ok":true}"#.to_vec())
            .map_err(ControlServiceError::new)
    }
}

/// Polls authoritative phases of subscribed matches and delivers turn-start pushes.
pub async fn watch(host: LiveMatchHost<ScrabbleSimulation>, service: Arc<PushService>) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        for id in service.subscribed_matches() {
            let observation = match MatchId::new(id.clone()) {
                Ok(match_id) => match host
                    .inspect(&match_id, |runtime| {
                        runtime.snapshot().ok().map(|snapshot| snapshot.payload)
                    })
                    .await
                {
                    Ok(Some(payload)) => observe_payload(&payload),
                    Ok(None) => continue,
                    Err(game_server::LiveHostError::UnknownMatch) => Observation::Ended,
                    Err(_) => continue,
                },
                Err(_) => Observation::Ended,
            };
            for delivery in service.observe(&id, observation) {
                let service = Arc::clone(&service);
                tokio::task::spawn_blocking(move || {
                    if let Err(error) = service.deliver(&delivery, &turn_payload()) {
                        eprintln!("turn notification delivery failed: {error}");
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_8291_appendix_a_vector() {
        let decode = |text: &str| B64.decode(text).unwrap();
        let salt: [u8; 16] = decode("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let body = seal(
            &decode("kyrL1jIIOHEzg3sM2ZWRHDRB62YACZhhSlknJ672kSs"),
            &decode("BTBZMqHH6r4Tts7J_aSIgg"),
            &decode("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4"),
            &decode("BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8"),
            &salt,
            b"When I grow up, I want to be a watermelon",
        )
        .unwrap();
        assert_eq!(
            B64.encode(body),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }
}
