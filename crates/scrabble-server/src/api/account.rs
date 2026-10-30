//! Account-linked library routes (#132); cookie-authenticated, board origin only.
use super::{word_lists::word_list_error, *};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum LibraryKind {
    WordList,
    TileProfile,
    Layout,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AccountRequest {
    version: u8,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LinkRequest {
    version: u8,
    kind: LibraryKind,
    id: String,
    owner_capability: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReconnectRequest {
    version: u8,
    kind: LibraryKind,
    id: String,
}
pub(super) fn linked_body(linked: Vec<LinkedLibrary>) -> serde_json::Value {
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
pub(super) async fn account_route(
    request: &Request,
    games: &Games,
    caller: Caller,
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
        // Cookie-authenticated mutations come only from the board's own origin. Guest
        // origins pass the preflight above so the board can read this answer and stay
        // guest-only instead of reporting a failure.
        match caller {
            Caller::Board => {}
            Caller::Direct => return Err(Response::error(403, "origin-rejected")),
            Caller::Guest => return Err(Response::error(403, "guest-origin")),
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
        let account = accounts
            .verify(&token)
            .await
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
pub(super) fn kind_name(kind: LibraryKind) -> &'static str {
    match kind {
        LibraryKind::WordList => "wordList",
        LibraryKind::TileProfile => "tileProfile",
        LibraryKind::Layout => "layout",
    }
}
