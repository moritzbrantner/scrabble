//! Guest board-layout library routes (#101).
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CreateLayout {
    version: u8,
    name: String,
    layout: serde_json::Value,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OwnerRequest {
    pub(super) version: u8,
    pub(super) owner_capability: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UpdateLayout {
    version: u8,
    owner_capability: String,
    name: String,
    layout: serde_json::Value,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CopyLayout {
    pub(super) version: u8,
    pub(super) name: String,
}
pub(super) fn layout_error(error: LayoutError) -> Response {
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
pub(super) fn view_body(view: &LayoutView) -> serde_json::Value {
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
pub(super) fn created_body(created: &Created) -> serde_json::Value {
    let mut body = view_body(&created.view);
    body["ownerCapability"] = serde_json::json!(created.owner_capability);
    body
}
pub(super) fn parse_layout(value: serde_json::Value) -> Result<Layout, Response> {
    serde_json::from_value(value).map_err(|error| {
        let mut reason = error.to_string();
        reason.truncate(reason.floor_char_boundary(200));
        layout_error(LayoutError::Invalid(reason))
    })
}
pub(super) async fn layout_route(request: &Request, games: &Games) -> Response {
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
pub(super) fn share_id_of(view: &LayoutView) -> Option<&str> {
    view.share_id.as_deref()
}
