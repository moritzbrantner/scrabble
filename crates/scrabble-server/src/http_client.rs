//! The one outbound HTTP client (#214) for the account service and Web Push delivery:
//! pooled keep-alive connections, ring-backed rustls and the native root store loaded once.
use std::{sync::Arc, time::Duration};

/// Bounded connect time; each request sets its own overall timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub fn client() -> Result<reqwest::Client, Box<dyn std::error::Error + Send + Sync>> {
    let mut roots = rustls::RootCertStore::empty();
    for certificate in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(certificate);
    }
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

/// Reads at most `limit` body bytes; a longer body is an error.
pub async fn bounded_body(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err("response too large".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        body.extend_from_slice(&chunk);
        if body.len() > limit {
            return Err("response too large".into());
        }
    }
    Ok(body)
}
