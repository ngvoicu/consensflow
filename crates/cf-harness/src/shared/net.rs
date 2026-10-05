//! HTTP/1.1 to a peer on loopback, the system's `Loopback`
//! (`seams::loopback`): the request sent as its caller wrote it, the
//! reply's head, then its body within a size. A failure says whether a head
//! came: before one, `send` fails; after one, only the body can. No
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

use crate::seams::loopback::{BodyFailed, Method, Request};

/// The connection a reply comes over, driven while its head and its body
/// are awaited: hyper's client needs it polled, and nothing here spawns.
type Connection = Pin<Box<dyn Future<Output = hyper::Result<()>>>>;

/// A reply whose head came.
pub struct Reply {
    status: u16,
    body: Incoming,
    connection: Option<Connection>,
}

/// Sends `request`: its reply once a head came, or why none did.
pub async fn send(request: Request) -> Result<Reply, String> {
    let url = Url::parse(&request.url).map_err(|failed| failed.to_string())?;
    let addresses = url
        .socket_addrs(|| None)
        .map_err(|failed| failed.to_string())?;
    let stream = TcpStream::connect(&*addresses)
        .await
        .map_err(|failed| failed.to_string())?;
    let (mut sender, connection) = http1::handshake::<_, Full<Bytes>>(TokioIo::new(stream))
        .await
        .map_err(|failed| failed.to_string())?;
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
        .map_err(|failed| failed.to_string())?;
    let mut asked = std::pin::pin!(sender.send_request(message));
    let head = poll_fn(|context| {
        if let Poll::Ready(head) = asked.as_mut().poll(context) {
            return Poll::Ready(head);
        }
        drive(&mut connection, context);
        Poll::Pending
    })
    .await
    .map_err(|failed| failed.to_string())?;
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
                Poll::Ready(Some(Err(failed))) => {
                    return Poll::Ready(Err(BodyFailed::Cut(failed.to_string())))
                }
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

    #[test]
    fn a_reply_carries_its_status_and_its_body_whatever_the_status() {
        let ((status, body), asked) = against(
            b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 5\r\n\r\nno no",
            async |base| {
                let mut reply = send(get(format!("{base}/session?directory=a+b%20c")))
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
            async |base| send(get(format!("{base}/")) ).await.unwrap().body(1024).await,
        );
        assert_eq!(body.unwrap(), b"abcde");
    }

    #[test]
    fn a_body_past_its_size_is_too_large() {
        let (body, _) = against(
            b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\n\r\n0123456789",
            async |base| send(get(format!("{base}/"))).await.unwrap().body(4).await,
        );
        assert_eq!(body.unwrap_err(), BodyFailed::TooLarge);
    }

    #[test]
    fn a_body_the_peer_ends_early_is_cut_after_its_head_came() {
        let ((status, body), _) = against(
            b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\n\r\n0123",
            async |base| {
                let mut reply = send(get(format!("{base}/"))).await.unwrap();
                (reply.status(), reply.body(1024).await)
            },
        );
        assert_eq!(status, 200);
        assert!(matches!(body, Err(BodyFailed::Cut(_))), "{body:?}");
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
            send(request).await.unwrap().status()
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
                send(get(format!("http://127.0.0.1:{port}/"))).await.err()
            });
        assert!(failed.is_some());
    }
}
