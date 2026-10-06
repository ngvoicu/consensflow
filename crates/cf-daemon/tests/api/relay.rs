//! What a run of `cf` sends, written down as it passes: a relay on a port of
//! loopback of its own that the run is told is the API, which forwards every
//! byte to the API under test and every byte of the answer back, and keeps each
//! request the run wrote.
//!
//! The API cannot say what it was sent. A handler is given the path, the bearer
//! and a body it reads when it chooses (`Request` holds no query to list, no
//! content type, and no body that a route refused before reading); Node's
//! recorder read each request off the bytes a connection carried, and so does
//! this. A connection may carry several requests (`ureq` keeps them alive), so
//! they are told apart by their framing: a head, and as many bytes of body as
//! its `Content-Length` says.
//!
//! The relay writes a request down before the API can have taken it (the same
//! turn of the one thread that forwards the bytes reads the request in them),
//! so once a run has ended, and the API answered each request it made, the
//! relay holds exactly the requests the run made.

use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

/// A request as the client wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub method: String,
    /// The request line's target, with its query, as written.
    pub target: String,
    pub authorization: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

/// What has passed.
#[derive(Default)]
struct Passed {
    requests: Vec<Sent>,
    /// Why a request could not be read, which the relay then stopped reading.
    unread: Vec<String>,
}

/// A relay to the API, and what has passed through it.
pub struct Relay {
    address: String,
    passed: Rc<RefCell<Passed>>,
    listening: JoinHandle<()>,
}

impl Relay {
    /// A relay to the API at `api` (`127.0.0.1:<port>`), listening on a port of
    /// loopback of its own.
    pub async fn start(api: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let address = listener.local_addr()?.to_string();
        let passed = Rc::new(RefCell::new(Passed::default()));
        let listening =
            tokio::task::spawn_local(listen(listener, api.to_owned(), Rc::clone(&passed)));
        Ok(Self {
            address,
            passed,
            listening,
        })
    }

    /// Where it listens: `127.0.0.1:<port>`.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// The requests that have passed, in the order they were written, and why
    /// any request could not be read.
    pub fn taken(&self) -> (Vec<Sent>, Vec<String>) {
        let passed = self.passed.borrow();
        (passed.requests.clone(), passed.unread.clone())
    }
}

impl Drop for Relay {
    /// Its connections end with it.
    fn drop(&mut self) {
        self.listening.abort();
    }
}

/// Takes each connection to the relay until the relay is dropped, and passes it
/// on; what is passing ends with the listener.
async fn listen(listener: TcpListener, api: String, passed: Rc<RefCell<Passed>>) {
    let mut passing = JoinSet::new();
    while let Ok((client, _)) = listener.accept().await {
        passing.spawn_local(pass(client, api.clone(), Rc::clone(&passed)));
    }
}

/// One connection, passed to the API and back until both ends have ended.
async fn pass(client: TcpStream, api: String, passed: Rc<RefCell<Passed>>) {
    let Ok(upstream) = TcpStream::connect(&api).await else {
        return;
    };
    if client.set_nodelay(true).is_err() || upstream.set_nodelay(true).is_err() {
        return;
    }
    let (mut from_client, mut to_client) = client.into_split();
    let (mut from_api, mut to_api) = upstream.into_split();
    let asking = async {
        let mut chunk = vec![0_u8; 16 * 1024];
        let mut pending = Vec::new();
        let mut forwarding = true;
        loop {
            let count = match from_client.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(count) => count,
            };
            // The API may have gone, having refused what it need not read: the
            // client's request is read to its end all the same.
            if forwarding && to_api.write_all(&chunk[..count]).await.is_err() {
                forwarding = false;
            }
            pending.extend_from_slice(&chunk[..count]);
            match requests(&mut pending) {
                Ok(more) => passed.borrow_mut().requests.extend(more),
                Err(why) => {
                    passed.borrow_mut().unread.push(why);
                    break;
                }
            }
        }
        let _ = to_api.shutdown().await;
    };
    let answering = async {
        let _ = tokio::io::copy(&mut from_api, &mut to_client).await;
        let _ = to_client.shutdown().await;
    };
    tokio::join!(asking, answering);
}

/// The requests whole in `pending`, taken off its front: a head, and a body of
/// as many bytes as the head says.
fn requests(pending: &mut Vec<u8>) -> Result<Vec<Sent>, String> {
    let mut taken = Vec::new();
    while let Some(end) = pending.windows(4).position(|window| window == b"\r\n\r\n") {
        let head = String::from_utf8_lossy(&pending[..end]).into_owned();
        let mut lines = head.split("\r\n");
        let mut start = lines.next().unwrap_or_default().splitn(3, ' ');
        let (method, target) = (
            start.next().unwrap_or_default(),
            start.next().unwrap_or_default(),
        );
        let (mut authorization, mut content_type, mut length) = (None, None, 0);
        for line in lines {
            let (name, value) = line
                .split_once(':')
                .ok_or_else(|| format!("a header with no colon: {line}"))?;
            let value = value.trim();
            match name.to_ascii_lowercase().as_str() {
                "authorization" => authorization = Some(value.to_owned()),
                "content-type" => content_type = Some(value.to_owned()),
                "content-length" => {
                    length = value
                        .parse::<usize>()
                        .map_err(|_| format!("a content length that is no number: {value}"))?;
                }
                "transfer-encoding" => {
                    return Err(format!(
                        "a body sent as {value}, which the relay does not read"
                    ));
                }
                _ => {}
            }
        }
        let whole = end + 4 + length;
        if pending.len() < whole {
            break;
        }
        let body = pending[end + 4..whole].to_vec();
        pending.drain(..whole);
        taken.push(Sent {
            method: method.to_owned(),
            target: target.to_owned(),
            authorization,
            content_type,
            body,
        });
    }
    Ok(taken)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::trace::locally;

    const NOTE: &[u8] = b"POST /api/notes?to=human HTTP/1.1\r\nHost: x\r\nAUTHORIZATION: Bearer abc\r\ncontent-type: application/json\r\nContent-Length: 7\r\n\r\n{\"a\":1}";
    const WHOAMI: &[u8] = b"GET /api/whoami HTTP/1.1\r\nHost: x\r\n\r\n";

    fn note() -> Sent {
        Sent {
            method: "POST".to_owned(),
            target: "/api/notes?to=human".to_owned(),
            authorization: Some("Bearer abc".to_owned()),
            content_type: Some("application/json".to_owned()),
            body: b"{\"a\":1}".to_vec(),
        }
    }

    #[test]
    fn a_request_is_taken_when_its_head_and_its_body_are_whole_and_not_before() {
        let mut pending = NOTE[..NOTE.len() - 3].to_vec();
        assert_eq!(requests(&mut pending), Ok(Vec::new()));
        assert_eq!(pending.len(), NOTE.len() - 3, "what is not whole is kept");
        pending.extend_from_slice(&NOTE[NOTE.len() - 3..]);
        assert_eq!(requests(&mut pending), Ok(vec![note()]));
        assert!(pending.is_empty());
    }

    #[test]
    fn a_connection_that_carries_requests_one_after_the_other_gives_each() {
        let mut pending = [NOTE, WHOAMI, &NOTE[..20]].concat();
        let taken = requests(&mut pending).unwrap();
        let whoami = Sent {
            method: "GET".to_owned(),
            target: "/api/whoami".to_owned(),
            authorization: None,
            content_type: None,
            body: Vec::new(),
        };
        assert_eq!(taken, vec![note(), whoami]);
        assert_eq!(pending, &NOTE[..20], "the one still coming is kept");
    }

    #[test]
    fn a_body_it_cannot_frame_is_a_failure_and_not_a_guess() {
        for head in [
            "Transfer-Encoding: chunked",
            "Content-Length: many",
            "no colon here",
        ] {
            let mut pending = format!("POST / HTTP/1.1\r\n{head}\r\n\r\n").into_bytes();
            assert!(requests(&mut pending).is_err(), "{head}");
        }
    }

    #[test]
    fn what_passes_is_the_same_bytes_both_ways_and_is_written_down() {
        locally(async {
            let api = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let address = api.local_addr().unwrap().to_string();
            const ANSWER: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
            let serving = tokio::task::spawn_local(async move {
                let (mut stream, _) = api.accept().await.unwrap();
                let mut got = vec![0_u8; NOTE.len()];
                stream.read_exact(&mut got).await.unwrap();
                stream.write_all(ANSWER).await.unwrap();
                got
            });
            let relay = Relay::start(&address).await.unwrap();
            let mut client = TcpStream::connect(relay.address()).await.unwrap();
            // The head and the body come in two writes, as `ureq` may write them.
            let (head, body) = NOTE.split_at(NOTE.len() - 7);
            client.write_all(head).await.unwrap();
            tokio::task::yield_now().await;
            client.write_all(body).await.unwrap();
            let mut answered = vec![0_u8; ANSWER.len()];
            client.read_exact(&mut answered).await.unwrap();
            assert_eq!(answered, ANSWER);
            assert_eq!(serving.await.unwrap(), NOTE);
            assert_eq!(relay.taken(), (vec![note()], Vec::new()));
        });
    }
}
