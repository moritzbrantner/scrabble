//! Client of the shared Moenarch account service (#132; Decision 1.46).
//!
//! Scrabble never authenticates players itself. It depends only on the account
//! service's HTTP session contract (`moenarch.com` `services/accounts/docs/
//! session-contract.md`): the browser carries an opaque token in the `moenarch_session`
//! cookie, and `POST /v1/sessions/verify {token}` answers `{account_id, display_name,
//! session_id, expires_at}` or `401`. Positive answers are cached for at most 60 seconds
//! so a sign-out or revoked device takes effect within a minute.
use crate::account_link::is_account_id;
use ring::digest;
use serde::Deserialize;
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const SESSION_COOKIE: &str = "moenarch_session";
const CACHE_SECONDS: u64 = 60;
const MAX_CACHED: usize = 1024;
const MAX_TOKEN_BYTES: usize = 512;
const MAX_RESPONSE_BYTES: usize = 16 * 1024;
const TIMEOUT: Duration = Duration::from_secs(3);

/// A verified account. Only `account_id` keys linked data; the name is for display.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Account {
    pub account_id: String,
    pub display_name: String,
}

/// The account service could not answer (network, TLS or an unexpected response).
#[derive(Debug)]
pub struct Unavailable;

#[derive(Deserialize)]
struct Verified {
    account_id: String,
    display_name: String,
    expires_at: u64,
}

pub struct Accounts {
    origin: url::Url,
    allow_insecure: bool,
    cache: Mutex<HashMap<Vec<u8>, (Account, u64)>>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

impl Accounts {
    /// `origin` is the account service's public origin, e.g. `https://accounts.moenarch.com`.
    /// Plain `http` is accepted only with `allow_insecure` (local mode).
    pub fn new(origin: url::Url, allow_insecure: bool) -> Self {
        Self {
            origin,
            allow_insecure,
            cache: Mutex::new(HashMap::new()),
        }
    }
    /// Where a signed-out player signs in, recovers or manages devices.
    pub fn sign_in_url(&self) -> String {
        self.origin.as_str().to_owned()
    }
    /// Resolves a session token; `Ok(None)` means signed out (unknown, expired or revoked).
    /// Blocking: call it off the async executor.
    pub fn verify(&self, token: &str) -> Result<Option<Account>, Unavailable> {
        if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
            return Ok(None);
        }
        let key = digest::digest(&digest::SHA256, token.as_bytes())
            .as_ref()
            .to_vec();
        let current = now();
        if let Ok(cache) = self.cache.lock()
            && let Some((account, until)) = cache.get(&key)
            && *until > current
        {
            return Ok(Some(account.clone()));
        }
        let body =
            serde_json::to_vec(&serde_json::json!({ "token": token })).map_err(|_| Unavailable)?;
        let (status, response) = self
            .post("/v1/sessions/verify", &body)
            .map_err(|_| Unavailable)?;
        match status {
            200 => {
                let verified: Verified =
                    serde_json::from_slice(&response).map_err(|_| Unavailable)?;
                if !is_account_id(&verified.account_id) {
                    return Err(Unavailable);
                }
                let account = Account {
                    account_id: verified.account_id.to_ascii_lowercase(),
                    display_name: verified.display_name,
                };
                if let Ok(mut cache) = self.cache.lock() {
                    cache.retain(|_, (_, until)| *until > current);
                    if cache.len() >= MAX_CACHED {
                        cache.clear();
                    }
                    let until = (current + CACHE_SECONDS).min(verified.expires_at);
                    cache.insert(key, (account.clone(), until));
                }
                Ok(Some(account))
            }
            401 => Ok(None),
            _ => Err(Unavailable),
        }
    }

    fn post(
        &self,
        path: &str,
        body: &[u8],
    ) -> Result<(u16, Vec<u8>), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.origin.join(path)?;
        let host = url.host_str().ok_or("account service has no host")?;
        let port = url
            .port_or_known_default()
            .ok_or("account service has no port")?;
        let address = (host, port)
            .to_socket_addrs()?
            .next()
            .ok_or("account service did not resolve")?;
        let stream = TcpStream::connect_timeout(&address, TIMEOUT)?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.set_write_timeout(Some(TIMEOUT))?;
        let authority = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        let mut request = format!(
            "POST {} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            url.path(),
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
                read_bounded(&mut tls, &mut response)?;
            }
            "http" if self.allow_insecure => {
                let mut stream = stream;
                stream.write_all(&request)?;
                read_bounded(&mut stream, &mut response)?;
            }
            _ => return Err("account service must be HTTPS".into()),
        }
        parse_response(&response)
    }
}

fn read_bounded(stream: &mut impl Read, response: &mut Vec<u8>) -> std::io::Result<()> {
    let mut buffer = [0; 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => {
                response.extend_from_slice(&buffer[..read]);
                if response.len() > MAX_RESPONSE_BYTES {
                    return Err(std::io::Error::other("account response too large"));
                }
            }
            // A TLS peer that closes without close_notify still delivered its full body.
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        }
    }
}

type Parsed = Result<(u16, Vec<u8>), Box<dyn std::error::Error + Send + Sync>>;
fn parse_response(response: &[u8]) -> Parsed {
    let end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("malformed account service response")?;
    let head = std::str::from_utf8(&response[..end])?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or("malformed account service status")?;
    let mut body = response[end + 4..].to_vec();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "transfer-encoding" => return Err("unsupported account service framing".into()),
            "content-length" => {
                let length: usize = value.trim().parse()?;
                if body.len() < length {
                    return Err("truncated account service response".into());
                }
                body.truncate(length);
            }
            _ => {}
        }
    }
    Ok((status, body))
}

/// The `moenarch_session` value from a `Cookie` header, if present.
pub fn session_cookie(header: &str) -> Option<&str> {
    header.split(';').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name.trim() == SESSION_COOKIE).then(|| value.trim().trim_matches('"'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_cookie_is_found_among_others() {
        assert_eq!(
            session_cookie("a=1; moenarch_session=mns_abc; b=2"),
            Some("mns_abc")
        );
        assert_eq!(session_cookie("moenarch_session_x=1"), None);
    }

    #[test]
    fn responses_are_split_at_their_declared_length() {
        let (status, body) =
            parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}junk").unwrap();
        assert_eq!((status, body.as_slice()), (200, b"{}".as_slice()));
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n").is_err());
    }
}
