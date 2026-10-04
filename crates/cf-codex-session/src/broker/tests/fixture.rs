//! What the broker's tests stand on: a Codex server that answers as the test
//! says, a TUI that connects to the broker, the daemon's two requests, and a
//! runtime that is one thread, as the broker's is.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::net::SocketAddr;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use cf_board::Board;
use cf_proto::codex::Bridge;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::{self, ClientRequestBuilder, Message};
use tokio_tungstenite::{accept_hdr_async_with_config, client_async_with_config};

use crate::broker::transport::config;
use crate::broker::{now_ms, Broker, Config};
use crate::endpoint::{Target, Upstream};

pub(crate) const A: &str = "01a09094-938f-7fd1-a2d3-315cf92b4559";
pub(crate) const B: &str = "01a09094-a559-7db0-bf50-e2309856c3c0";
pub(crate) const TOKEN: &str = "private-launch-token-1234567890";
pub(crate) const TEXT: &str = "complete\nworker result";

/// Runs `test` as the window's supervisor runs what it runs: on one thread,
/// with the local tasks the broker spawns.
pub(crate) fn run<F: Future>(test: F) -> F::Output {
    crate::block_on_local(test).expect("a runtime")
}

/// Waits until `condition` holds, polling; the test fails if it never does.
pub(crate) async fn wait(condition: impl Fn() -> bool) {
    for _ in 0..2000 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("native boundary did not complete");
}

/// [`wait`] for a condition that has to ask something.
pub(crate) async fn wait_for<F: Future<Output = bool>>(condition: impl Fn() -> F) {
    for _ in 0..2000 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("native boundary did not complete");
}

// ---------------------------------------------------------------------------
// Codex's server

/// How the fake server answers a request that it does not leave to the test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Behaviour {
    /// Answers `initialize`; holds every other request for the test to answer.
    Ready,
    /// Answers every message with an error.
    Refusing,
    /// Answers nothing.
    Silent,
}

struct Held {
    peer: usize,
    message: Value,
}

struct Peer {
    out: mpsc::UnboundedSender<Message>,
    headers: Vec<(String, String)>,
    open: Rc<Cell<bool>>,
    kill: Rc<Notify>,
    pongs: Rc<Cell<usize>>,
}

struct Inner {
    behaviour: Cell<Behaviour>,
    /// Hold each connection's handshake this long.
    handshake_delay: Cell<Duration>,
    /// Never read what a connection sends once it is open.
    stall: Cell<bool>,
    requests: RefCell<Vec<Value>>,
    /// Every text any connection sent, with the connection, whether or not it
    /// is JSON serde_json reads.
    texts: RefCell<Vec<(usize, String)>>,
    held: RefCell<Vec<Held>>,
    peers: RefCell<Vec<Peer>>,
}

/// A Codex server: the broker's own connection is its first peer, then one
/// for each TUI connection the broker proxies.
pub(crate) struct FakeCodex {
    pub(crate) address: SocketAddr,
    inner: Rc<Inner>,
    accept: JoinHandle<()>,
}

impl Drop for FakeCodex {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

impl Inner {
    fn new(behaviour: Behaviour) -> Rc<Self> {
        Rc::new(Self {
            behaviour: Cell::new(behaviour),
            handshake_delay: Cell::new(Duration::ZERO),
            stall: Cell::new(false),
            requests: RefCell::new(Vec::new()),
            texts: RefCell::new(Vec::new()),
            held: RefCell::new(Vec::new()),
            peers: RefCell::new(Vec::new()),
        })
    }
}

impl FakeCodex {
    pub(crate) async fn start(behaviour: Behaviour) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("its address");
        let inner = Inner::new(behaviour);
        let served = Rc::clone(&inner);
        let accept = tokio::task::spawn_local(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::task::spawn_local(serve(Rc::clone(&served), stream));
            }
        });
        Self {
            address,
            inner,
            accept,
        }
    }

    /// The same server on a Unix socket.
    #[cfg(unix)]
    pub(crate) async fn start_on_socket(path: &std::path::Path) -> Self {
        let listener = tokio::net::UnixListener::bind(path).expect("bind");
        let inner = Inner::new(Behaviour::Ready);
        let served = Rc::clone(&inner);
        let accept = tokio::task::spawn_local(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::task::spawn_local(serve(Rc::clone(&served), stream));
            }
        });
        Self {
            address: SocketAddr::from(([127, 0, 0, 1], 0)),
            inner,
            accept,
        }
    }

    /// Hold each connection's handshake for `delay` from now on.
    pub(crate) fn delay_handshakes(&self, delay: Duration) {
        self.inner.handshake_delay.set(delay);
    }

    /// Stop reading what a connection sends, from the next one on.
    pub(crate) fn stall(&self) {
        self.inner.stall.set(true);
    }

    /// Everything any connection has sent, in order.
    pub(crate) fn requests(&self) -> Vec<Value> {
        self.inner.requests.borrow().clone()
    }

    /// Every text any connection has sent, as it came, with the connection.
    pub(crate) fn texts(&self) -> Vec<(usize, String)> {
        self.inner.texts.borrow().clone()
    }

    pub(crate) fn requests_of(&self, method: &str) -> Vec<Value> {
        self.requests()
            .into_iter()
            .filter(|request| request["method"] == method)
            .collect()
    }

    pub(crate) fn is_held(&self, method: &str) -> bool {
        self.inner
            .held
            .borrow()
            .iter()
            .any(|held| held.message["method"] == method)
    }

    pub(crate) fn is_held_by_id(&self, id: &Value) -> bool {
        self.inner
            .held
            .borrow()
            .iter()
            .any(|held| &held.message["id"] == id)
    }

    /// How many connections it has taken.
    pub(crate) fn connections(&self) -> usize {
        self.inner.peers.borrow().len()
    }

    /// Whether connection `peer` is open from this end.
    pub(crate) fn is_open(&self, peer: usize) -> bool {
        self.inner
            .peers
            .borrow()
            .get(peer)
            .is_some_and(|peer| peer.open.get())
    }

    /// Whether every connection it took has closed.
    pub(crate) fn all_closed(&self) -> bool {
        self.inner
            .peers
            .borrow()
            .iter()
            .all(|peer| !peer.open.get())
    }

    pub(crate) fn headers(&self, peer: usize) -> Vec<(String, String)> {
        self.inner.peers.borrow()[peer].headers.clone()
    }

    pub(crate) fn pongs(&self, peer: usize) -> usize {
        self.inner.peers.borrow()[peer].pongs.get()
    }

    /// The connection a held `method` request came in on.
    pub(crate) fn peer_of(&self, method: &str) -> usize {
        self.inner
            .held
            .borrow()
            .iter()
            .find(|held| held.message["method"] == method)
            .map(|held| held.peer)
            .expect("a held request")
    }

    /// Sends `text` on connection `peer`.
    pub(crate) fn send_text(&self, peer: usize, text: impl Into<String>) {
        self.send_message(peer, Message::text(text.into()));
    }

    pub(crate) fn send_json(&self, peer: usize, value: &Value) {
        self.send_text(peer, value.to_string());
    }

    pub(crate) fn send_message(&self, peer: usize, message: Message) {
        let _ = self.inner.peers.borrow()[peer].out.send(message);
    }

    /// Drops connection `peer` with no goodbye.
    pub(crate) fn terminate(&self, peer: usize) {
        self.inner.peers.borrow()[peer].kill.notify_one();
    }

    /// Answers the first held `method` request with `result`, or `error` when
    /// there is one, and returns the request.
    pub(crate) async fn respond(&self, method: &str, result: Value, error: Option<Value>) -> Value {
        wait(|| self.is_held(method)).await;
        let held = {
            let mut held = self.inner.held.borrow_mut();
            let at = held
                .iter()
                .position(|held| held.message["method"] == method)
                .expect("a held request");
            held.remove(at)
        };
        let id = held.message["id"].clone();
        let answer = match error {
            Some(error) => json!({ "id": id, "error": error }),
            None => json!({ "id": id, "result": result }),
        };
        self.send_json(held.peer, &answer);
        held.message
    }
}

/// One connection to the fake server.
async fn serve<S: AsyncRead + AsyncWrite + Unpin + 'static>(inner: Rc<Inner>, stream: S) {
    let delay = inner.handshake_delay.get();
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let headers = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&headers);
    // The handshake callback's signature, with the large error it names.
    #[allow(clippy::result_large_err)]
    let capture = move |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
        *seen.borrow_mut() = request
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        Ok(response)
    };
    let Ok(socket) = accept_hdr_async_with_config(stream, capture, Some(config())).await else {
        return;
    };
    let (mut sink, mut stream) = socket.split();
    let (out, mut outgoing) = mpsc::unbounded_channel();
    let open = Rc::new(Cell::new(true));
    let kill = Rc::new(Notify::new());
    let pongs = Rc::new(Cell::new(0));
    let me = {
        let mut peers = inner.peers.borrow_mut();
        peers.push(Peer {
            out: out.clone(),
            headers: headers.borrow().clone(),
            open: Rc::clone(&open),
            kill: Rc::clone(&kill),
            pongs: Rc::clone(&pongs),
        });
        peers.len() - 1
    };
    let writing = async {
        while let Some(message) = outgoing.recv().await {
            if sink.send(message).await.is_err() {
                return;
            }
        }
    };
    let reading = async {
        if inner.stall.get() {
            std::future::pending::<()>().await;
        }
        while let Some(Ok(message)) = stream.next().await {
            match message {
                Message::Text(text) => {
                    inner.texts.borrow_mut().push((me, text.to_string()));
                    answer(&inner, me, &out, &text);
                }
                Message::Pong(_) => pongs.set(pongs.get() + 1),
                _ => {}
            }
        }
    };
    tokio::select! {
        () = writing => {}
        () = reading => {}
        () = kill.notified() => {}
    }
    open.set(false);
}

/// What the fake server does with one message of `peer`.
fn answer(inner: &Inner, peer: usize, out: &mpsc::UnboundedSender<Message>, text: &str) {
    let Ok(message) = serde_json::from_str::<Value>(text) else {
        return;
    };
    inner.requests.borrow_mut().push(message.clone());
    let reply = |value: Value| {
        let _ = out.send(Message::text(value.to_string()));
    };
    match inner.behaviour.get() {
        Behaviour::Silent => {}
        Behaviour::Refusing => {
            reply(json!({ "id": message["id"], "error": { "message": "not ready" } }));
        }
        Behaviour::Ready => {
            if message["method"] == "initialize" {
                reply(json!({ "id": message["id"], "result": {} }));
            } else if message.get("id").is_some() {
                inner.held.borrow_mut().push(Held { peer, message });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// A TUI

/// A TUI's connection to the broker: what the broker says to it is kept.
pub(crate) struct Tui {
    out: mpsc::UnboundedSender<Message>,
    seen: Rc<RefCell<Vec<Value>>>,
    /// Every text the broker sent, whether or not it is JSON serde_json reads.
    texts: Rc<RefCell<Vec<String>>>,
    pongs: Rc<Cell<usize>>,
    /// Whether the broker answered a close with a close of its own.
    goodbye: Rc<Cell<bool>>,
    closed: Rc<Cell<bool>>,
    kill: Rc<Notify>,
}

impl Tui {
    /// Connects to `address` at `path`, with the `authorization` header when
    /// there is one: the handshake's error when the broker turns it away.
    pub(crate) async fn connect(
        address: SocketAddr,
        path: &str,
        authorization: Option<&str>,
    ) -> Result<Self, tungstenite::Error> {
        let stream = TcpStream::connect(address).await?;
        let uri = format!("ws://{address}{path}")
            .parse()
            .map_err(tungstenite::Error::from)?;
        let mut request = ClientRequestBuilder::new(uri);
        if let Some(authorization) = authorization {
            request = request.with_header("authorization", authorization);
        }
        let (socket, _) = client_async_with_config(request, stream, Some(config())).await?;
        let (mut sink, mut stream) = socket.split();
        let (out, mut outgoing) = mpsc::unbounded_channel::<Message>();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let texts = Rc::new(RefCell::new(Vec::new()));
        let pongs = Rc::new(Cell::new(0));
        let goodbye = Rc::new(Cell::new(false));
        let closed = Rc::new(Cell::new(false));
        let kill = Rc::new(Notify::new());
        let (heard, said, ponged, bye, gone, stop) = (
            Rc::clone(&seen),
            Rc::clone(&texts),
            Rc::clone(&pongs),
            Rc::clone(&goodbye),
            Rc::clone(&closed),
            Rc::clone(&kill),
        );
        tokio::task::spawn_local(async move {
            let writing = async {
                while let Some(message) = outgoing.recv().await {
                    if sink.send(message).await.is_err() {
                        return;
                    }
                }
            };
            let reading = async {
                while let Some(Ok(message)) = stream.next().await {
                    match message {
                        Message::Text(text) => {
                            said.borrow_mut().push(text.to_string());
                            if let Ok(value) = serde_json::from_str(&text) {
                                heard.borrow_mut().push(value);
                            }
                        }
                        Message::Pong(_) => ponged.set(ponged.get() + 1),
                        Message::Close(_) => bye.set(true),
                        _ => {}
                    }
                }
            };
            tokio::select! {
                () = writing => {}
                () = reading => {}
                () = stop.notified() => {}
            }
            gone.set(true);
        });
        Ok(Self {
            out,
            seen,
            texts,
            pongs,
            goodbye,
            closed,
            kill,
        })
    }

    pub(crate) fn send(&self, value: Value) {
        self.send_message(Message::text(value.to_string()));
    }

    pub(crate) fn send_text(&self, text: &str) {
        self.send_message(Message::text(text));
    }

    pub(crate) fn send_message(&self, message: Message) {
        let _ = self.out.send(message);
    }

    /// Everything the broker has said to this TUI.
    pub(crate) fn seen(&self) -> Vec<Value> {
        self.seen.borrow().clone()
    }

    pub(crate) fn has_seen(&self, predicate: impl Fn(&Value) -> bool) -> bool {
        self.seen.borrow().iter().any(predicate)
    }

    /// Whether the broker answered request `id`.
    pub(crate) fn has_answered(&self, id: &Value) -> bool {
        self.has_seen(|message| &message["id"] == id && message.get("method").is_none())
    }

    /// Every text the broker has sent this TUI, as it came.
    pub(crate) fn texts(&self) -> Vec<String> {
        self.texts.borrow().clone()
    }

    pub(crate) fn pongs(&self) -> usize {
        self.pongs.get()
    }

    /// Whether the broker answered a close with a close.
    pub(crate) fn heard_goodbye(&self) -> bool {
        self.goodbye.get()
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.get()
    }

    /// Says goodbye, as a TUI that quits does.
    pub(crate) fn close(&self) {
        self.send_message(Message::Close(None));
    }

    /// Drops the connection with no goodbye.
    pub(crate) fn terminate(&self) {
        self.kill.notify_one();
    }
}

// ---------------------------------------------------------------------------
// The daemon's requests

/// A reply to a raw HTTP request.
pub(crate) struct Reply {
    pub(crate) status: u16,
    pub(crate) headers: String,
    pub(crate) body: Vec<u8>,
}

impl Reply {
    pub(crate) fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("a JSON body")
    }

    /// The value of a header, `name` in lower case.
    pub(crate) fn header(&self, name: &str) -> Option<String> {
        self.headers.lines().find_map(|line| {
            let (found, value) = line.split_once(':')?;
            (found.to_ascii_lowercase() == name).then(|| value.trim().to_string())
        })
    }
}

/// One HTTP request, written by hand, and its reply read to the end.
pub(crate) async fn http(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Reply {
    let mut stream = TcpStream::connect(address).await.expect("connect");
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nhost: 127.0.0.1\r\nconnection: close\r\ncontent-length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    // A server that has what it needs may close before it has read it all.
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body).await;
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw).await;
    parse(&raw)
}

fn parse(raw: &[u8]) -> Reply {
    let end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("no HTTP reply in {:?}", String::from_utf8_lossy(raw)));
    let head = String::from_utf8_lossy(&raw[..end]).into_owned();
    let (status, headers) = head.split_once("\r\n").unwrap_or((&head, ""));
    let status = status
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("a status");
    Reply {
        status,
        headers: headers.to_string(),
        body: raw[end + 4..].to_vec(),
    }
}

// ---------------------------------------------------------------------------
// A window

/// What a fixture's broker is started with.
pub(crate) struct Options {
    pub(crate) fresh_bypass: bool,
    pub(crate) board: Option<Arc<Board>>,
    pub(crate) question_wait: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            fresh_bypass: false,
            board: None,
            question_wait: Duration::from_secs(5),
        }
    }
}

/// A broker, the Codex server it is connected to, and the daemon's side of it.
pub(crate) struct Fixture {
    pub(crate) codex: FakeCodex,
    pub(crate) broker: Broker,
}

impl Fixture {
    pub(crate) async fn start() -> Self {
        Self::with(Options::default()).await
    }

    pub(crate) async fn with(options: Options) -> Self {
        let codex = FakeCodex::start(Behaviour::Ready).await;
        let broker = Broker::start(config_for(&codex, options))
            .await
            .expect("a broker");
        Self { codex, broker }
    }

    pub(crate) fn address(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.broker.port()))
    }

    /// A TUI that connected and initialized, as Codex's does.
    pub(crate) async fn connect(&self) -> Tui {
        let tui = Tui::connect(self.address(), "/", Some(&format!("Bearer {TOKEN}")))
            .await
            .expect("the broker takes it");
        tui.send(json!({
            "id": "init",
            "method": "initialize",
            "params": { "clientInfo": { "name": "codex-tui", "version": "test" } },
        }));
        wait(|| tui.has_answered(&json!("init"))).await;
        tui
    }

    /// `GET /session`.
    pub(crate) async fn read(&self) -> Value {
        http(
            self.address(),
            "GET",
            "/session",
            &[("authorization", &format!("Bearer {TOKEN}"))],
            b"",
        )
        .await
        .json()
    }

    /// `POST /deliver` of a message for `session`, `overrides` applied.
    pub(crate) async fn deliver(&self, session: &str, overrides: Value) -> Value {
        let mut record = json!({
            "launchId": "launch-1",
            "sessionId": session,
            "text": TEXT,
            "expiresAt": now_ms() + 2000.0,
        });
        for (key, value) in overrides.as_object().expect("overrides") {
            record[key] = value.clone();
        }
        self.post_deliver(record.to_string().as_bytes())
            .await
            .json()
    }

    pub(crate) async fn post_deliver(&self, body: &[u8]) -> Reply {
        http(
            self.address(),
            "POST",
            "/deliver",
            &[
                ("authorization", &format!("Bearer {TOKEN}")),
                ("content-type", "application/json"),
            ],
            body,
        )
        .await
    }

    pub(crate) async fn respond(&self, method: &str, result: Value) -> Value {
        self.codex.respond(method, result, None).await
    }

    pub(crate) async fn respond_error(&self, method: &str, error: Value) -> Value {
        self.codex.respond(method, Value::Null, Some(error)).await
    }

    /// The TUI says `method`; Codex answers it with `result`; the answer has
    /// reached the TUI, so the broker has taken it. Returns what Codex was asked.
    pub(crate) async fn call(
        &self,
        tui: &Tui,
        id: u64,
        method: &str,
        params: Value,
        result: Value,
    ) -> Value {
        tui.send(json!({ "id": id, "method": method, "params": params }));
        let asked = self.respond(method, result).await;
        wait(|| tui.has_answered(&json!(id))).await;
        asked["params"].clone()
    }

    /// The TUI starts its main thread, and Codex answers it. The broker takes
    /// the thread before it passes the answer on, so the TUI holding the
    /// answer means the broker has it.
    pub(crate) async fn start_thread(&self, tui: &Tui, id: u64, thread: Value) {
        self.call(
            tui,
            id,
            "thread/start",
            json!({ "ephemeral": false, "threadSource": "user" }),
            json!({ "thread": thread }),
        )
        .await;
    }
}

/// A broker's configuration for `codex`: any port, this launch.
pub(crate) fn config_for(codex: &FakeCodex, options: Options) -> Config {
    Config {
        bridge: Bridge {
            launch_id: "launch-1".into(),
            port: 0,
            token: TOKEN.into(),
        },
        upstream: Upstream {
            target: Target::Tcp(codex.address),
            authorization: None,
        },
        fresh_bypass: options.fresh_bypass,
        board: options.board,
        question_wait: options.question_wait,
    }
}
