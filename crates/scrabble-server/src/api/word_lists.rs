//! Guest word-list library routes (#97).
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CreateWordList {
    version: u8,
    name: String,
    words: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UpdateWordList {
    version: u8,
    owner_capability: String,
    name: String,
    words: String,
}
pub(super) fn word_list_error(error: WordListLibraryError) -> Response {
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
pub(super) fn word_list_body(view: &WordListView) -> serde_json::Value {
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
pub(super) fn created_word_list_body(
    created: &crate::word_list_library::Created,
) -> serde_json::Value {
    let mut body = word_list_body(&created.view);
    // A new original is never shared; the capability goes only to its creator.
    if let Some(object) = body.as_object_mut() {
        object.remove("shareId");
    }
    body["ownerCapability"] = serde_json::json!(created.owner_capability);
    body
}
pub(super) fn owned_word_list_body(view: &WordListView, words: String) -> serde_json::Value {
    let mut body = word_list_body(view);
    body["words"] = serde_json::json!(words);
    body
}
/// Guest word-list library (#97). Mirrors the layout library: every mutation is a POST,
/// only the private owner capability manages an original, and a share id grants read
/// and copy only.
pub(super) async fn word_list_route(request: &Request, games: &Games) -> Response {
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
            if !crate::word_list_library::is_share_id(&share) {
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
