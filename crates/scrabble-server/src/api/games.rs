//! Match creation, lookup, rematch and retirement plus the public profile catalogs.
use super::*;

pub(super) fn join_response(
    result: Result<crate::games::JoinInformation, GameOperationError>,
) -> Response {
    match result {
        Ok(join) => match serde_json::to_string(&join) {
            Ok(body) => Response { status: 200, body },
            Err(_) => operation_error(GameOperationError::Internal),
        },
        Err(error) => operation_error(error),
    }
}
/// Match and catalog routes; the origin was already accepted by the dispatcher.
pub(super) async fn route(request: &Request, games: &Games) -> Response {
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
