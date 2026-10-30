//! The game API over axum (#213). Each resource keeps its own module; this module owns the
//! request envelope, body bounds, origin policy, CORS and the HTTP server itself.
mod account;
mod games;
mod layouts;
mod tile_profiles;
mod word_lists;

use self::{account::*, layouts::*, tile_profiles::*, word_lists::*};
use crate::account_link::{Linked, LinkedLibrary};
use crate::accounts::Accounts;
use crate::games::{
    ConfiguredError, CreateConfiguredGame, CreateGame, CreateRematch, GameOperationError, Games,
    unix_seconds,
};
use crate::layout_library::{Created, Layout, LayoutError, LayoutView, is_layout_id, is_share_id};
use crate::tile_profile_library::{TileProfileError, TileProfileView, is_tile_profile_id};
use crate::word_list_library::{WordListLibraryError, WordListView, is_word_list_id};
use axum::{
    Router,
    body::to_bytes,
    extract::{Request as HttpRequest, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::IntoResponse,
};
use game_server::MatchId;
use serde::Deserialize;
use std::{io, sync::Arc, time::Duration};
use tokio::net::TcpListener;

const MAX_BODY_BYTES: usize = 2048;
/// Layout documents (a 15×15 square is about 3.5 KiB) get their own bounded cap; every
/// other route keeps the original 2 KiB limit.
const MAX_LAYOUT_BODY_BYTES: usize = 32 * 1024;
/// Tile profiles have at most 64 rows, far below this cap.
const MAX_TILE_PROFILE_BODY_BYTES: usize = 16 * 1024;
fn is_tile_profile_path(path: &str) -> bool {
    path == "/tile-profiles" || path.starts_with("/tile-profiles/")
}
fn is_layout_path(path: &str) -> bool {
    path == "/layouts" || path.starts_with("/layouts/") || path.starts_with("/shared-layouts/")
}
/// Word-list routes admit the configured text bound after JSON escaping (each line
/// break may double) plus the small envelope; every other route keeps its own cap.
fn word_list_body_bytes(limits: &crate::word_list_library::WordListLimits) -> usize {
    2 * limits.max_bytes + 4096
}
fn is_word_list_path(path: &str) -> bool {
    path == "/word-lists"
        || path.starts_with("/word-lists/")
        || path.starts_with("/shared-word-lists/")
}
const MAX_CONCURRENT_REQUESTS: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// The request facts every resource route reads.
struct Request {
    method: String,
    path: String,
    origin: Option<String>,
    content_type: Option<String>,
    /// The shared account session token from the `moenarch_session` cookie (#132).
    session: Option<String>,
    body: Vec<u8>,
}
/// A JSON answer; errors are `{"error": code}` bodies.
struct Response {
    status: u16,
    body: String,
}
impl Response {
    fn error(status: u16, code: &str) -> Self {
        Self {
            status,
            body: format!("{{\"error\":\"{code}\"}}"),
        }
    }
}
fn operation_error(error: GameOperationError) -> Response {
    match error {
        GameOperationError::InvalidRequest => Response::error(400, "invalid-request"),
        GameOperationError::JoinClosed => Response::error(409, "join-closed"),
        GameOperationError::ExpiredMatch => Response::error(410, "expired-match"),
        GameOperationError::InactiveMatch => Response::error(410, "inactive-match"),
        GameOperationError::ExpiredRequest => Response::error(410, "expired-request"),
        GameOperationError::NotServing => Response::error(503, "not-serving"),
        GameOperationError::Draining => Response::error(503, "draining"),
        GameOperationError::AtCapacity => Response {
            status: 429,
            body: format!(
                "{{\"error\":\"at-capacity\",\"retryAfterSeconds\":{}}}",
                crate::games::CAPACITY_RETRY_SECONDS
            ),
        },
        GameOperationError::UnknownMatch => Response::error(404, "unknown-match"),
        GameOperationError::NotRetirable => Response::error(409, "not-retirable"),
        GameOperationError::BoardRejected => Response::error(403, "board-rejected"),
        GameOperationError::NotFinished => Response::error(409, "not-finished"),
        GameOperationError::RematchExists => Response::error(409, "rematch-exists"),
        GameOperationError::NoRematch => Response::error(404, "no-rematch"),
        GameOperationError::Internal => Response::error(500, "internal-error"),
    }
}
fn json_response(status: u16, body: &serde_json::Value) -> Response {
    Response {
        status,
        body: body.to_string(),
    }
}
fn parse_body<T: serde::de::DeserializeOwned>(
    request: &Request,
    version: impl Fn(&T) -> u8,
) -> Result<T, Response> {
    if request.content_type.as_deref() != Some("application/json") {
        return Err(Response::error(415, "json-required"));
    }
    let body: T = serde_json::from_slice(&request.body)
        .map_err(|_| Response::error(400, "invalid-request"))?;
    if version(&body) != 1 {
        return Err(Response::error(400, "invalid-request"));
    }
    Ok(body)
}

/// Browser origins the API answers (#206). The board origin may use every route; guest
/// origins (e.g. the GitHub Pages demo) may not use the cookie-authenticated account routes.
#[derive(Clone, Debug)]
pub struct BoardOrigins {
    pub board: String,
    pub guests: Vec<String>,
}
impl From<String> for BoardOrigins {
    fn from(board: String) -> Self {
        Self {
            board,
            guests: Vec::new(),
        }
    }
}
impl From<&str> for BoardOrigins {
    fn from(board: &str) -> Self {
        board.to_owned().into()
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Caller {
    /// No `Origin` header: not a browser page.
    Direct,
    Board,
    Guest,
}
impl BoardOrigins {
    fn caller(&self, origin: Option<&str>) -> Option<Caller> {
        match origin {
            None => Some(Caller::Direct),
            Some(origin) if origin == self.board => Some(Caller::Board),
            Some(origin) if self.guests.iter().any(|guest| guest == origin) => Some(Caller::Guest),
            Some(_) => None,
        }
    }
    /// The CORS origin to answer with: the caller's own when accepted, else the board's.
    fn cors(&self, origin: Option<&str>) -> &str {
        self.guests
            .iter()
            .find(|guest| Some(guest.as_str()) == origin)
            .unwrap_or(&self.board)
    }
}

/// Dispatches one accepted request to its resource.
async fn route(
    request: Request,
    games: &Games,
    origins: &BoardOrigins,
    accounts: Option<&Arc<Accounts>>,
) -> Response {
    let Some(caller) = origins.caller(request.origin.as_deref()) else {
        return Response::error(403, "origin-rejected");
    };
    if request.path.starts_with("/account/") {
        return account_route(&request, games, caller, accounts).await;
    }
    if is_layout_path(&request.path) {
        return layout_route(&request, games).await;
    }
    if is_word_list_path(&request.path) {
        return word_list_route(&request, games).await;
    }
    if is_tile_profile_path(&request.path) {
        return tile_profile_route(&request, games).await;
    }
    games::route(&request, games).await
}

struct Api {
    games: Arc<Games>,
    origins: BoardOrigins,
    accounts: Option<Arc<Accounts>>,
    word_list_body: usize,
}

/// One header value, or a 400 when a security-relevant header is repeated.
fn single(
    headers: &HeaderMap,
    name: header::HeaderName,
    error: &str,
) -> Result<Option<String>, Response> {
    let mut values = headers.get_all(&name).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(Response::error(400, error));
    }
    value
        .to_str()
        .map(|value| Some(value.trim().to_owned()))
        .map_err(|_| Response::error(400, "invalid-header"))
}
async fn envelope(request: HttpRequest, word_list_body: usize) -> Result<Request, Response> {
    let (parts, body) = request.into_parts();
    let path = parts.uri.path().to_owned();
    let origin = single(&parts.headers, header::ORIGIN, "duplicate-origin")?;
    let content_type = single(
        &parts.headers,
        header::CONTENT_TYPE,
        "duplicate-content-type",
    )?;
    let mut session = None;
    for cookie in parts.headers.get_all(header::COOKIE) {
        let cookie = cookie
            .to_str()
            .map_err(|_| Response::error(400, "invalid-header"))?;
        if let Some(token) = crate::accounts::session_cookie(cookie)
            && session.replace(token.to_owned()).is_some()
        {
            return Err(Response::error(400, "duplicate-session"));
        }
    }
    let limit = if is_layout_path(&path) {
        MAX_LAYOUT_BODY_BYTES
    } else if is_tile_profile_path(&path) {
        MAX_TILE_PROFILE_BODY_BYTES
    } else if is_word_list_path(&path) {
        word_list_body
    } else {
        MAX_BODY_BYTES
    };
    let body = to_bytes(body, limit)
        .await
        .map_err(|_| Response::error(413, "body-too-large"))?;
    if parts.method != Method::POST && !body.is_empty() {
        return Err(Response::error(400, "unexpected-body"));
    }
    Ok(Request {
        method: parts.method.as_str().to_owned(),
        path,
        origin,
        content_type,
        session,
        body: body.to_vec(),
    })
}
async fn handle(State(api): State<Arc<Api>>, request: HttpRequest) -> impl IntoResponse {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let cors = api.origins.cors(origin.as_deref()).to_owned();
    let operation = async {
        match envelope(request, api.word_list_body).await {
            Ok(request) => route(request, &api.games, &api.origins, api.accounts.as_ref()).await,
            Err(error) => error,
        }
    };
    let response = tokio::time::timeout(REQUEST_TIMEOUT, operation)
        .await
        .unwrap_or_else(|_| Response::error(408, "request-timeout"));
    let mut answer = (
        StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        response.body,
    )
        .into_response();
    let headers = answer.headers_mut();
    let fixed = [
        (header::CONTENT_TYPE, "application/json"),
        (header::CACHE_CONTROL, "no-store"),
        (header::ACCESS_CONTROL_ALLOW_CREDENTIALS, "true"),
        (header::VARY, "Origin"),
        (
            header::ACCESS_CONTROL_ALLOW_METHODS,
            "GET, POST, DELETE, OPTIONS",
        ),
        (header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type"),
    ];
    for (name, value) in fixed {
        headers.insert(name, HeaderValue::from_static(value));
    }
    if let Ok(cors) = HeaderValue::from_str(&cors) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, cors);
    }
    answer
}

pub async fn serve(
    listener: TcpListener,
    games: Arc<Games>,
    origins: BoardOrigins,
    accounts: Option<Arc<Accounts>>,
) -> io::Result<()> {
    let word_list_body = word_list_body_bytes(&games.word_list_limits());
    let api = Arc::new(Api {
        games,
        origins,
        accounts,
        word_list_body,
    });
    let router = Router::new()
        .fallback(handle)
        // Bodies are bounded per route in `envelope`; disable axum's blanket default.
        .layer(axum::extract::DefaultBodyLimit::disable())
        .layer(tower::limit::GlobalConcurrencyLimitLayer::new(
            MAX_CONCURRENT_REQUESTS,
        ))
        .with_state(api);
    axum::serve(listener, router).await
}

/// Retires due games every five seconds while the server runs.
pub async fn sweep(games: Arc<Games>) {
    let mut sweep = tokio::time::interval(Duration::from_secs(5));
    sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        sweep.tick().await;
        if let Ok(now) = unix_seconds() {
            games.sweep(now).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_word_list_body_cap_follows_the_configured_byte_bound() {
        let limits = crate::word_list_library::WordListLimits::default();
        assert_eq!(word_list_body_bytes(&limits), 2 * 1024 * 1024 + 4096);
        let small = crate::word_list_library::WordListLimits {
            max_bytes: 64,
            ..limits
        };
        assert_eq!(word_list_body_bytes(&small), 2 * 64 + 4096);
    }
}
