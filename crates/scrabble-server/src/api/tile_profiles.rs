//! Guest tile-profile library routes (#100).
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CreateTileProfile {
    version: u8,
    name: String,
    tiles: serde_json::Value,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UpdateTileProfile {
    version: u8,
    owner_capability: String,
    name: String,
    tiles: serde_json::Value,
}
pub(super) fn tile_profile_error(error: TileProfileError) -> Response {
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
pub(super) fn tile_profile_body(view: &TileProfileView) -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "profileId": view.profile_id,
        "name": view.name,
        "revision": view.revision,
        "identity": view.identity,
        "tiles": view.tiles,
    })
}
pub(super) fn parse_tiles(
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
pub(super) async fn tile_profile_route(request: &Request, games: &Games) -> Response {
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
