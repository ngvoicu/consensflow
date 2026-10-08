//! The server: hyper's `http1` on the engine's `LocalSet`, a task to each
//! connection.
//!
//! A request's handler is begun where the request is ([`begin`]): what it does
//! before its first wait is done in the request's first part, as JavaScript
//! ran it, and what the first part woke is run to its end ([`DaemonSpawn::drain`])
//! before the connection goes on to what it reads next. The rest of the
//! handler is the executor's work, and the connection waits for its answer: a
//! client that leaves does not drop the handler, which Node ran to the end (an
//! update would die halfway), and a panic in it is caught there and answered
//! 500 `internal`. A connection that waits for its next request is held to
//! five seconds, as Node's keep-alive held it. The handler is told when its
//! client has gone ([`Consumer`]), for the doors that wait: a door has
//! nobody to give an answer to, and claims none.
//!
//! Each poll of a connection is a callback of its own ([`Drained`]): what it
//! woke is run to its end before another task runs, as Node ran its microtasks
//! after each `data` event of a socket. A request's body reaches its handler
//! through a pump beside the request ([`Body::pumped`]), which does the same
//! after each piece it hands over and when the body ends. A handler's
//! answer is written when its connection is next polled, a turn after the
//! drain that ended its work; hyper serves a connection's requests one at a
//! time, so a request written behind another waits for its answer.
//!
//! Closing ([`Api::close`]) is told to every part together: the doors waiting
//! for an answer are answered at once ([`Closing`]), the listener is let go,
//! and each connection is asked to finish what it has (hyper's graceful
//! shutdown, which ends an idle connection at once and one with a request in
//! it when its answer is out). What is still open when the caller's deadline
//! comes is dropped ([`Api::drop_connections`]).
//!
//! The server knows nothing of routes: what answers a request is the handler it
//! is given, which the daemon makes of [`super::handle`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use cf_base::js;
use cf_engine::runtime::begin;
use futures_util::future::LocalBoxFuture;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::header::{HeaderValue, CONTENT_TYPE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::AbortHandle;
use tokio::time::sleep;

use super::answer::{Answer, Content, Failure};
use super::body::Body;
use super::context::Closing;
use super::request::{Consumer, Request};
use crate::errors::{contain, Errors};
use crate::seams::DaemonSpawn;

/// How long a connection may wait for its next request, or for the rest of one
/// whose head has begun: Node's keep-alive timeout.
const IDLE: Duration = Duration::from_secs(5);

/// What answers a request: its answer, or why it has none.
pub type Handler = dyn Fn(Request) -> LocalBoxFuture<'static, Result<Answer, Failure>>;

/// The HTTP front, listening.
pub struct Api {
    url: String,
    server: Rc<Server>,
}

/// What every task of the front shares.
struct Server {
    handler: Rc<Handler>,
    /// How long a connection may wait for its next request.
    idle: Duration,
    /// Set when the front closes, for the doors that wait.
    closing_flag: Closing,
    /// Where each request's handler runs, and a panic is written down.
    spawn: Rc<DaemonSpawn>,
    /// Told when the front closes: the listener is let go and each connection
    /// asked to finish.
    closing: watch::Sender<bool>,
    /// How many connections are open.
    open: watch::Sender<usize>,
    aborts: RefCell<HashMap<u64, AbortHandle>>,
    next: Cell<u64>,
}

impl Api {
    /// Listens on a port of loopback the system chooses, and answers each
    /// request with `handler` from now on, on the local set this runs in and
    /// the executor of `spawn`. `closing` is set when the front closes.
    pub async fn start(
        handler: Rc<Handler>,
        closing: Closing,
        spawn: Rc<DaemonSpawn>,
    ) -> io::Result<Self> {
        Self::start_holding_idle_to(IDLE, handler, closing, spawn).await
    }

    /// [`Api::start`], with a connection that waits for its next request held
    /// to `idle`.
    pub(crate) async fn start_holding_idle_to(
        idle: Duration,
        handler: Rc<Handler>,
        closing: Closing,
        spawn: Rc<DaemonSpawn>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let url = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
        let server = Rc::new(Server {
            handler,
            idle,
            closing_flag: closing,
            spawn,
            closing: watch::Sender::new(false),
            open: watch::Sender::new(0),
            aborts: RefCell::new(HashMap::new()),
            next: Cell::new(0),
        });
        let accepting = Rc::clone(&server);
        server
            .errors()
            .spawn("the listener failed", accept(accepting, listener));
        Ok(Self { url, server })
    }

    /// Where it listens: `http://127.0.0.1:<port>`, with no slash at its end,
    /// which is what a window's `CONSENSFLOW_URL` is.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Closes the front and ends when every connection has: the doors are
    /// answered at once, the listener is let go, and each connection is asked
    /// to finish. It has no bound of its own: the caller's deadline is the
    /// bound.
    pub async fn close(&self) {
        self.server.closing_flag.set();
        self.server.closing.send_replace(true);
        let mut open = self.server.open.subscribe();
        let _ = open.wait_for(|connections| *connections == 0).await;
    }

    /// Drops every connection still open, with the request in it.
    pub fn drop_connections(&self) {
        let open: Vec<AbortHandle> = self.server.aborts.borrow().values().cloned().collect();
        for connection in open {
            connection.abort();
        }
    }
}

/// Takes every connection to the port until the front closes; the listener
/// is let go as it ends.
async fn accept(server: Rc<Server>, listener: TcpListener) {
    let mut closing = server.closing.subscribe();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => server.connect(stream),
                    // Out of descriptors, or a connection that went before it
                    // was taken: nothing to do but try again in a moment.
                    Err(_) => sleep(Duration::from_millis(10)).await,
                }
            }
            _ = closing.wait_for(|closing| *closing) => return,
        }
    }
}

/// A connection counted while it lives: its task's end, any end, takes it off.
struct Open {
    server: Rc<Server>,
    id: u64,
}

impl Drop for Open {
    fn drop(&mut self) {
        self.server.aborts.borrow_mut().remove(&self.id);
        self.server.open.send_modify(|open| *open -= 1);
    }
}

impl Server {
    fn errors(&self) -> &Rc<Errors> {
        self.spawn.errors()
    }

    fn connect(self: &Rc<Self>, stream: TcpStream) {
        let id = self.next.get();
        self.next.set(id + 1);
        self.open.send_modify(|open| *open += 1);
        let held = Open {
            server: Rc::clone(self),
            id,
        };
        let server = Rc::clone(self);
        let task = tokio::task::spawn_local(async move {
            let _held = held;
            if let Err(panicked) = contain(serve(&server, stream)).await {
                server.errors().caught("a connection failed", &panicked);
            }
        });
        self.aborts.borrow_mut().insert(id, task.abort_handle());
    }
}

/// One connection's requests, until it ends or the front is closed.
async fn serve(server: &Rc<Server>, stream: TcpStream) {
    if stream.set_nodelay(true).is_err() {
        return;
    }
    let requests = Rc::clone(server);
    let service = service_fn(move |request| {
        let server = Rc::clone(&requests);
        async move { Ok::<_, Infallible>(respond(&server, request).await) }
    });
    let connection = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(server.idle)
        .serve_connection(TokioIo::new(stream), service);
    let mut connection = Drained::new(connection, Rc::clone(&server.spawn));
    let mut closing = server.closing.subscribe();
    let ended = tokio::select! {
        _ = &mut connection => true,
        _ = closing.wait_for(|closing| *closing) => false,
    };
    if !ended {
        // What it has in hand is finished, and no request after it is read.
        connection.pinned().graceful_shutdown();
        let _ = (&mut connection).await;
    }
}

/// A connection each poll of which is a callback of its own. What a poll
/// reads wakes the work that waits for it directly, where no drain is, and
/// tokio would run the tasks queued ahead of the driver first: another
/// connection's, the bridge's reader. The poll is followed by a drain, so that
/// what it woke runs to its end before any other task, as Node's microtasks
/// ran after each `data` event of a socket and before the next. (A request's
/// body is not read by the handler's work: it comes through the pump of
/// [`Body::pumped`], a task of its own, which drains as the poll does.)
struct Drained<F> {
    connection: Pin<Box<F>>,
    spawn: Rc<DaemonSpawn>,
}

impl<F> Drained<F> {
    fn new(connection: F, spawn: Rc<DaemonSpawn>) -> Self {
        Self {
            connection: Box::pin(connection),
            spawn,
        }
    }

    /// The connection, for what hyper asks of it pinned.
    fn pinned(&mut self) -> Pin<&mut F> {
        self.connection.as_mut()
    }
}

impl<F: Future> Future for Drained<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<F::Output> {
        let this = &mut *self;
        let polled = this.connection.as_mut().poll(context);
        this.spawn.drain();
        polled
    }
}

/// What tells a handler its client is gone: the request's work in the
/// connection ends with it, and is dropped before its answer is written when
/// the client closes the connection. The handler goes on, and is told. Once
/// the request is answered nobody is left to ask, and it makes no difference.
struct Leaves(Consumer);

impl Drop for Leaves {
    fn drop(&mut self) {
        self.0.leave();
    }
}

/// One request: its handler is begun here, in the request's first part, and
/// the rest of it is the executor's, so that the client going away does not
/// stop it, and the handler is told it has gone ([`Consumer`]); a panic in
/// it is a 500. What the first part woke is run to its end before the
/// connection goes on.
async fn respond(server: &Rc<Server>, request: hyper::Request<Incoming>) -> Response<Full<Bytes>> {
    let consumer = Consumer::default();
    let _client = Leaves(consumer.clone());
    // The body comes through a pump beside the request, which reads and lets
    // go what is left of it once the request no longer reads it.
    let (parts, incoming) = request.into_parts();
    let spawn = Rc::clone(&server.spawn);
    let (body, pump) = Body::pumped(incoming, move || spawn.drain());
    tokio::task::spawn_local(pump);
    let request = hyper::Request::from_parts(parts, body);
    let state = Rc::clone(server);
    let begun = begin(&*server.spawn, async move {
        let outcome = contain(async {
            let request = Request::from_hyper(request)?.consumed_by(consumer);
            (state.handler)(request).await
        })
        .await;
        match outcome {
            Ok(Ok(answer)) => answer,
            Ok(Err(failure)) => failure.answer(),
            Err(panicked) => {
                state.errors().caught("a request failed", &panicked);
                Failure::Internal(panicked.message).answer()
            }
        }
    })
    .await;
    server.spawn.drain();
    response(&begun.await)
}

/// An answer as a response: a page as `text/html; charset=utf-8`, nothing as a
/// status alone, anything else as `application/json`, written as
/// `JSON.stringify` writes it.
fn response(answer: &Answer) -> Response<Full<Bytes>> {
    let (kind, body) = match &answer.content {
        Content::Html(page) => (Some("text/html; charset=utf-8"), Bytes::from(page.clone())),
        Content::Nothing => (None, Bytes::new()),
        Content::Json(value) => (Some("application/json"), Bytes::from(js::stringify(value))),
    };
    let mut response = Response::new(Full::new(body));
    *response.status_mut() =
        StatusCode::from_u16(answer.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if let Some(kind) = kind {
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static(kind));
    }
    response
}

#[cfg(test)]
mod tests;
