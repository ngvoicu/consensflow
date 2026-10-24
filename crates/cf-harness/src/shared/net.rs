//! HTTP/1.1 to a peer on loopback, the system's `Loopback`
//! (`seams::loopback`): the request sent as its caller wrote it, the
//! reply's head, then its body within a size. A failure says whether a head
//! came: before one, `send` fails; after one, only the body can. A send its
//! caller no longer admits once its connection is up writes nothing. No
//! `accept-encoding` is asked for, so no body comes compressed.

use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::task::{Context, Poll};

use http_body_util::Full;
use hyper::body::{Body, Bytes, Incoming};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use url::{Position, Url};

use crate::contract::NOT_ADMITTED;
use crate::seams::loopback::{BodyFailed, Method, Request, FETCH_FAILED};

/// The connection a reply comes over, driven while its head and its body
/// are awaited: hyper's client needs it polled, and nothing here spawns.
type Connection = Pin<Box<dyn Future<Output = hyper::Result<()>>>>;

/// A reply whose head came.
pub struct Reply {
    status: u16,
    body: Incoming,
    connection: Option<Connection>,
}

/// Sends `request`, if `admits` says yes once its connection is up: its reply
/// once a head came, or why none did ([`NOT_ADMITTED`] where nothing of it was
/// written, as `admits` said no).
pub async fn send(request: Request, admits: &dyn Fn() -> bool) -> Result<Reply, String> {
    exchange(request, admits)
        .await
        .map_err(|failed| match failed {
            Failed::NotAdmitted => NOT_ADMITTED.to_owned(),
            Failed::Fetch => FETCH_FAILED.to_owned(),
        })
}

/// Why no head came.
enum Failed {
    /// `admits` said no: nothing of the request was written.
    NotAdmitted,
    Fetch,
}

/// The request sent and its head read, or what failed on the way. `admits` is
/// asked once nothing is left to wait for before the request is handed to the
/// connection, which writes it as it is next driven, in the same poll: the
/// connection and its handshake are awaited first.
async fn exchange(request: Request, admits: &dyn Fn() -> bool) -> Result<Reply, Failed> {
    let url = Url::parse(&request.url).map_err(|_| Failed::Fetch)?;
    let addresses = url.socket_addrs(|| None).map_err(|_| Failed::Fetch)?;
    let stream = TcpStream::connect(&*addresses)
        .await
        .map_err(|_| Failed::Fetch)?;
    let (mut sender, connection) = http1::handshake::<_, Full<Bytes>>(TokioIo::new(stream))
        .await
        .map_err(|_| Failed::Fetch)?;
    let mut connection: Option<Connection> = Some(Box::pin(connection));
    let host = &url[Position::BeforeHost..Position::BeforePath];
    let mut message = hyper::Request::builder()
        .method(match request.method {
            Method::Get => hyper::Method::GET,
            Method::Post => hyper::Method::POST,
        })
        .uri(&url[Position::BeforePath..Position::AfterQuery])
        .header(hyper::header::HOST, host);
    for (name, value) in &request.headers {
        message = message.header(name.as_str(), value.as_str());
    }
    let message = message
        .body(Full::new(Bytes::from(request.body.unwrap_or_default())))
        .map_err(|_| Failed::Fetch)?;
    if !admits() {
        return Err(Failed::NotAdmitted);
    }
    let mut asked = std::pin::pin!(sender.send_request(message));
    let head = poll_fn(|context| {
        if let Poll::Ready(head) = asked.as_mut().poll(context) {
            return Poll::Ready(head);
        }
        drive(&mut connection, context);
        Poll::Pending
    })
    .await
    .map_err(|_| Failed::Fetch)?;
    Ok(Reply {
        status: head.status().as_u16(),
        body: head.into_body(),
        connection,
    })
}

impl Reply {
    pub fn status(&self) -> u16 {
        self.status
    }

    /// Its body whole, at most `limit` bytes.
    pub async fn body(&mut self, limit: usize) -> Result<Vec<u8>, BodyFailed> {
        let mut bytes = Vec::new();
        poll_fn(|context| loop {
            match Pin::new(&mut self.body).poll_frame(context) {
                Poll::Ready(None) => return Poll::Ready(Ok(std::mem::take(&mut bytes))),
                Poll::Ready(Some(Err(_))) => return Poll::Ready(Err(BodyFailed::Cut)),
                Poll::Ready(Some(Ok(frame))) => {
                    if let Ok(data) = frame.into_data() {
                        if bytes.len() + data.len() > limit {
                            return Poll::Ready(Err(BodyFailed::TooLarge));
                        }
                        bytes.extend_from_slice(&data);
                    }
                }
                Poll::Pending => {
                    drive(&mut self.connection, context);
                    return Poll::Pending;
                }
            }
        })
        .await
    }
}

/// Polls the connection once, let go once it has ended: its end, broken or
/// not, shows in the head or the body that awaited it.
fn drive(connection: &mut Option<Connection>, context: &mut Context<'_>) {
    if let Some(running) = connection {
        if running.as_mut().poll(context).is_ready() {
            *connection = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Runs `test` against a peer that answers each connection with
    /// `answer`'s bytes, written as they are, and keeps what it was sent.
    fn against<T>(answer: &'static [u8], test: impl AsyncFnOnce(String) -> T) -> (T, Vec<u8>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
                let peer = async {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut asked = Vec::new();
                    let mut chunk = [0; 4096];
                    while !asked.windows(4).any(|end| end == b"\r\n\r\n") {
                        let count = socket.read(&mut chunk).await.unwrap();
                        asked.extend_from_slice(&chunk[..count]);
                    }
                    socket.write_all(answer).await.unwrap();
                    socket.shutdown().await.unwrap();
                    asked
                };
                let (answered, asked) = tokio::join!(test(base), peer);
                (answered, asked)
            })
    }

    fn get(url: String) -> Request {
        Request {
            method: Method::Get,
            url,
            headers: vec![("authorization".to_owned(), "Bearer t".to_owned())],
            body: None,
        }
    }

    /// `request` sent with nothing to ask whether it is admitted.
    async fn sent(request: Request) -> Result<Reply, String> {
        send(request, &|| true).await
    }

    /// Astra's review of 2026-10-16, round 5: the pane host admitted a native
    /// send, and the project closed while its connection was made, which the
    /// system makes in a wait of its own. It is asked once the connection is up,
    /// and a send it does not admit writes nothing.
    #[test]
    fn a_send_is_asked_whether_it_is_admitted_once_its_connection_is_up() {
        let (failed, asked) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let open = std::cell::Cell::new(true);
                let request = Request {
                    method: Method::Post,
                    url: format!("http://127.0.0.1:{port}/deliver"),
                    headers: Vec::new(),
                    body: Some(b"{\"text\":\"hello\"}".to_vec()),
                };
                // What came over the connection until it ended or a request's head
                // was whole; the connection then goes, answered with nothing.
                let peer = async {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut asked = Vec::new();
                    let mut chunk = [0; 4096];
                    while !asked.windows(4).any(|end| end == b"\r\n\r\n") {
                        let count = socket.read(&mut chunk).await.unwrap();
                        if count == 0 {
                            break;
                        }
                        asked.extend_from_slice(&chunk[..count]);
                    }
                    asked
                };
                // Polled once the send waits for its connection.
                let closing = async { open.set(false) };
                let admits = || open.get();
                let (sent, (), asked) = tokio::join!(send(request, &admits), closing, peer);
                (sent.err(), asked)
            });
        assert_eq!(failed.as_deref(), Some(NOT_ADMITTED));
        assert_eq!(String::from_utf8_lossy(&asked), "", "nothing was written");
    }

    #[test]
    fn a_reply_carries_its_status_and_its_body_whatever_the_status() {
        let ((status, body), asked) = against(
            b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 5\r\n\r\nno no",
            async |base| {
                let mut reply = sent(get(format!("{base}/session?directory=a+b%20c")))
                    .await
                    .unwrap();
                (reply.status(), reply.body(1024).await)
            },
        );
        assert_eq!(status, 401);
        assert_eq!(body.unwrap(), b"no no");
        let asked = String::from_utf8(asked).unwrap();
        assert!(
            asked.starts_with("GET /session?directory=a+b%20c HTTP/1.1\r\n"),
            "{asked}"
        );
        assert!(asked.contains("authorization: Bearer t\r\n"), "{asked}");
        assert!(
            !asked.to_ascii_lowercase().contains("accept-encoding"),
            "{asked}"
        );
    }

    #[test]
    fn a_chunked_body_is_read_whole() {
        let (body, _) = against(
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n",
            async |base| sent(get(format!("{base}/")) ).await.unwrap().body(1024).await,
        );
        assert_eq!(body.unwrap(), b"abcde");
    }

    #[test]
    fn a_body_past_its_size_is_too_large() {
        let (body, _) = against(
            b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\n\r\n0123456789",
            async |base| sent(get(format!("{base}/"))).await.unwrap().body(4).await,
        );
        assert_eq!(body.unwrap_err(), BodyFailed::TooLarge);
    }

    #[test]
    fn a_body_the_peer_ends_early_is_cut_after_its_head_came() {
        let ((status, body), _) = against(
            b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\n\r\n0123",
            async |base| {
                let mut reply = sent(get(format!("{base}/"))).await.unwrap();
                (reply.status(), reply.body(1024).await)
            },
        );
        assert_eq!(status, 200);
        assert_eq!(body, Err(BodyFailed::Cut));
    }

    #[test]
    fn a_post_carries_its_body_and_length() {
        let (_, asked) = against(b"HTTP/1.1 204 No Content\r\n\r\n", async |base| {
            let request = Request {
                method: Method::Post,
                url: format!("{base}/deliver"),
                headers: vec![("content-type".to_owned(), "application/json".to_owned())],
                body: Some(b"{\"ok\":true}".to_vec()),
            };
            sent(request).await.unwrap().status()
        });
        let asked = String::from_utf8(asked).unwrap();
        assert!(asked.starts_with("POST /deliver HTTP/1.1\r\n"), "{asked}");
        assert!(asked.contains("content-length: 11\r\n"), "{asked}");
    }

    #[test]
    fn no_peer_is_a_failure_before_any_head() {
        let failed = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let port = listener.local_addr().unwrap().port();
                drop(listener);
                sent(get(format!("http://127.0.0.1:{port}/"))).await.err()
            });
        assert_eq!(failed.as_deref(), Some("fetch failed"));
    }
}
