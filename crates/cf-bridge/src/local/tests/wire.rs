//! What the scenarios stand on: bridges over in-memory pipes with the test as
//! the peer, and a runtime on one thread whose clock moves only when nothing
//! else can run, so a deadline costs no time and a wait that never ends fails
//! at once.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::Duration;

use cf_proto::bridge::Role;
use serde_json::{json, Value};
use tokio::io::{duplex, split, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::task::LocalSet;

use crate::local::{Bridge, BridgeBuilder};
use crate::BridgeError;

/// Room in a pipe: what a scenario writes never waits for the reader.
const PIPE_BYTES: usize = 256 * 1024;

/// How long a scenario waits for anything, on the clock that does not wait.
const PATIENCE: Duration = Duration::from_secs(60);

/// Runs a scenario as the daemon runs: on one thread, with the `LocalSet` the
/// bridge's tasks are spawned on.
pub(super) fn run<F: Future>(scenario: F) -> F::Output {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .expect("a runtime");
    LocalSet::new().block_on(&runtime, scenario)
}

/// Lets everything that can run, run: the clock moves on only after.
pub(super) async fn quiet() {
    tokio::time::sleep(Duration::from_millis(1)).await;
}

/// Waits until `condition` holds, polling; the scenario fails if it never does.
pub(super) async fn wait_for(condition: impl Fn() -> bool) {
    for _ in 0..400 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("timed out waiting");
}

/// `future`, or a failed scenario if it does not end.
pub(super) async fn within<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(PATIENCE, future)
        .await
        .expect("timed out waiting")
}

/// A frame as the peer writes it.
pub(super) fn frame(kind: &str, id: &str, op: &str, body: Value) -> Value {
    json!({ "v": 1, "id": id, "kind": kind, "op": op, "body": body })
}

/// What a bridge reports, as the text of each report.
pub(super) fn collector() -> (Rc<RefCell<Vec<String>>>, impl Fn(BridgeError) + 'static) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    (seen, move |error| sink.borrow_mut().push(error.to_string()))
}

/// The other end of a bridge that has nobody on it but the test: what the
/// peer would write, and what the bridge wrote.
pub(super) struct Wire {
    to_bridge: Option<DuplexStream>,
    written: Rc<RefCell<Vec<u8>>>,
    ended: Rc<Cell<bool>>,
}

impl Wire {
    pub(super) async fn send(&mut self, frame: Value) {
        self.send_bytes(format!("{frame}\n").as_bytes()).await;
    }

    pub(super) async fn send_text(&mut self, text: &str) {
        self.send_bytes(text.as_bytes()).await;
    }

    pub(super) async fn send_bytes(&mut self, bytes: &[u8]) {
        let pipe = self.to_bridge.as_mut().expect("the input is still open");
        pipe.write_all(bytes)
            .await
            .expect("the bridge reads its input");
    }

    /// Ends what the bridge reads, as a peer that is gone does.
    pub(super) fn end(&mut self) {
        self.to_bridge = None;
    }

    /// Everything the bridge wrote to its output.
    pub(super) fn text(&self) -> String {
        String::from_utf8(self.written.borrow().clone()).expect("the output is text")
    }

    /// What the bridge wrote, a frame per line: every line must be one.
    pub(super) fn frames(&self) -> Vec<Value> {
        self.text()
            .lines()
            .map(|line| serde_json::from_str(line).expect("a line of the output is a frame"))
            .collect()
    }

    pub(super) async fn wait_for_frames(&self, count: usize) -> Vec<Value> {
        wait_for(|| self.frames().len() >= count).await;
        self.frames()
    }

    /// Whether the bridge ended its output, so that a peer reading it sees it end.
    pub(super) fn output_ended(&self) -> bool {
        self.ended.get()
    }
}

/// A bridge, the daemon's end, with the test for its peer.
pub(super) fn lonely() -> (Bridge, Wire) {
    lonely_with(BridgeBuilder::new(Role::Daemon))
}

pub(super) fn lonely_with(builder: BridgeBuilder) -> (Bridge, Wire) {
    let (to_bridge, input) = duplex(PIPE_BYTES);
    let (output, mut from_bridge) = duplex(PIPE_BYTES);
    let (bridge, connection) = builder.connect(input, output);
    tokio::task::spawn_local(connection);
    let written = Rc::new(RefCell::new(Vec::new()));
    let ended = Rc::new(Cell::new(false));
    let (sink, done) = (Rc::clone(&written), Rc::clone(&ended));
    tokio::task::spawn_local(async move {
        let mut chunk = [0; 4096];
        while let Ok(count @ 1..) = from_bridge.read(&mut chunk).await {
            sink.borrow_mut().extend_from_slice(&chunk[..count]);
        }
        done.set(true);
    });
    let wire = Wire {
        to_bridge: Some(to_bridge),
        written,
        ended,
    };
    (bridge, wire)
}

/// A bridge whose output is a [`Sink`] the test can break.
pub(super) fn lonely_over(builder: BridgeBuilder, broken: &Rc<Cell<bool>>) -> (Bridge, Wire) {
    let (to_bridge, input) = duplex(PIPE_BYTES);
    let written = Rc::new(RefCell::new(Vec::new()));
    let ended = Rc::new(Cell::new(false));
    let sink = Sink {
        broken: Rc::clone(broken),
        written: Rc::clone(&written),
        ended: Rc::clone(&ended),
    };
    let (bridge, connection) = builder.connect(input, sink);
    tokio::task::spawn_local(connection);
    let wire = Wire {
        to_bridge: Some(to_bridge),
        written,
        ended,
    };
    (bridge, wire)
}

/// An output that takes what is written until the test breaks it.
struct Sink {
    broken: Rc<Cell<bool>>,
    written: Rc<RefCell<Vec<u8>>>,
    ended: Rc<Cell<bool>>,
}

impl AsyncWrite for Sink {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.broken.get() {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe broke")));
        }
        self.written.borrow_mut().extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.ended.set(true);
        Poll::Ready(Ok(()))
    }
}

/// Two bridges talking to each other over in-memory pipes, as Node and Rust
/// do: `a` is the daemon's end and `b` the pane host's.
pub(super) fn pair() -> (Bridge, Bridge) {
    pair_of(
        BridgeBuilder::new(Role::Daemon),
        BridgeBuilder::new(Role::Host),
    )
}

pub(super) fn pair_of(a: BridgeBuilder, b: BridgeBuilder) -> (Bridge, Bridge) {
    let (a_end, b_end) = duplex(PIPE_BYTES);
    let (a_input, a_output) = split(a_end);
    let (b_input, b_output) = split(b_end);
    let (a, a_connection) = a.connect(a_input, a_output);
    let (b, b_connection) = b.connect(b_input, b_output);
    tokio::task::spawn_local(a_connection);
    tokio::task::spawn_local(b_connection);
    (a, b)
}
