//! The broker's HTTP: `GET /session`, `POST /deliver`, and the upgrade at `/`
//! that a TUI connects with. Loopback only, every request carrying the
//! launch's bearer token; routes are exact, and anything else is refused.

use std::rc::Rc;
use std::time::Duration;

use bytes::Bytes;
use cf_base::js;
use cf_proto::codex::{DeliveryReply, Refusal};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{
    HeaderMap, HeaderValue, AUTHORIZATION, CACHE_CONTROL, CONNECTION, CONTENT_TYPE, UPGRADE,
};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde::Serialize;
use subtle::ConstantTimeEq;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::WebSocketStream;

use super::delivery;
use super::pair::Pair;
use super::transport::{config, Io};
use super::Shared;

/// The most a delivery's body may be, streamed: 128 KiB.
const MAX_BODY: usize = 128 * 1024;
/// How long a request's headers may take to arrive, and then its body: five
/// seconds each (a connection waiting for its next request is held to the
/// first, as Node's keep-alive timeout held it).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// A connection ended with no answer: what `socket.destroy()` was.
#[derive(Debug, thiserror::Error)]
#[error("connection refused")]
struct Destroyed;

/// Takes every connection to the broker's port until the broker closes.
pub(super) async fn accept(shared: Rc<Shared>, listener: TcpListener) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            // Out of descriptors, or a connection that went before it was taken.
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        };
        let shared_for_connection = Rc::clone(&shared);
        shared.spawn(serve(shared_for_connection, stream));
    }
}

async fn serve(shared: Rc<Shared>, stream: TcpStream) {
    if stream.set_nodelay(true).is_err() {
        return;
    }
    let service = service_fn(move |request| route(Rc::clone(&shared), request));
    let connection = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(REQUEST_TIMEOUT)
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades();
    let _ = connection.await;
}

async fn route(
    shared: Rc<Shared>,
    mut request: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, Destroyed> {
    let authorized = authorized(&shared.bridge.token, request.headers());
    // The request's target as it was written, compared whole: a route is the
    // path alone, so a query, or a target with a host in it, is no route.
    let target = request.uri().to_string();
    if is_upgrade(request.headers()) {
        if !authorized || target != "/" || shared.is_closed() {
            return Err(Destroyed);
        }
        return Ok(upgrade(&shared, &mut request));
    }
    if !authorized {
        return Ok(refuse(StatusCode::UNAUTHORIZED, Refusal::Unauthorized));
    }
    if request.method() == Method::GET && target == "/session" {
        return Ok(json(StatusCode::OK, &shared.session()));
    }
    if request.method() != Method::POST || target != "/deliver" {
        return Ok(refuse(StatusCode::NOT_FOUND, Refusal::InvalidRecord));
    }
    Ok(deliver(&shared, request.into_body()).await)
}

/// `POST /deliver`.
async fn deliver(shared: &Rc<Shared>, body: Incoming) -> Response<Full<Bytes>> {
    let body = match tokio::time::timeout(REQUEST_TIMEOUT, read_body(body)).await {
        Ok(Ok(body)) => body,
        Ok(Err(Unread::TooLarge)) => {
            return refuse(StatusCode::PAYLOAD_TOO_LARGE, Refusal::InvalidRecord)
        }
        Ok(Err(Unread::Failed)) => return refuse(StatusCode::BAD_REQUEST, Refusal::InvalidRecord),
        Err(_) => return refuse(StatusCode::REQUEST_TIMEOUT, Refusal::InvalidRecord),
    };
    let Some(record) = delivery::read(&body, &shared.bridge.launch_id) else {
        return refuse(StatusCode::BAD_REQUEST, Refusal::InvalidRecord);
    };
    json(StatusCode::OK, &shared.deliver(&record).await)
}

enum Unread {
    /// More than [`MAX_BODY`] bytes came.
    TooLarge,
    Failed,
}

/// A request's body, up to [`MAX_BODY`] bytes: it is refused as soon as it is
/// known to be more, not once it is all there.
async fn read_body(mut body: Incoming) -> Result<Vec<u8>, Unread> {
    let mut read = Vec::new();
    while let Some(frame) = body.frame().await {
        let Ok(data) = frame.map_err(|_| Unread::Failed)?.into_data() else {
            continue;
        };
        if read.len() + data.len() > MAX_BODY {
            return Err(Unread::TooLarge);
        }
        read.extend_from_slice(&data);
    }
    Ok(read)
}

/// The bearer token as the launch gave it, compared in constant time.
fn authorized(token: &str, headers: &HeaderMap) -> bool {
    let actual = headers
        .get(AUTHORIZATION)
        .map_or(&[][..], HeaderValue::as_bytes);
    let expected = format!("Bearer {token}");
    actual.len() == expected.len() && bool::from(actual.ct_eq(expected.as_bytes()))
}

/// Whether the request asks to switch protocols.
fn is_upgrade(headers: &HeaderMap) -> bool {
    headers.contains_key(UPGRADE)
        && headers
            .get_all(CONNECTION)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
}

/// Answers the WebSocket handshake of a TUI, and proxies its connection. A
/// handshake is refused as the `ws` package refused one, with its statuses,
/// in its order: the method, the upgrade header, the key, the version (8 or
/// 13), the sub-protocols. The first sub-protocol the TUI offers is the one
/// answered, and no extension is: compression stays off.
fn upgrade(shared: &Rc<Shared>, request: &mut Request<Incoming>) -> Response<Full<Bytes>> {
    let headers = request.headers();
    let text = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    if request.method() != Method::GET {
        return abort(StatusCode::METHOD_NOT_ALLOWED, "Invalid HTTP method", None);
    }
    if !text("upgrade").is_some_and(|name| name.eq_ignore_ascii_case("websocket")) {
        return abort(StatusCode::BAD_REQUEST, "Invalid Upgrade header", None);
    }
    let Some(key) = text("sec-websocket-key").filter(|key| is_websocket_key(key)) else {
        return abort(
            StatusCode::BAD_REQUEST,
            "Missing or invalid Sec-WebSocket-Key header",
            None,
        );
    };
    let version = text("sec-websocket-version").map(js::number);
    if version != Some(13.0) && version != Some(8.0) {
        return abort(
            StatusCode::BAD_REQUEST,
            "Missing or invalid Sec-WebSocket-Version header",
            Some(("sec-websocket-version", "13, 8")),
        );
    }
    let offered = headers
        .get_all("sec-websocket-protocol")
        .iter()
        .map(|value| value.to_str().map_err(|_| ()))
        .collect::<Result<Vec<_>, _>>()
        .map(|values| values.join(", "));
    let protocol = match offered {
        Ok(offered) if !offered.is_empty() => match subprotocols(&offered) {
            Some(offered) => offered
                .first()
                .and_then(|name| HeaderValue::from_str(name).ok()),
            None => {
                return abort(
                    StatusCode::BAD_REQUEST,
                    "Invalid Sec-WebSocket-Protocol header",
                    None,
                )
            }
        },
        Ok(_) => None,
        Err(()) => {
            return abort(
                StatusCode::BAD_REQUEST,
                "Invalid Sec-WebSocket-Protocol header",
                None,
            )
        }
    };
    let accept = derive_accept_key(key.as_bytes());
    let upgraded = hyper::upgrade::on(&mut *request);
    let shared_for_pair = Rc::clone(shared);
    shared.spawn(async move {
        let Ok(upgraded) = upgraded.await else {
            return;
        };
        let io: Box<dyn Io> = Box::new(TokioIo::new(upgraded));
        let socket = WebSocketStream::from_raw_socket(io, Role::Server, Some(config())).await;
        Pair::start(&shared_for_pair, socket);
    });
    let mut switching = plain(StatusCode::SWITCHING_PROTOCOLS);
    let headers = switching.headers_mut();
    headers.insert(CONNECTION, HeaderValue::from_static("upgrade"));
    headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
    if let Ok(accept) = HeaderValue::from_str(&accept) {
        headers.insert("sec-websocket-accept", accept);
    }
    if let Some(protocol) = protocol {
        headers.insert("sec-websocket-protocol", protocol);
    }
    switching
}

/// Whether `key` is a `Sec-WebSocket-Key`: 16 bytes in base64.
fn is_websocket_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    bytes.len() == 24
        && bytes.ends_with(b"==")
        && bytes[..22]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
}

/// The sub-protocols a `Sec-WebSocket-Protocol` header offers, in order: tokens
/// separated by commas, with spaces or tabs around them but none before the
/// first; none for a header that is anything else, or names one twice.
fn subprotocols(header: &str) -> Option<Vec<&str>> {
    if header.starts_with([' ', '\t']) {
        return None;
    }
    let mut offered: Vec<&str> = Vec::new();
    for part in header.split(',') {
        let name = part.trim_matches([' ', '\t']);
        if !is_token(name) || offered.contains(&name) {
            return None;
        }
        offered.push(name);
    }
    Some(offered)
}

/// Whether `text` is a token as HTTP writes one: what a sub-protocol is named with.
fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// A handshake refused: `message` as the body, as `ws` writes one, and the
/// connection closed after it.
fn abort(
    status: StatusCode,
    message: &'static str,
    header: Option<(&'static str, &'static str)>,
) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from_static(message.as_bytes())));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(CONNECTION, HeaderValue::from_static("close"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/html"));
    if let Some((name, value)) = header {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

/// A JSON answer, never cached.
fn json<T: Serialize>(status: StatusCode, value: &T) -> Response<Full<Bytes>> {
    let body = serde_json::to_vec(value).unwrap_or_default();
    let mut response = Response::new(Full::new(Bytes::from(body)));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// An answer that says nothing was taken, and why.
fn refuse(status: StatusCode, reason: Refusal) -> Response<Full<Bytes>> {
    json(status, &DeliveryReply::Refused(reason))
}

fn plain(status: StatusCode) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::new()));
    *response.status_mut() = status;
    response
}
