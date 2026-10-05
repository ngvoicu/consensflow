//! The server: hyper's `http1` on the engine's `LocalSet`, a task to each
//! connection and a task to each request.
//!
//! A request's handler runs as a task of its own, and the connection waits for
//! its end: a client that leaves does not drop the handler, which Node ran to
//! the end (an update would die halfway), and a panic in it is caught there
//! and answered 500 `internal`. A connection that waits for its next request
//! is held to five seconds, as Node's keep-alive held it.
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
use std::io;
use std::pin::pin;
use std::rc::Rc;
use std::time::Duration;

use bytes::Bytes;
use cf_base::js;
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
use super::context::Closing;
use super::request::Request;
use crate::errors::{contain, Errors};

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
    errors: Rc<Errors>,
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
    /// request with `handler` from now on, on the local set this runs in.
    /// `closing` is set when the front closes.
    pub async fn start(
        handler: Rc<Handler>,
        closing: Closing,
        errors: Rc<Errors>,
    ) -> io::Result<Self> {
        Self::start_holding_idle_to(IDLE, handler, closing, errors).await
    }

    /// [`Api::start`], with a connection that waits for its next request held
    /// to `idle`.
    pub(crate) async fn start_holding_idle_to(
        idle: Duration,
        handler: Rc<Handler>,
        closing: Closing,
        errors: Rc<Errors>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let url = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
        let server = Rc::new(Server {
            handler,
            idle,
            closing_flag: closing,
            errors,
            closing: watch::Sender::new(false),
            open: watch::Sender::new(0),
            aborts: RefCell::new(HashMap::new()),
            next: Cell::new(0),
        });
        let accepting = Rc::clone(&server);
        server
            .errors
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
                server.errors.caught("a connection failed", &panicked);
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
    let mut connection = pin!(connection);
    let mut closing = server.closing.subscribe();
    let ended = tokio::select! {
        _ = connection.as_mut() => true,
        _ = closing.wait_for(|closing| *closing) => false,
    };
    if !ended {
        // What it has in hand is finished, and no request after it is read.
        connection.as_mut().graceful_shutdown();
        let _ = connection.as_mut().await;
    }
}

/// One request: its handler runs as a task of its own, so that the client
/// going away does not stop it, and a panic in it is a 500.
async fn respond(server: &Rc<Server>, request: hyper::Request<Incoming>) -> Response<Full<Bytes>> {
    let state = Rc::clone(server);
    let handler = tokio::task::spawn_local(async move {
        let outcome = contain(async {
            let request = Request::from_hyper(request)?;
            (state.handler)(request).await
        })
        .await;
        match outcome {
            Ok(Ok(answer)) => answer,
            Ok(Err(failure)) => failure.answer(),
            Err(panicked) => {
                state.errors.caught("a request failed", &panicked);
                Failure::Internal(panicked.message).answer()
            }
        }
    });
    let answer = handler
        .await
        .unwrap_or_else(|_| Failure::Internal("the request was cut short".to_owned()).answer());
    response(&answer)
}

/// An answer as a response (`send`, `api.js:503-516`): a page as
/// `text/html; charset=utf-8`, nothing as a status alone, anything else as
/// `application/json`, written as `JSON.stringify` writes it.
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
