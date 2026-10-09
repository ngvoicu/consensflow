//! A server on loopback that a test stands up where a harness's own would be
//! (Codex's broker, OpenCode's server and plugin): real sockets, which the
//! channels' `SystemLoopback` is held against, where `ScriptedLoopback`
//! (`peer`) answers on none. It reads each request whole, writes it down, and
//! does what the test's handler says of it: answers, takes it and never
//! answers, ends the connection, or answers a head and never finishes the body:
//! the ways a harness fails a send.
//!
//! Its tasks are local, like the engine's work: a test runs a server inside a
//! `LocalSet`, on the thread of its runtime.

use std::cell::RefCell;
use std::convert::Infallible;
use std::future::{pending, ready};
use std::io;
use std::net::SocketAddr;
use std::rc::Rc;

use futures_util::stream::{self, StreamExt as _};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{spawn_local, JoinHandle, JoinSet};
use url::form_urlencoded;

/// A request as the server read it, whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// What the request line names, as it was written: a path, and after a
    /// `?` its query.
    pub target: String,
    /// The headers in the order they came, their names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// The target's path.
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or_default()
    }

    /// The first value the query gives `name`, as `URLSearchParams.get` reads
    /// it: a `+` is a space.
    pub fn query(&self, name: &str) -> Option<String> {
        let (_, query) = self.target.split_once('?')?;
        form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    }

    /// The first value of the header `name`, in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The body as JSON; a test reads what a channel wrote, which is JSON.
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|failed| {
            panic!(
                "a body that is no JSON ({failed}): {}",
                String::from_utf8_lossy(&self.body)
            )
        })
    }
}

/// What a server does with a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// Answers `status`, with `body` where there is one.
    Answer { status: u16, body: Vec<u8> },
    /// Takes the request and never answers: a caller that must not retry
    /// waits for its own timer.
    Hang,
    /// Ends the connection with no answer, as `socket.destroy()` does.
    Drop,
    /// Answers a head with `status` and the first bytes of a body that never
    /// ends.
    Stall { status: u16, start: Vec<u8> },
}

impl Reply {
    /// Answers `status` with `body` as JSON.
    pub fn json(status: u16, body: &Value) -> Self {
        Self::text(status, &body.to_string())
    }

    /// Answers `status` with `body` as it is.
    pub fn text(status: u16, body: &str) -> Self {
        Self::Answer {
            status,
            body: body.as_bytes().to_vec(),
        }
    }

    /// Answers `status` with no body.
    pub fn status(status: u16) -> Self {
        Self::text(status, "")
    }

    /// Answers a head with `status`, then `start` of a body that never ends.
    pub fn stall(status: u16, start: &str) -> Self {
        Self::Stall {
            status,
            start: start.as_bytes().to_vec(),
        }
    }
}

/// How a server decides what to do with a request.
type Handler = Rc<dyn Fn(&Request) -> Reply>;

/// A server on a port of loopback of its own, for as long as it is held: its
/// connections end with it. It keeps every request it was sent.
pub struct Server {
    address: SocketAddr,
    calls: Rc<RefCell<Vec<Request>>>,
    serving: JoinHandle<()>,
}

impl Server {
    /// Starts a server on the runtime and local set the caller is in.
    /// `handler` is asked for each request once it has been written down.
    pub async fn start(handler: impl Fn(&Request) -> Reply + 'static) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a port on loopback");
        let address = listener.local_addr().expect("the port it was given");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let written = Rc::clone(&calls);
        let serving = spawn_local(serve(listener, move |request: &Request| {
            written.borrow_mut().push(request.clone());
            handler(request)
        }));
        Self {
            address,
            calls,
            serving,
        }
    }

    /// Where it listens, as a channel names a server: `http://127.0.0.1:<port>`.
    pub fn endpoint(&self) -> String {
        format!("http://{}", self.address)
    }

    /// The requests it was sent so far, in the order they came.
    pub fn calls(&self) -> Vec<Request> {
        self.calls.borrow().clone()
    }
}

impl Drop for Server {
    /// The connections it holds end with it.
    fn drop(&mut self) {
        self.serving.abort();
    }
}

/// Takes every connection to `listener` and answers its requests as `handler`
/// says, until it is dropped: what a process that stands in for a server runs,
/// and what a [`Server`] runs as a task.
pub async fn serve(listener: TcpListener, handler: impl Fn(&Request) -> Reply + 'static) {
    let handler: Handler = Rc::new(handler);
    let mut connections = JoinSet::new();
    while let Ok((stream, _)) = listener.accept().await {
        while connections.try_join_next().is_some() {}
        connections.spawn_local(connection(stream, Rc::clone(&handler)));
    }
}

/// One connection, until it ends.
async fn connection(stream: TcpStream, handler: Handler) {
    let service = service_fn(move |request| answer(Rc::clone(&handler), request));
    // A connection that breaks is for its client to have noticed.
    let _ = http1::Builder::new()
        .timer(TokioTimer::new())
        .serve_connection(TokioIo::new(stream), service)
        .await;
}

/// What a response's body is: whole, or one that stalls.
type Body = BoxBody<Bytes, Infallible>;

/// One request, read whole and handed to `handler`. An error ends the
/// connection with no answer.
async fn answer(
    handler: Handler,
    request: hyper::Request<Incoming>,
) -> Result<hyper::Response<Body>, io::Error> {
    let (head, body) = request.into_parts();
    let body = body.collect().await.map_err(io::Error::other)?.to_bytes();
    let asked = Request {
        method: head.method.to_string(),
        target: head.uri.to_string(),
        headers: head
            .headers
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect(),
        body: body.to_vec(),
    };
    let (status, body): (u16, Body) = match handler(&asked) {
        Reply::Answer { status, body } => (status, Full::new(Bytes::from(body)).boxed()),
        Reply::Hang => pending().await,
        Reply::Drop => return Err(io::Error::other("the stand-in ended the connection")),
        Reply::Stall { status, start } => {
            let frames =
                stream::once(ready(Ok(Frame::data(Bytes::from(start))))).chain(stream::pending());
            (status, BodyExt::boxed(StreamBody::new(frames)))
        }
    };
    Ok(hyper::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(body)
        .expect("a response"))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::time::Duration;

    use serde_json::json;
    use tokio::task::LocalSet;
    use tokio::time::timeout;

    use super::*;
    use crate::seams::loopback::{Loopback, Method, Request as Asking, SystemLoopback};

    /// `test`, on one thread with the local tasks a server spawns.
    fn run<T>(test: impl Future<Output = T>) -> T {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        LocalSet::new().block_on(&runtime, test)
    }

    fn post(server: &Server, target: &str) -> Asking {
        Asking {
            method: Method::Post,
            url: format!("{}{target}", server.endpoint()),
            headers: vec![("authorization".to_owned(), "Bearer t".to_owned())],
            body: Some(b"{\"text\":\"hi\"}".to_vec()),
        }
    }

    #[test]
    fn a_request_is_written_down_whole_and_answered_as_the_handler_says() {
        run(async {
            let server =
                Server::start(|request| Reply::json(201, &json!({ "path": request.path() }))).await;
            let mut reply = SystemLoopback
                .send(post(&server, "/session/ses_1?directory=a+b%20c&x=1"))
                .await
                .unwrap();
            assert_eq!(reply.status(), 201);
            assert_eq!(
                reply.body(1024).await.unwrap(),
                br#"{"path":"/session/ses_1"}"#
            );
            let [call] = &server.calls()[..] else {
                panic!("one request");
            };
            assert_eq!(call.method, "POST");
            assert_eq!(call.target, "/session/ses_1?directory=a+b%20c&x=1");
            assert_eq!(call.query("directory").as_deref(), Some("a b c"));
            assert_eq!(call.query("x").as_deref(), Some("1"));
            assert_eq!(call.query("y"), None);
            assert_eq!(call.header("Authorization"), Some("Bearer t"));
            assert_eq!(call.json(), json!({ "text": "hi" }));
        });
    }

    #[test]
    fn a_request_taken_and_never_answered_waits_for_its_callers_own_timer() {
        run(async {
            let server = Server::start(|_| Reply::Hang).await;
            let sent = timeout(
                Duration::from_millis(150),
                SystemLoopback.send(post(&server, "/deliver")),
            )
            .await;
            assert!(sent.is_err(), "no head came");
            assert_eq!(server.calls().len(), 1, "but the request did");
        });
    }

    #[test]
    fn a_connection_ended_has_no_head_and_the_caller_is_told_so() {
        run(async {
            let server = Server::start(|_| Reply::Drop).await;
            let failed = SystemLoopback.send(post(&server, "/deliver")).await.err();
            assert_eq!(failed.as_deref(), Some("fetch failed"));
            assert_eq!(server.calls().len(), 1);
        });
    }

    #[test]
    fn a_stalled_answer_has_its_head_and_a_body_that_never_ends() {
        run(async {
            let server = Server::start(|_| Reply::stall(200, "{")).await;
            let mut reply = SystemLoopback
                .send(post(&server, "/global/health"))
                .await
                .unwrap();
            assert_eq!(reply.status(), 200);
            let body = timeout(Duration::from_millis(150), reply.body(1024)).await;
            assert!(body.is_err(), "the body does not end");
        });
    }

    #[test]
    fn a_server_dropped_ends_its_connections() {
        run(async {
            let server = Server::start(|_| Reply::Hang).await;
            let endpoint = server.endpoint();
            drop(server);
            tokio::task::yield_now().await;
            let request = Asking {
                method: Method::Get,
                url: format!("{endpoint}/session"),
                headers: Vec::new(),
                body: None,
            };
            let failed = SystemLoopback.send(request).await.err();
            assert_eq!(failed.as_deref(), Some("fetch failed"));
        });
    }
}
