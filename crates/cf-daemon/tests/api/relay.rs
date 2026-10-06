//! What a run of `cf` sends and is answered, written down as it passes: a relay
//! on a port of loopback of its own that the run is told is the API, which
//! forwards every byte to the API under test and every byte of the answer back,
//! and keeps each request the run wrote and each answer the API gave.
//!
//! The API cannot say what it was sent. A handler is given the path, the bearer
//! and a body it reads when it chooses (`Request` holds no query to list, no
//! content type, and no body that a route refused before reading); Node's
//! recorder read each request off the bytes a connection carried, and so does
//! this. Nor can what the API says be had from what its handler answered: the
//! server writes it, and a status or a content type it got wrong would still
//! be what the handler said, and what `cf` takes for an answer (any 2xx; any
//! type). So the answer is read off the bytes the relay passes back, as `cf`
//! got them. How a connection's bytes are cut into either is `frames.rs`.
//!
//! The relay writes a request down before the API is sent a byte of it, and an
//! answer before the client is: so once a run has ended, and the API answered
//! each request it made, the relay holds exactly the requests the run made and
//! each answer the run was given. An answer is its request's by its place: the
//! API serves a connection's requests one at a time, so the n-th answer on a
//! connection is the n-th request's, whichever of the two was whole first (a
//! route that refuses before it reads a body answers before the body is).

use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

use crate::frames::{replies, requests, Sent};
use crate::front::Reply;

/// A request that passed, and where: the connection it came over, and which of
/// that connection's requests it was.
struct Written {
    sent: Sent,
    connection: usize,
    nth: usize,
}

/// What has passed.
#[derive(Default)]
struct Passed {
    requests: Vec<Written>,
    /// What each connection was answered, in order: one list a connection.
    answers: Vec<Vec<Reply>>,
    /// Why a request or an answer could not be read, which the relay then
    /// stopped reading.
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
    /// any request or answer could not be read.
    pub fn taken(&self) -> (Vec<Sent>, Vec<String>) {
        let passed = self.passed.borrow();
        let sent = passed.requests.iter().map(|it| it.sent.clone()).collect();
        (sent, passed.unread.clone())
    }

    /// How each request that has passed was answered, as the API wrote the
    /// answer, in the order of `taken`: none where no whole answer has passed.
    pub fn replies(&self) -> Vec<Option<Reply>> {
        let passed = self.passed.borrow();
        passed
            .requests
            .iter()
            .map(|it| passed.answers[it.connection].get(it.nth).cloned())
            .collect()
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
    let connection = {
        let mut passed = passed.borrow_mut();
        passed.answers.push(Vec::new());
        passed.answers.len() - 1
    };
    let (mut from_client, mut to_client) = client.into_split();
    let (mut from_api, mut to_api) = upstream.into_split();
    let asking = async {
        let mut chunk = vec![0_u8; 16 * 1024];
        let mut pending = Vec::new();
        let mut forwarding = true;
        let mut counted = 0;
        loop {
            let count = match from_client.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(count) => count,
            };
            // Written down before the API is sent a byte of it.
            pending.extend_from_slice(&chunk[..count]);
            let read = requests(&mut pending);
            match &read {
                Ok(more) => {
                    let mut passed = passed.borrow_mut();
                    for sent in more {
                        passed.requests.push(Written {
                            sent: sent.clone(),
                            connection,
                            nth: counted,
                        });
                        counted += 1;
                    }
                }
                Err(why) => passed.borrow_mut().unread.push(why.clone()),
            }
            // The API may have gone, having refused what it need not read: the
            // client's request is read to its end all the same.
            if forwarding && to_api.write_all(&chunk[..count]).await.is_err() {
                forwarding = false;
            }
            if read.is_err() {
                break;
            }
        }
        let _ = to_api.shutdown().await;
    };
    let answering = async {
        let mut chunk = vec![0_u8; 16 * 1024];
        let mut pending = Vec::new();
        let mut reading = true;
        loop {
            let count = match from_api.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(count) => count,
            };
            // Written down before the client is sent a byte of it, so that a
            // client that has read its answer through has it written down.
            if reading {
                pending.extend_from_slice(&chunk[..count]);
                match replies(&mut pending) {
                    Ok(more) => passed.borrow_mut().answers[connection].extend(more),
                    Err(why) => {
                        passed.borrow_mut().unread.push(why);
                        reading = false;
                    }
                }
            }
            if to_client.write_all(&chunk[..count]).await.is_err() {
                break;
            }
        }
        let _ = to_client.shutdown().await;
    };
    tokio::join!(asking, answering);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::support::trace::locally;

    const NOTE: &[u8] = b"POST /api/notes?to=human HTTP/1.1\r\nHost: x\r\nAUTHORIZATION: Bearer abc\r\ncontent-type: application/json\r\nContent-Length: 7\r\n\r\n{\"a\":1}";
    const ANSWER: &[u8] =
        b"HTTP/1.1 201 Created\r\ncontent-type: application/json\r\nContent-Length: 2\r\n\r\n{}";

    fn note() -> Sent {
        Sent {
            method: "POST".to_owned(),
            target: "/api/notes?to=human".to_owned(),
            authorization: Some("Bearer abc".to_owned()),
            content_type: Some("application/json".to_owned()),
            body: b"{\"a\":1}".to_vec(),
        }
    }

    fn created() -> Reply {
        Reply {
            status: 201,
            content_type: Some("application/json".to_owned()),
            body: b"{}".to_vec(),
        }
    }

    #[test]
    fn what_passes_is_the_same_bytes_both_ways_and_is_written_down() {
        locally(async {
            let api = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let address = api.local_addr().unwrap().to_string();
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
            // The client has read the answer through: it is written down.
            assert_eq!(relay.replies(), vec![Some(created())]);
        });
    }

    /// Reads more of what `stream` carries onto `seen`: false once it has ended.
    async fn more(stream: &mut TcpStream, seen: &mut Vec<u8>) -> bool {
        let mut chunk = [0_u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => false,
            Ok(count) => {
                seen.extend_from_slice(&chunk[..count]);
                true
            }
        }
    }

    /// One connection of an API that answers each request with a 200 that
    /// says its target and how many requests it has answered here. It answers
    /// as soon as it has a request's head if `early` (a route that refuses
    /// before it reads a body does), and reads the body after; else it waits
    /// for the whole request.
    async fn echo(mut stream: TcpStream, early: bool) {
        let mut seen = Vec::new();
        let mut answered = 0;
        loop {
            let end = loop {
                if let Some(end) = seen.windows(4).position(|window| window == b"\r\n\r\n") {
                    break end;
                }
                if !more(&mut stream, &mut seen).await {
                    return;
                }
            };
            let head = String::from_utf8_lossy(&seen[..end]).into_owned();
            let target = head.split(' ').nth(1).unwrap_or_default().to_owned();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .map_or(0, |length| length.parse::<usize>().unwrap());
            let whole = end + 4 + length;
            while !early && seen.len() < whole {
                if !more(&mut stream, &mut seen).await {
                    return;
                }
            }
            let body = format!("{{\"target\":\"{target}\",\"nth\":{answered}}}");
            let said = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(said.as_bytes()).await.unwrap();
            answered += 1;
            while seen.len() < whole {
                if !more(&mut stream, &mut seen).await {
                    return;
                }
            }
            seen.drain(..whole);
        }
    }

    /// The address of an API of that kind.
    async fn echoing(early: bool) -> String {
        let api = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = api.local_addr().unwrap().to_string();
        tokio::task::spawn_local(async move {
            while let Ok((stream, _)) = api.accept().await {
                tokio::task::spawn_local(echo(stream, early));
            }
        });
        address
    }

    /// What `client` is answered, read to the end of the JSON the echo says.
    async fn answer_to(client: &mut TcpStream) -> String {
        let mut said = String::new();
        let mut chunk = [0_u8; 4096];
        while !said.ends_with('}') {
            let count = client.read(&mut chunk).await.unwrap();
            said.push_str(&String::from_utf8_lossy(&chunk[..count]));
        }
        said
    }

    /// A request with a body written as `ureq` writes one, in two writes: the
    /// head, and then the body. An API that answers at the head is read from
    /// before the body is written, so that its answer is the relay's first.
    async fn post(client: &mut TcpStream, target: &str, early: bool) -> String {
        let body = "{\"a\":1}";
        let head = format!(
            "POST {target} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        client.write_all(head.as_bytes()).await.unwrap();
        if early {
            let said = answer_to(client).await;
            client.write_all(body.as_bytes()).await.unwrap();
            return said;
        }
        tokio::task::yield_now().await;
        client.write_all(body.as_bytes()).await.unwrap();
        answer_to(client).await
    }

    #[test]
    fn an_answer_is_its_requests_by_its_place_on_its_connection_whichever_was_whole_first() {
        for early in [false, true] {
            locally(async {
                let relay = Relay::start(&echoing(early).await).await.unwrap();
                // Two connections, and a request again on the first: the
                // order the requests were written is the order of the replies.
                let mut clients = [
                    TcpStream::connect(relay.address()).await.unwrap(),
                    TcpStream::connect(relay.address()).await.unwrap(),
                ];
                for (client, target) in [(0, "/a"), (1, "/b"), (0, "/c")] {
                    post(&mut clients[client], target, early).await;
                }
                // An API that answered at the head did so before the body was
                // written, and the relay reads the body when it next can.
                until_taken(&relay, 3).await;
                let (sent, unread) = relay.taken();
                assert_eq!(unread, Vec::<String>::new(), "early {early}");
                let targets: Vec<&str> = sent.iter().map(|sent| sent.target.as_str()).collect();
                assert_eq!(targets, ["/a", "/b", "/c"], "early {early}");
                let bodies: Vec<String> = relay
                    .replies()
                    .into_iter()
                    .map(|reply| String::from_utf8(reply.expect("answered").body).unwrap())
                    .collect();
                assert_eq!(
                    bodies,
                    [
                        r#"{"target":"/a","nth":0}"#,
                        r#"{"target":"/b","nth":0}"#,
                        r#"{"target":"/c","nth":1}"#
                    ],
                    "early {early}"
                );
            });
        }
    }

    /// Waits until the relay has written down `count` requests: it has read
    /// them when it has.
    async fn until_taken(relay: &Relay, count: usize) {
        for _ in 0..500 {
            if relay.taken().0.len() == count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[test]
    fn a_request_that_passed_and_was_not_answered_has_no_reply() {
        locally(async {
            // An API that takes what it is sent and says nothing.
            let silent = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let address = silent.local_addr().unwrap().to_string();
            tokio::task::spawn_local(async move {
                let _kept = silent.accept().await;
                std::future::pending::<()>().await;
            });
            let relay = Relay::start(&address).await.unwrap();
            let mut client = TcpStream::connect(relay.address()).await.unwrap();
            client.write_all(NOTE).await.unwrap();
            until_taken(&relay, 1).await;
            assert_eq!(relay.replies(), vec![None]);
        });
    }

    /// An answer that carries no length and is not bodiless: it ends where its
    /// connection does, which the relay does not read.
    const ODD: &[u8] = b"HTTP/1.1 200 OK\r\n\r\n{}";

    #[test]
    fn an_answer_that_cannot_be_framed_is_passed_on_as_it_was_and_says_why_it_was_not_read() {
        locally(async {
            let odd = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let address = odd.local_addr().unwrap().to_string();
            tokio::task::spawn_local(async move {
                let (mut stream, _) = odd.accept().await.unwrap();
                let mut got = vec![0_u8; NOTE.len()];
                stream.read_exact(&mut got).await.unwrap();
                stream.write_all(ODD).await.unwrap();
                std::future::pending::<()>().await;
            });
            let relay = Relay::start(&address).await.unwrap();
            let mut client = TcpStream::connect(relay.address()).await.unwrap();
            client.write_all(NOTE).await.unwrap();
            let mut said = vec![0_u8; ODD.len()];
            client.read_exact(&mut said).await.unwrap();
            assert_eq!(said, ODD, "the client has it as it was");
            let (_, unread) = relay.taken();
            assert!(
                unread[0].starts_with("an answer of 200 with no length"),
                "{unread:?}"
            );
            assert_eq!(relay.replies(), vec![None]);
        });
    }
}
