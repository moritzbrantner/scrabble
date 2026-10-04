//! Small, bounded HTTP request boundary. The upstream runtime owns placement and session fencing.
use crate::games::{CreateGame, GameOperationError, Games, unix_seconds};
use game_server::MatchId;
use std::{io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
};

const MAX_HEADER_BYTES: usize = 4096;
const MAX_BODY_BYTES: usize = 512;
const MAX_CONNECTIONS: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

struct Request {
    method: String,
    path: String,
    origin: Option<String>,
    content_type: Option<String>,
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

async fn read_request(stream: &mut TcpStream) -> Result<Request, Response> {
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
            _ => {}
        }
    }
    let length = match (method.as_str(), length) {
        ("POST", None) => return Err(Response::error(411, "length-required")),
        (_, length) => length.unwrap_or(0),
    };
    if length > MAX_BODY_BYTES {
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
        body,
    })
}
async fn route(request: Request, games: &Games, board_origin: &str) -> Response {
    if request
        .origin
        .as_deref()
        .is_some_and(|origin| origin != board_origin)
    {
        return Response::error(403, "origin-rejected");
    }
    let new_admission = request.path.ends_with("/join");
    let is_game = request.path.strip_prefix("/games/").and_then(|id| {
        MatchId::new(if new_admission {
            id.strip_suffix("/join")?
        } else {
            id
        })
        .ok()
    });
    if request.path != "/games" && is_game.is_none() {
        return Response::error(404, "not-found");
    }
    if request.method == "OPTIONS" {
        return Response {
            status: 204,
            body: String::new(),
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
            let create: CreateGame = match serde_json::from_slice(&request.body) {
                Ok(create) => create,
                Err(_) => return Response::error(400, "invalid-request"),
            };
            match games.create(&create, now).await {
                Ok(join) => match serde_json::to_string(&join) {
                    Ok(body) => Response { status: 200, body },
                    Err(_) => operation_error(GameOperationError::Internal),
                },
                Err(error) => operation_error(error),
            }
        }
        "GET" => match is_game {
            Some(id) => match if new_admission {
                games.check_join(&id, now).await
            } else {
                games.lookup(&id, now).await
            } {
                Ok(join) => match serde_json::to_string(&join) {
                    Ok(body) => Response { status: 200, body },
                    Err(_) => operation_error(GameOperationError::Internal),
                },
                Err(error) => operation_error(error),
            },
            None => Response::error(405, "method-not-allowed"),
        },
        "DELETE" if !new_admission => match is_game {
            Some(id) => match games.retire(&id, now).await {
                Ok(()) => Response {
                    status: 204,
                    body: String::new(),
                },
                Err(error) => operation_error(error),
            },
            None => Response::error(405, "method-not-allowed"),
        },
        _ => Response::error(405, "method-not-allowed"),
    }
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
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nAccess-Control-Allow-Origin: {origin}\r\nVary: Origin\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\n\r\n",
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
) -> io::Result<()> {
    let capacity = Arc::new(Semaphore::new(MAX_CONNECTIONS));
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
                requests.spawn(async move {
                    let _permit = permit;
                    let operation = async {
                        match read_request(&mut stream).await {
                            Ok(request) => route(request, &games, &origin).await,
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
        let status = match read_request(&mut stream).await {
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
            rejection(b"POST /games HTTP/1.1\r\nContent-Length: 513\r\n\r\n").await,
            413
        );
        let oversized = format!(
            "POST /games HTTP/1.1\r\nX: {}",
            "x".repeat(MAX_HEADER_BYTES)
        );
        assert_eq!(rejection(oversized.as_bytes()).await, 431);
    }
}
