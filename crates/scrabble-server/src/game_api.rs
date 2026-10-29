//! Small, bounded HTTP request boundary. The upstream runtime owns placement and session fencing.
use crate::account_link::{Linked, LinkedLibrary};
use crate::accounts::Accounts;
use crate::games::{
    ConfiguredError, CreateConfiguredGame, CreateGame, CreateRematch, GameOperationError, Games,
    unix_seconds,
};
use crate::layout_library::{Created, Layout, LayoutError, LayoutView, is_layout_id, is_share_id};
use crate::tile_profile_library::{TileProfileError, TileProfileView, is_tile_profile_id};
use crate::word_list_library::{
    self as word_lists, WordListLibraryError, WordListView, is_word_list_id,
};
use game_server::MatchId;
use serde::Deserialize;
use std::{io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
};

const MAX_HEADER_BYTES: usize = 4096;
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
fn word_list_body_bytes(limits: &word_lists::WordListLimits) -> usize {
    2 * limits.max_bytes + 4096
}
fn is_word_list_path(path: &str) -> bool {
    path == "/word-lists"
        || path.starts_with("/word-lists/")
        || path.starts_with("/shared-word-lists/")
}
const MAX_CONNECTIONS: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

struct Request {
    method: String,
    path: String,
    origin: Option<String>,
    content_type: Option<String>,
    /// The shared account session token from the `moenarch_session` cookie (#132).
    session: Option<String>,
    body: Vec<u8>,
}
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
        GameOperationError::ExpiredRequest => Response::error(410, "expired-request"),
        GameOperationError::NotServing => Response::error(503, "not-serving"),
        GameOperationError::Draining => Response::error(503, "draining"),
        GameOperationError::AtCapacity => Response::error(429, "at-capacity"),
        GameOperationError::UnknownMatch => Response::error(404, "unknown-match"),
        GameOperationError::NotRetirable => Response::error(409, "not-retirable"),
        GameOperationError::BoardRejected => Response::error(403, "board-rejected"),
        GameOperationError::NotFinished => Response::error(409, "not-finished"),
        GameOperationError::RematchExists => Response::error(409, "rematch-exists"),
        GameOperationError::NoRematch => Response::error(404, "no-rematch"),
        GameOperationError::Internal => Response::error(500, "internal-error"),
    }
}
fn set_once(slot: &mut Option<String>, value: &str, error: &str) -> Result<(), Response> {
    if slot.replace(value.to_owned()).is_some() {
        Err(Response::error(400, error))
    } else {
        Ok(())
    }
}

async fn read_request(stream: &mut TcpStream, word_list_body: usize) -> Result<Request, Response> {
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        if bytes.len() >= MAX_HEADER_BYTES {
            return Err(Response::error(431, "header-too-large"));
        }
        let mut chunk = [0; 512];
        let limit = chunk.len().min(MAX_HEADER_BYTES - bytes.len());
        let read = stream
            .read(&mut chunk[..limit])
            .await
            .map_err(|_| Response::error(400, "incomplete-request"))?;
        if read == 0 {
            return Err(Response::error(400, "incomplete-request"));
        }
        bytes.extend_from_slice(&chunk[..read]);
    };
    let header = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| Response::error(400, "invalid-header"))?;
    let mut lines = header.split("\r\n");
    let parts: Vec<_> = lines
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    if parts.len() != 3 || !matches!(parts[2], "HTTP/1.0" | "HTTP/1.1") {
        return Err(Response::error(400, "invalid-request-line"));
    }
    let method = parts[0].to_owned();
    let path = parts[1].to_owned();
    let mut length = None;
    let mut origin = None;
    let mut content_type = None;
    let mut session = None;
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| Response::error(400, "invalid-header"))?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || value
                .bytes()
                .any(|byte| byte < 32 && byte != b'\t' || byte == 127)
        {
            return Err(Response::error(400, "invalid-header"));
        }
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "transfer-encoding" => return Err(Response::error(400, "unsupported-framing")),
            "content-length" => {
                if length.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err(Response::error(400, "invalid-length"));
                }
                length = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| Response::error(413, "body-too-large"))?,
                );
            }
            "origin" => set_once(&mut origin, value, "duplicate-origin")?,
            "content-type" => set_once(&mut content_type, value, "duplicate-content-type")?,
            "cookie" => {
                if let Some(token) = crate::accounts::session_cookie(value) {
                    set_once(&mut session, token, "duplicate-session")?;
                }
            }
            _ => {}
        }
    }
    let length = match (method.as_str(), length) {
        ("POST", None) => return Err(Response::error(411, "length-required")),
        (_, length) => length.unwrap_or(0),
    };
    let limit = if is_layout_path(&path) {
        MAX_LAYOUT_BODY_BYTES
    } else if is_tile_profile_path(&path) {
        MAX_TILE_PROFILE_BODY_BYTES
    } else if is_word_list_path(&path) {
        word_list_body
    } else {
        MAX_BODY_BYTES
    };
    if length > limit {
        return Err(Response::error(413, "body-too-large"));
    }
    if method != "POST" && length != 0 {
        return Err(Response::error(400, "unexpected-body"));
    }
    let mut body = bytes.split_off(header_end);
    if body.len() > length {
        return Err(Response::error(400, "trailing-bytes"));
    }
    let buffered = body.len();
    body.resize(length, 0);
    stream
        .read_exact(&mut body[buffered..])
        .await
        .map_err(|_| Response::error(400, "incomplete-body"))?;
    Ok(Request {
        method,
        path,
        origin,
        content_type,
        session,
        body,
    })
}
fn join_response(result: Result<crate::games::JoinInformation, GameOperationError>) -> Response {
    match result {
        Ok(join) => match serde_json::to_string(&join) {
            Ok(body) => Response { status: 200, body },
            Err(_) => operation_error(GameOperationError::Internal),
        },
        Err(error) => operation_error(error),
    }
}
async fn route(
    request: Request,
    games: &Games,
    board_origin: &str,
    accounts: Option<&Arc<Accounts>>,
) -> Response {
    if request
        .origin
        .as_deref()
        .is_some_and(|origin| origin != board_origin)
    {
        return Response::error(403, "origin-rejected");
    }
    if request.path.starts_with("/account/") {
        return account_route(&request, games, accounts).await;
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
    #[derive(Clone, Copy, PartialEq)]
    enum Resource {
        Match,
        Join,
        Rematch,
    }
    let is_game = request.path.strip_prefix("/games/").and_then(|rest| {
        let (id, resource) = if let Some(id) = rest.strip_suffix("/join") {
            (id, Resource::Join)
        } else if let Some(id) = rest.strip_suffix("/rematch") {
            (id, Resource::Rematch)
        } else {
            (rest, Resource::Match)
        };
        Some((MatchId::new(id).ok()?, resource))
    });
    if request.path != "/games"
        && request.path != "/profiles"
        && request.path != "/profiles/hex"
        && is_game.is_none()
    {
        return Response::error(404, "not-found");
    }
    if request.method == "OPTIONS" {
        return Response {
            status: 204,
            body: String::new(),
        };
    }
    if request.path == "/profiles" || request.path == "/profiles/hex" {
        return if request.method == "GET" {
            let catalog = if request.path == "/profiles/hex" {
                Some(games.hex_profiles())
            } else {
                games.profiles()
            };
            let Some(catalog) = catalog else {
                return Response::error(406, "legacy-square-catalog-unavailable");
            };
            match serde_json::to_string(&catalog) {
                Ok(body) => Response { status: 200, body },
                Err(_) => operation_error(GameOperationError::Internal),
            }
        } else {
            Response::error(405, "method-not-allowed")
        };
    }
    let now = match unix_seconds() {
        Ok(now) => now,
        Err(error) => return operation_error(error),
    };
    match request.method.as_str() {
        "POST" if request.path == "/games" => {
            if request.content_type.as_deref() != Some("application/json") {
                return Response::error(415, "json-required");
            }
            let version = serde_json::from_slice::<serde_json::Value>(&request.body)
                .ok()
                .and_then(|v| v.get("version").and_then(serde_json::Value::as_u64));
            if version == Some(3) {
                let create: CreateConfiguredGame = match serde_json::from_slice(&request.body) {
                    Ok(value) => value,
                    Err(_) => return Response::error(400, "invalid-request"),
                };
                return match games.create_configured(&create, now).await {
                    Ok(join) => join_response(Ok(join)),
                    Err(ConfiguredError::Game(error)) => operation_error(error),
                    Err(ConfiguredError::Conflict) => Response::error(409, "creation-conflict"),
                    Err(ConfiguredError::TimeControl(reason)) => Response {
                        status: 400,
                        body: serde_json::json!({"error":"invalid-time-control","reason":reason})
                            .to_string(),
                    },
                    Err(ConfiguredError::Selection(error)) => {
                        let body = match error {
                            crate::profile_catalog::SelectionError::Unknown(profile) => {
                                serde_json::json!({"error":"unknown-profile","profile":profile})
                            }
                            crate::profile_catalog::SelectionError::Incompatible(letters) => {
                                serde_json::json!({"error":"incompatible-profiles","letters":letters})
                            }
                            crate::profile_catalog::SelectionError::InvalidRules => {
                                serde_json::json!({"error":"invalid-match-rules"})
                            }
                        };
                        Response {
                            status: 400,
                            body: body.to_string(),
                        }
                    }
                };
            }
            let create: CreateGame = match serde_json::from_slice(&request.body) {
                Ok(create) => create,
                Err(_) => return Response::error(400, "invalid-request"),
            };
            join_response(games.create(&create, now).await)
        }
        "POST" => match is_game {
            Some((id, Resource::Rematch)) => {
                if request.content_type.as_deref() != Some("application/json") {
                    return Response::error(415, "json-required");
                }
                let rematch: CreateRematch = match serde_json::from_slice(&request.body) {
                    Ok(rematch) => rematch,
                    Err(_) => return Response::error(400, "invalid-request"),
                };
                join_response(games.rematch(&id, &rematch, now).await)
            }
            _ => Response::error(405, "method-not-allowed"),
        },
        "GET" => match is_game {
            Some((id, resource)) => join_response(match resource {
                Resource::Match => games.lookup(&id, now).await,
                Resource::Join => games.check_join(&id, now).await,
                Resource::Rematch => games.rematch_of(&id, now).await,
            }),
            None => Response::error(405, "method-not-allowed"),
        },
        "DELETE" => match is_game {
            Some((id, Resource::Match)) => match games.retire(&id, now).await {
                Ok(()) => Response {
                    status: 204,
                    body: String::new(),
                },
                Err(error) => operation_error(error),
            },
            _ => Response::error(405, "method-not-allowed"),
        },
        _ => Response::error(405, "method-not-allowed"),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateLayout {
    version: u8,
    name: String,
    layout: serde_json::Value,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnerRequest {
    version: u8,
    owner_capability: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateLayout {
    version: u8,
    owner_capability: String,
    name: String,
    layout: serde_json::Value,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CopyLayout {
    version: u8,
    name: String,
}
fn layout_error(error: LayoutError) -> Response {
    match error {
        LayoutError::Invalid(reason) => json_response(
            400,
            &serde_json::json!({"error":"invalid-layout","reason":reason}),
        ),
        LayoutError::OwnerRejected => Response::error(403, "layout-owner-rejected"),
        LayoutError::LinkedElsewhere => Response::error(409, "account-link-conflict"),
        LayoutError::NotAccountOwner => Response::error(403, "account-not-owner"),
        LayoutError::Unknown => Response::error(404, "unknown-layout"),
        LayoutError::Full => Response::error(429, "layout-library-full"),
        LayoutError::Internal => operation_error(GameOperationError::Internal),
    }
}
fn json_response(status: u16, body: &serde_json::Value) -> Response {
    Response {
        status,
        body: body.to_string(),
    }
}
fn view_body(view: &LayoutView) -> serde_json::Value {
    let mut body = serde_json::json!({
        "version": 1,
        "layoutId": view.layout_id,
        "name": view.name,
        "revision": view.revision,
        "board": view.board,
        "layout": view.layout,
    });
    if let Some(share) = &view.share_id {
        body["shareId"] = serde_json::json!(share);
    }
    body
}
fn created_body(created: &Created) -> serde_json::Value {
    let mut body = view_body(&created.view);
    body["ownerCapability"] = serde_json::json!(created.owner_capability);
    body
}
fn parse_layout(value: serde_json::Value) -> Result<Layout, Response> {
    serde_json::from_value(value).map_err(|error| {
        let mut reason = error.to_string();
        reason.truncate(reason.floor_char_boundary(200));
        layout_error(LayoutError::Invalid(reason))
    })
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateTileProfile {
    version: u8,
    name: String,
    tiles: serde_json::Value,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateTileProfile {
    version: u8,
    owner_capability: String,
    name: String,
    tiles: serde_json::Value,
}
fn tile_profile_error(error: TileProfileError) -> Response {
    match error {
        TileProfileError::Invalid(reason) => json_response(
            400,
            &serde_json::json!({"error":"invalid-tile-profile","reason":reason}),
        ),
        TileProfileError::OwnerRejected => Response::error(403, "tile-profile-owner-rejected"),
        TileProfileError::LinkedElsewhere => Response::error(409, "account-link-conflict"),
        TileProfileError::NotAccountOwner => Response::error(403, "account-not-owner"),
        TileProfileError::Unknown => Response::error(404, "unknown-tile-profile"),
        TileProfileError::Full => Response::error(429, "tile-profile-library-full"),
        TileProfileError::Internal => operation_error(GameOperationError::Internal),
    }
}
fn tile_profile_body(view: &TileProfileView) -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "profileId": view.profile_id,
        "name": view.name,
        "revision": view.revision,
        "identity": view.identity,
        "tiles": view.tiles,
    })
}
fn parse_tiles(
    value: serde_json::Value,
) -> Result<Vec<scrabble_game::ruleset::TileDefinition>, Response> {
    serde_json::from_value(value).map_err(|error| {
        let mut reason = error.to_string();
        reason.truncate(reason.floor_char_boundary(200));
        tile_profile_error(TileProfileError::Invalid(reason))
    })
}
/// Private guest tile-profile library (#100). Every mutation is a POST and only the
/// owner capability reads or manages an original; there are no share links.
async fn tile_profile_route(request: &Request, games: &Games) -> Response {
    if request.method == "OPTIONS" {
        return Response {
            status: 204,
            body: String::new(),
        };
    }
    let result = async {
        if request.method != "POST" {
            return Err(Response::error(405, "method-not-allowed"));
        }
        if request.path == "/tile-profiles" {
            let body: CreateTileProfile = parse_body(request, |b: &CreateTileProfile| b.version)?;
            let tiles = parse_tiles(body.tiles)?;
            let created = games
                .with_tile_profiles(move |library| library.create(&body.name, tiles))
                .await
                .map_err(operation_error)?
                .map_err(tile_profile_error)?;
            let mut response = tile_profile_body(&created.view);
            response["ownerCapability"] = serde_json::json!(created.owner_capability);
            return Ok(json_response(200, &response));
        }
        let rest = request
            .path
            .strip_prefix("/tile-profiles/")
            .unwrap_or_default();
        let Some((id, action)) = rest.split_once('/') else {
            return Err(Response::error(404, "not-found"));
        };
        if !is_tile_profile_id(id) {
            return Err(Response::error(404, "unknown-tile-profile"));
        }
        let id = id.to_owned();
        Ok(match action {
            "update" => {
                let body: UpdateTileProfile =
                    parse_body(request, |b: &UpdateTileProfile| b.version)?;
                let tiles = parse_tiles(body.tiles)?;
                let view = games
                    .with_tile_profiles(move |library| {
                        library.update(&id, &body.owner_capability, &body.name, tiles)
                    })
                    .await
                    .map_err(operation_error)?
                    .map_err(tile_profile_error)?;
                json_response(200, &tile_profile_body(&view))
            }
            "read" => {
                let body: OwnerRequest = parse_body(request, |b: &OwnerRequest| b.version)?;
                let view = games
                    .with_tile_profiles(move |library| library.read(&id, &body.owner_capability))
                    .await
                    .map_err(operation_error)?
                    .map_err(tile_profile_error)?;
                json_response(200, &tile_profile_body(&view))
            }
            "delete" => {
                let body: OwnerRequest = parse_body(request, |b: &OwnerRequest| b.version)?;
                games
                    .with_tile_profiles(move |library| library.delete(&id, &body.owner_capability))
                    .await
                    .map_err(operation_error)?
                    .map_err(tile_profile_error)?;
                Response {
                    status: 204,
                    body: String::new(),
                }
            }
            _ => return Err(Response::error(404, "not-found")),
        })
    }
    .await;
    result.unwrap_or_else(|response| response)
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
/// Guest board-layout library (#101). Every mutation is a POST; only the private owner
/// capability manages an original, and a share id grants read and copy only.
async fn layout_route(request: &Request, games: &Games) -> Response {
    if request.method == "OPTIONS" {
        return Response {
            status: 204,
            body: String::new(),
        };
    }
    let result = async {
        if request.path == "/layouts" {
            if request.method != "POST" {
                return Err(Response::error(405, "method-not-allowed"));
            }
            let body: CreateLayout = parse_body(request, |b: &CreateLayout| b.version)?;
            let layout = parse_layout(body.layout)?;
            let created = games
                .with_layouts(move |library| library.create(&body.name, layout))
                .await
                .map_err(operation_error)?
                .map_err(layout_error)?;
            return Ok(json_response(200, &created_body(&created)));
        }
        if let Some(rest) = request.path.strip_prefix("/shared-layouts/") {
            let (share, copy) = match rest.strip_suffix("/copy") {
                Some(share) => (share.to_owned(), true),
                None => (rest.to_owned(), false),
            };
            if !is_share_id(&share) {
                return Err(Response::error(404, "unknown-layout"));
            }
            return if copy {
                if request.method != "POST" {
                    return Err(Response::error(405, "method-not-allowed"));
                }
                let body: CopyLayout = parse_body(request, |b: &CopyLayout| b.version)?;
                let created = games
                    .with_layouts(move |library| library.copy(&share, &body.name))
                    .await
                    .map_err(operation_error)?
                    .map_err(layout_error)?;
                Ok(json_response(200, &created_body(&created)))
            } else {
                if request.method != "GET" {
                    return Err(Response::error(405, "method-not-allowed"));
                }
                let view = games
                    .with_layouts(move |library| library.shared(&share))
                    .await
                    .map_err(operation_error)?
                    .map_err(layout_error)?;
                let mut body = view_body(&view);
                // The public share never names the private original.
                if let Some(object) = body.as_object_mut() {
                    object.remove("layoutId");
                }
                body["shareId"] = serde_json::json!(share_id_of(&view));
                Ok(json_response(200, &body))
            };
        }
        let rest = request.path.strip_prefix("/layouts/").unwrap_or_default();
        let Some((id, action)) = rest.split_once('/') else {
            return Err(Response::error(404, "not-found"));
        };
        if !is_layout_id(id) {
            return Err(Response::error(404, "unknown-layout"));
        }
        if !matches!(action, "read" | "update" | "delete" | "share" | "unshare") {
            return Err(Response::error(404, "not-found"));
        }
        if request.method != "POST" {
            return Err(Response::error(405, "method-not-allowed"));
        }
        let id = id.to_owned();
        if action == "update" {
            let body: UpdateLayout = parse_body(request, |b: &UpdateLayout| b.version)?;
            let layout = parse_layout(body.layout)?;
            let view = games
                .with_layouts(move |library| {
                    library.update(&id, &body.owner_capability, &body.name, layout)
                })
                .await
                .map_err(operation_error)?
                .map_err(layout_error)?;
            return Ok(json_response(200, &view_body(&view)));
        }
        let body: OwnerRequest = parse_body(request, |b: &OwnerRequest| b.version)?;
        let capability = body.owner_capability;
        Ok(match action {
            "read" => {
                let view = games
                    .with_layouts(move |library| library.read(&id, &capability))
                    .await
                    .map_err(operation_error)?
                    .map_err(layout_error)?;
                json_response(200, &view_body(&view))
            }
            "share" => {
                let share = games
                    .with_layouts(move |library| library.share(&id, &capability))
                    .await
                    .map_err(operation_error)?
                    .map_err(layout_error)?;
                json_response(200, &serde_json::json!({"version":1,"shareId":share}))
            }
            "unshare" => {
                games
                    .with_layouts(move |library| library.unshare(&id, &capability))
                    .await
                    .map_err(operation_error)?
                    .map_err(layout_error)?;
                Response {
                    status: 204,
                    body: String::new(),
                }
            }
            _ => {
                games
                    .with_layouts(move |library| library.delete(&id, &capability))
                    .await
                    .map_err(operation_error)?
                    .map_err(layout_error)?;
                Response {
                    status: 204,
                    body: String::new(),
                }
            }
        })
    }
    .await;
    result.unwrap_or_else(|response| response)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateWordList {
    version: u8,
    name: String,
    words: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateWordList {
    version: u8,
    owner_capability: String,
    name: String,
    words: String,
}
fn word_list_error(error: WordListLibraryError) -> Response {
    match error {
        WordListLibraryError::Invalid { reason, line } => {
            let mut body = serde_json::json!({"error":"invalid-word-list","reason":reason});
            if let Some(line) = line {
                body["line"] = serde_json::json!(line);
            }
            json_response(400, &body)
        }
        WordListLibraryError::TooLarge(reason) => json_response(
            413,
            &serde_json::json!({"error":"word-list-too-large","reason":reason}),
        ),
        WordListLibraryError::OwnerRejected => Response::error(403, "word-list-owner-rejected"),
        WordListLibraryError::LinkedElsewhere => Response::error(409, "account-link-conflict"),
        WordListLibraryError::NotAccountOwner => Response::error(403, "account-not-owner"),
        WordListLibraryError::Unknown => Response::error(404, "unknown-word-list"),
        WordListLibraryError::Full => Response::error(429, "word-list-library-full"),
        WordListLibraryError::Internal => operation_error(GameOperationError::Internal),
    }
}
fn word_list_body(view: &WordListView) -> serde_json::Value {
    let mut body = serde_json::json!({
        "version": 1,
        "wordListId": view.word_list_id,
        "name": view.name,
        "revision": view.revision,
        "wordList": view.word_list,
        "wordCount": view.word_count,
    });
    if let Some(share) = &view.share_id {
        body["shareId"] = serde_json::json!(share);
    }
    body
}
fn created_word_list_body(created: &word_lists::Created) -> serde_json::Value {
    let mut body = word_list_body(&created.view);
    // A new original is never shared; the capability goes only to its creator.
    if let Some(object) = body.as_object_mut() {
        object.remove("shareId");
    }
    body["ownerCapability"] = serde_json::json!(created.owner_capability);
    body
}
fn owned_word_list_body(view: &WordListView, words: String) -> serde_json::Value {
    let mut body = word_list_body(view);
    body["words"] = serde_json::json!(words);
    body
}
/// Guest word-list library (#97). Mirrors the layout library: every mutation is a POST,
/// only the private owner capability manages an original, and a share id grants read
/// and copy only.
async fn word_list_route(request: &Request, games: &Games) -> Response {
    if request.method == "OPTIONS" {
        return Response {
            status: 204,
            body: String::new(),
        };
    }
    let result = async {
        if request.path == "/word-lists/limits" {
            if request.method != "GET" {
                return Err(Response::error(405, "method-not-allowed"));
            }
            let limits = games.word_list_limits();
            return Ok(json_response(
                200,
                &serde_json::json!({"version":1,"maxBytes":limits.max_bytes,
                    "maxWords":limits.max_words,"maxLists":limits.max_lists,
                    "maxVersions":limits.max_versions,"maxStoredBytes":limits.max_stored_bytes}),
            ));
        }
        if request.path == "/word-lists" {
            if request.method != "POST" {
                return Err(Response::error(405, "method-not-allowed"));
            }
            let body: CreateWordList = parse_body(request, |b: &CreateWordList| b.version)?;
            let created = games
                .with_word_lists(move |library| library.create(&body.name, &body.words))
                .await
                .map_err(operation_error)?
                .map_err(word_list_error)?;
            return Ok(json_response(200, &created_word_list_body(&created)));
        }
        if let Some(rest) = request.path.strip_prefix("/shared-word-lists/") {
            let (share, copy) = match rest.strip_suffix("/copy") {
                Some(share) => (share.to_owned(), true),
                None => (rest.to_owned(), false),
            };
            if !word_lists::is_share_id(&share) {
                return Err(Response::error(404, "unknown-word-list"));
            }
            return if copy {
                if request.method != "POST" {
                    return Err(Response::error(405, "method-not-allowed"));
                }
                let body: CopyLayout = parse_body(request, |b: &CopyLayout| b.version)?;
                let created = games
                    .with_word_lists(move |library| library.copy(&share, &body.name))
                    .await
                    .map_err(operation_error)?
                    .map_err(word_list_error)?;
                Ok(json_response(200, &created_word_list_body(&created)))
            } else {
                if request.method != "GET" {
                    return Err(Response::error(405, "method-not-allowed"));
                }
                let view = games
                    .with_word_lists(move |library| library.shared(&share))
                    .await
                    .map_err(operation_error)?
                    .map_err(word_list_error)?;
                let mut body = word_list_body(&view);
                // The public share never names the private original.
                if let Some(object) = body.as_object_mut() {
                    object.remove("wordListId");
                }
                Ok(json_response(200, &body))
            };
        }
        let rest = request
            .path
            .strip_prefix("/word-lists/")
            .unwrap_or_default();
        let Some((id, action)) = rest.split_once('/') else {
            return Err(Response::error(404, "not-found"));
        };
        if !is_word_list_id(id) {
            return Err(Response::error(404, "unknown-word-list"));
        }
        if !matches!(action, "read" | "update" | "delete" | "share" | "unshare") {
            return Err(Response::error(404, "not-found"));
        }
        if request.method != "POST" {
            return Err(Response::error(405, "method-not-allowed"));
        }
        let id = id.to_owned();
        if action == "update" {
            let body: UpdateWordList = parse_body(request, |b: &UpdateWordList| b.version)?;
            let (view, words) = games
                .with_word_lists(move |library| {
                    library.update(&id, &body.owner_capability, &body.name, &body.words)
                })
                .await
                .map_err(operation_error)?
                .map_err(word_list_error)?;
            return Ok(json_response(200, &owned_word_list_body(&view, words)));
        }
        let body: OwnerRequest = parse_body(request, |b: &OwnerRequest| b.version)?;
        let capability = body.owner_capability;
        let no_content = || Response {
            status: 204,
            body: String::new(),
        };
        Ok(match action {
            "read" => {
                let (view, words) = games
                    .with_word_lists(move |library| library.read(&id, &capability))
                    .await
                    .map_err(operation_error)?
                    .map_err(word_list_error)?;
                json_response(200, &owned_word_list_body(&view, words))
            }
            "share" => {
                let share = games
                    .with_word_lists(move |library| library.share(&id, &capability))
                    .await
                    .map_err(operation_error)?
                    .map_err(word_list_error)?;
                json_response(200, &serde_json::json!({"version":1,"shareId":share}))
            }
            "unshare" => {
                games
                    .with_word_lists(move |library| library.unshare(&id, &capability))
                    .await
                    .map_err(operation_error)?
                    .map_err(word_list_error)?;
                no_content()
            }
            _ => {
                games
                    .with_word_lists(move |library| library.delete(&id, &capability))
                    .await
                    .map_err(operation_error)?
                    .map_err(word_list_error)?;
                no_content()
            }
        })
    }
    .await;
    result.unwrap_or_else(|response| response)
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum LibraryKind {
    WordList,
    TileProfile,
    Layout,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AccountRequest {
    version: u8,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LinkRequest {
    version: u8,
    kind: LibraryKind,
    id: String,
    owner_capability: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReconnectRequest {
    version: u8,
    kind: LibraryKind,
    id: String,
}
fn linked_body(linked: Vec<LinkedLibrary>) -> serde_json::Value {
    linked
        .into_iter()
        .map(|library| serde_json::json!({"id": library.id, "name": library.name}))
        .collect()
}
/// Optional account linking of guest-owned libraries (#132; Decision 1.46). Every call
/// verifies the `moenarch_session` cookie with the shared account service; the browser
/// never names an account. Scrabble stays the authority for the libraries themselves.
///
/// - `POST /account/session` `{version}` -> `{version,accountId,displayName}`.
/// - `POST /account/link` `{version,kind,id,ownerCapability}` -> `{version,kind,id,linked}`
///   where `linked` is `"new"` or `"existing"` (idempotent for the same account).
/// - `POST /account/libraries` `{version}` -> `{version,wordLists,tileProfiles,layouts}`,
///   each a list of `{id,name}` linked to the signed-in account.
/// - `POST /account/reconnect` `{version,kind,id}` -> `{version,kind,id,ownerCapability}`:
///   a new device capability for a library linked to the signed-in account.
///
/// Errors: 401 `signed-out` with `signIn` (the account page); 403 `account-not-owner`;
/// 409 `account-link-conflict` (linked to another account); 503 `accounts-unavailable`.
async fn account_route(
    request: &Request,
    games: &Games,
    accounts: Option<&Arc<Accounts>>,
) -> Response {
    if request.method == "OPTIONS" {
        return Response {
            status: 204,
            body: String::new(),
        };
    }
    let result = async {
        if request.method != "POST" {
            return Err(Response::error(405, "method-not-allowed"));
        }
        if !matches!(
            request.path.as_str(),
            "/account/session" | "/account/link" | "/account/libraries" | "/account/reconnect"
        ) {
            return Err(Response::error(404, "not-found"));
        }
        // Cookie-authenticated mutations come only from the board's own origin.
        if request.origin.is_none() {
            return Err(Response::error(403, "origin-rejected"));
        }
        let accounts = accounts.ok_or_else(|| Response::error(503, "accounts-unavailable"))?;
        let link = if request.path == "/account/link" {
            Some(parse_body(request, |b: &LinkRequest| b.version)?)
        } else {
            None
        };
        let reconnect = if request.path == "/account/reconnect" {
            Some(parse_body(request, |b: &ReconnectRequest| b.version)?)
        } else {
            None
        };
        if link.is_none() && reconnect.is_none() {
            parse_body(request, |b: &AccountRequest| b.version)?;
        }
        let signed_out = || {
            json_response(
                401,
                &serde_json::json!({"error":"signed-out","signIn":accounts.sign_in_url()}),
            )
        };
        let token = request.session.clone().ok_or_else(signed_out)?;
        let verifier = Arc::clone(accounts);
        let account = tokio::task::spawn_blocking(move || verifier.verify(&token))
            .await
            .map_err(|_| operation_error(GameOperationError::Internal))?
            .map_err(|_| Response::error(503, "accounts-unavailable"))?
            .ok_or_else(signed_out)?;
        let account_id = account.account_id.clone();
        if let Some(body) = link {
            let LinkRequest {
                kind,
                id,
                owner_capability,
                ..
            } = body;
            let response_id = id.clone();
            let linked = match kind {
                LibraryKind::WordList => games
                    .with_word_lists(move |l| l.link(&id, &owner_capability, &account_id))
                    .await
                    .map_err(operation_error)?
                    .map_err(word_list_error)?,
                LibraryKind::TileProfile => games
                    .with_tile_profiles(move |l| l.link(&id, &owner_capability, &account_id))
                    .await
                    .map_err(operation_error)?
                    .map_err(tile_profile_error)?,
                LibraryKind::Layout => games
                    .with_layouts(move |l| l.link(&id, &owner_capability, &account_id))
                    .await
                    .map_err(operation_error)?
                    .map_err(layout_error)?,
            };
            return Ok(json_response(
                200,
                &serde_json::json!({
                    "version": 1,
                    "kind": kind_name(kind),
                    "id": response_id,
                    "linked": if linked == Linked::New { "new" } else { "existing" },
                }),
            ));
        }
        if let Some(body) = reconnect {
            let ReconnectRequest { kind, id, .. } = body;
            let response_id = id.clone();
            let capability = match kind {
                LibraryKind::WordList => games
                    .with_word_lists(move |l| l.reconnect(&id, &account_id))
                    .await
                    .map_err(operation_error)?
                    .map_err(word_list_error)?,
                LibraryKind::TileProfile => games
                    .with_tile_profiles(move |l| l.reconnect(&id, &account_id))
                    .await
                    .map_err(operation_error)?
                    .map_err(tile_profile_error)?,
                LibraryKind::Layout => games
                    .with_layouts(move |l| l.reconnect(&id, &account_id))
                    .await
                    .map_err(operation_error)?
                    .map_err(layout_error)?,
            };
            return Ok(json_response(
                200,
                &serde_json::json!({
                    "version": 1,
                    "kind": kind_name(kind),
                    "id": response_id,
                    "ownerCapability": capability,
                }),
            ));
        }
        if request.path == "/account/libraries" {
            let words = account_id.clone();
            let tiles = account_id.clone();
            let word_lists = games
                .with_word_lists(move |l| l.linked(&words))
                .await
                .map_err(operation_error)?
                .map_err(word_list_error)?;
            let tile_profiles = games
                .with_tile_profiles(move |l| l.linked(&tiles))
                .await
                .map_err(operation_error)?
                .map_err(tile_profile_error)?;
            let layouts = games
                .with_layouts(move |l| l.linked(&account_id))
                .await
                .map_err(operation_error)?
                .map_err(layout_error)?;
            return Ok(json_response(
                200,
                &serde_json::json!({
                    "version": 1,
                    "wordLists": linked_body(word_lists),
                    "tileProfiles": linked_body(tile_profiles),
                    "layouts": linked_body(layouts),
                }),
            ));
        }
        Ok(json_response(
            200,
            &serde_json::json!({
                "version": 1,
                "accountId": account.account_id,
                "displayName": account.display_name,
            }),
        ))
    }
    .await;
    result.unwrap_or_else(|response| response)
}
fn kind_name(kind: LibraryKind) -> &'static str {
    match kind {
        LibraryKind::WordList => "wordList",
        LibraryKind::TileProfile => "tileProfile",
        LibraryKind::Layout => "layout",
    }
}
fn share_id_of(view: &LayoutView) -> Option<&str> {
    view.share_id.as_deref()
}
async fn write_response(
    stream: &mut TcpStream,
    response: Response,
    origin: &str,
) -> io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        500 => "Internal Server Error",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nAccess-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Credentials: true\r\nVary: Origin\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\n\r\n",
        response.status,
        response.body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(response.body.as_bytes()).await?;
    stream.shutdown().await
}
pub async fn serve(
    listener: TcpListener,
    games: Arc<Games>,
    board_origin: String,
    accounts: Option<Arc<Accounts>>,
) -> io::Result<()> {
    let capacity = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let word_list_body = word_list_body_bytes(&games.word_list_limits());
    let mut requests = JoinSet::new();
    let mut sweep = tokio::time::interval(Duration::from_secs(5));
    sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = sweep.tick() => { if let Ok(now) = unix_seconds() { games.sweep(now).await; } },
            _ = requests.join_next(), if !requests.is_empty() => {},
            accepted = listener.accept() => {
                let (mut stream, _) = accepted?;
                let Ok(permit) = Arc::clone(&capacity).try_acquire_owned() else { continue; };
                let games = Arc::clone(&games);
                let origin = board_origin.clone();
                let accounts = accounts.clone();
                requests.spawn(async move {
                    let _permit = permit;
                    let operation = async {
                        match read_request(&mut stream, word_list_body).await {
                            Ok(request) => route(request, &games, &origin, accounts.as_ref()).await,
                            Err(error) => error,
                        }
                    };
                    let response = tokio::time::timeout(REQUEST_TIMEOUT, operation).await
                        .unwrap_or_else(|_| Response::error(408, "request-timeout"));
                    let _ = tokio::time::timeout(REQUEST_TIMEOUT, write_response(&mut stream, response, &origin)).await;
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const MAX_WORD_LIST_BODY_BYTES: usize = 2 * 1024 * 1024 + 4096;
    #[test]
    fn the_word_list_body_cap_follows_the_configured_byte_bound() {
        let limits = word_lists::WordListLimits::default();
        assert_eq!(word_list_body_bytes(&limits), MAX_WORD_LIST_BODY_BYTES);
        let small = word_lists::WordListLimits {
            max_bytes: 64,
            ..limits
        };
        assert_eq!(word_list_body_bytes(&small), 2 * 64 + 4096);
    }
    async fn rejection(request: &[u8]) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let bytes = request.to_vec();
        let sender = tokio::spawn(async move {
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream.write_all(&bytes).await.unwrap();
            stream.shutdown().await.unwrap();
        });
        let (mut stream, _) = listener.accept().await.unwrap();
        let status = match read_request(&mut stream, MAX_WORD_LIST_BODY_BYTES).await {
            Err(error) => error.status,
            Ok(_) => panic!("malformed request accepted"),
        };
        sender.await.unwrap();
        status
    }
    #[tokio::test]
    async fn malformed_or_unbounded_http_framing_is_rejected_before_game_operations() {
        for request in [
            "POST /games HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n",
            "POST /games HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
            "POST /games HTTP/1.1\r\nContent-Length: 0\r\nOrigin: one\r\nOrigin: two\r\n\r\n",
            "POST /games HTTP/1.1\r\nContent-Length: 0\r\nContent-Type: application/json\r\nContent-Type: application/json\r\n\r\n",
            "POST /games HTTP/1.1\r\nContent-Length: 0\r\n\r\n{}",
        ] {
            assert_eq!(rejection(request.as_bytes()).await, 400);
        }
        assert_eq!(rejection(b"POST /games HTTP/1.1\r\n\r\n").await, 411);
        assert_eq!(
            rejection(
                format!(
                    "POST /games HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                    MAX_BODY_BYTES + 1
                )
                .as_bytes()
            )
            .await,
            413
        );
        // Only word-list routes admit their larger published bound, and it stays bounded.
        assert_eq!(
            rejection(
                format!(
                    "POST /word-lists HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                    MAX_WORD_LIST_BODY_BYTES + 1
                )
                .as_bytes()
            )
            .await,
            413
        );
        assert_eq!(
            rejection(
                format!(
                    "POST /layouts HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                    MAX_LAYOUT_BODY_BYTES + 1
                )
                .as_bytes()
            )
            .await,
            413
        );
        let oversized = format!(
            "POST /games HTTP/1.1\r\nX: {}",
            "x".repeat(MAX_HEADER_BYTES)
        );
        assert_eq!(rejection(oversized.as_bytes()).await, 431);
    }
}
