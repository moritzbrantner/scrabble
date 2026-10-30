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
    sync::Mutex,
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
    base: url::Url,
    allow_insecure: bool,
    client: reqwest::Client,
    cache: Mutex<HashMap<Vec<u8>, (Account, u64)>>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

impl Accounts {
    /// `base` is the account service's public base URL ending in `/`, e.g.
    /// `https://moenarch.com/accounts/`; its routes resolve below it.
    /// Plain `http` is accepted only with `allow_insecure` (local mode).
    pub fn new(base: url::Url, allow_insecure: bool, client: reqwest::Client) -> Self {
        Self {
            base,
            allow_insecure,
            client,
            cache: Mutex::new(HashMap::new()),
        }
    }
    /// Where a signed-out player signs in, recovers or manages devices.
    pub fn sign_in_url(&self) -> String {
        self.base.as_str().to_owned()
    }
    /// Resolves a session token; `Ok(None)` means signed out (unknown, expired or revoked).
    pub async fn verify(&self, token: &str) -> Result<Option<Account>, Unavailable> {
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
            .post("v1/sessions/verify", body)
            .await
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

    async fn post(
        &self,
        path: &str,
        body: Vec<u8>,
    ) -> Result<(u16, Vec<u8>), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.base.join(path)?;
        if !(url.scheme() == "https" || (self.allow_insecure && url.scheme() == "http")) {
            return Err("account service must be HTTPS".into());
        }
        let response = self
            .client
            .post(url.as_str())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ACCEPT, "application/json")
            .body(body)
            .timeout(TIMEOUT)
            .send()
            .await?;
        let status = response.status().as_u16();
        let body = crate::http_client::bounded_body(response, MAX_RESPONSE_BYTES).await?;
        Ok((status, body))
    }
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
}
