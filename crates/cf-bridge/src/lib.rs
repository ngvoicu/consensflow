//! The bridge's transport on threads: one end of the JSON-lines protocol
//! (`cf_proto::bridge`) over a reader and a writer, with requests matched to
//! their responses, handlers run off the reader's thread, a bounded writer
//! queue, frame size limits and an orderly shutdown. The pane host runs it as
//! [`Role::Host`]; which end a program is decides the ids it mints and the
//! ones it accepts. With the `local` feature, [`local`] is the daemon's end of
//! the same protocol: one thread, over tokio streams.

// One function calls into C, to make a pipe non-blocking; everything else is safe Rust.
#![deny(unsafe_code)]
#![deny(clippy::undocumented_unsafe_blocks)]

mod requests;
mod shutdown;
mod transport;
mod writer;

#[cfg(feature = "local")]
pub mod local;

#[cfg(test)]
mod queue_tests;

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use cf_proto::bridge::Role;
use serde_json::Value;

use crate::requests::{PendingRequest, RequestHandlerEntry};
use crate::shutdown::TransportShutdown;
use crate::writer::WriteJob;

#[cfg(all(test, unix))]
pub(crate) use cf_proto::bridge::Frame;

const WRITER_QUEUE_CAPACITY: usize = 32;
/// The most of the writer queue a stream may fill, so a response always finds
/// room behind a burst of pane output.
const STREAM_QUEUE_SHARE: usize = WRITER_QUEUE_CAPACITY / 2;
const DEFAULT_REQUEST_DEADLINE_MS: u64 = 30_000;
/// How often a frame that waits for room says so, to `on_error`: a peer that
/// takes nothing is a busy one until it is told of, and the log says it. Only a
/// frame that is still trying to be queued says it: a writer held by one write
/// with nothing queued behind it has nobody to.
const STALL_REPORT_MS: u64 = 30_000;
/// How long a frame that waits for room sleeps between its looks at the queue: a millisecond
/// for the first hundred, then ten, so that a long wait costs little.
const FIRST_LOOKS: u32 = 100;

type RequestHandler = dyn Fn(Bridge, Value) -> Result<Value, String> + Send + Sync;
type EventHandler = dyn Fn(Value) + Send + Sync;
type ErrorHandler = dyn Fn(BridgeError) + Send + Sync;
type CloseHandler = dyn Fn() + Send + Sync;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BridgeError {
    Eof,
    Io(String),
    InvalidHandle(String),
    MalformedFrame(String),
    LockPoisoned,
}

impl fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eof => write!(formatter, "eof"),
            Self::Io(message) => write!(formatter, "bridge I/O error: {message}"),
            Self::InvalidHandle(message) => write!(formatter, "invalid handle line: {message}"),
            Self::MalformedFrame(message) => write!(formatter, "malformed frame: {message}"),
            Self::LockPoisoned => write!(formatter, "bridge lock is poisoned"),
        }
    }
}

impl std::error::Error for BridgeError {}

impl From<std::io::Error> for BridgeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

struct Lifecycle {
    closed: bool,
    in_flight_handlers: usize,
    in_flight_launches: usize,
    transport_workers: usize,
}

struct BridgeInner {
    role: Role,
    writer: Mutex<Option<mpsc::SyncSender<WriteJob>>>,
    /// Frames in the writer queue the writer has not taken yet.
    queued: AtomicUsize,
    pending: Mutex<HashMap<String, PendingRequest>>,
    handlers: HashMap<String, RequestHandlerEntry>,
    event_handlers: HashMap<String, Vec<Arc<EventHandler>>>,
    on_error: Option<Arc<ErrorHandler>>,
    on_close: Option<Arc<CloseHandler>>,
    next_id: AtomicU64,
    closed: AtomicBool,
    /// Whether the output stays open when the input ends cleanly
    /// ([`BridgeBuilder::keep_output_after_eof`]).
    keeps_output: bool,
    /// Whether the output has ended, which a bridge that keeps it says apart from
    /// `closed`; one that does not has it end with `closed`.
    output_ended: AtomicBool,
    lifecycle: Mutex<Lifecycle>,
    closed_ready: Condvar,
    transport_shutdown: Arc<TransportShutdown>,
    max_frame_bytes: usize,
    default_request_deadline: Duration,
    /// The responses waiting for room now.
    responses_waiting: AtomicUsize,
    /// How often a frame that waits for room says so ([`STALL_REPORT_MS`]).
    stall_report: Duration,
    /// When a frame that waited for room last said so, in milliseconds since the
    /// bridge was built: 0 before any did.
    stalled_told: AtomicU64,
    built: Instant,
}

impl BridgeInner {
    /// Whether nothing more can be said to the peer: the transport closed, and the
    /// output with it, unless the bridge keeps its output past the end of its input
    /// and has not ended it.
    fn output_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
            && (!self.keeps_output || self.output_ended.load(Ordering::Acquire))
    }
}

#[derive(Clone)]
pub struct Bridge {
    inner: Arc<BridgeInner>,
}

pub struct ConnectedBridge {
    pub handle: Value,
    pub bridge: Bridge,
}

pub struct BridgeBuilder {
    role: Role,
    handlers: HashMap<String, RequestHandlerEntry>,
    event_handlers: HashMap<String, Vec<Arc<EventHandler>>>,
    on_error: Option<Arc<ErrorHandler>>,
    on_close: Option<Arc<CloseHandler>>,
    keeps_output: bool,
    max_frame_bytes: usize,
    default_request_deadline: Duration,
    stall_report: Duration,
}

impl BridgeBuilder {
    /// A bridge for the `role` end, refusing frames over `max_frame_bytes`.
    pub fn new(role: Role, max_frame_bytes: usize) -> Self {
        Self {
            role,
            handlers: HashMap::new(),
            event_handlers: HashMap::new(),
            on_error: None,
            on_close: None,
            keeps_output: false,
            max_frame_bytes,
            default_request_deadline: Duration::from_millis(DEFAULT_REQUEST_DEADLINE_MS),
            stall_report: Duration::from_millis(STALL_REPORT_MS),
        }
    }

    #[cfg(test)]
    fn with_stall_report_ms(mut self, milliseconds: u64) -> Self {
        self.stall_report = Duration::from_millis(milliseconds);
        self
    }

    #[cfg(test)]
    #[cfg_attr(not(unix), allow(dead_code))]
    fn with_default_request_deadline_ms(mut self, milliseconds: u64) -> Self {
        self.default_request_deadline = Duration::from_millis(milliseconds);
        self
    }

    pub fn on<F>(&mut self, op: impl Into<String>, handler: F) -> &mut Self
    where
        F: Fn(Bridge, Value) -> Result<Value, String> + Send + Sync + 'static,
    {
        self.handlers.insert(
            op.into(),
            RequestHandlerEntry {
                handler: Arc::new(handler),
                launches_panes: false,
            },
        );
        self
    }

    pub fn on_launch<F>(&mut self, op: impl Into<String>, handler: F) -> &mut Self
    where
        F: Fn(Bridge, Value) -> Result<Value, String> + Send + Sync + 'static,
    {
        self.handlers.insert(
            op.into(),
            RequestHandlerEntry {
                handler: Arc::new(handler),
                launches_panes: true,
            },
        );
        self
    }

    pub fn on_event<F>(&mut self, op: impl Into<String>, handler: F) -> &mut Self
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        self.event_handlers
            .entry(op.into())
            .or_default()
            .push(Arc::new(handler));
        self
    }

    pub fn on_error<F>(&mut self, handler: F) -> &mut Self
    where
        F: Fn(BridgeError) + Send + Sync + 'static,
    {
        self.on_error = Some(Arc::new(handler));
        self
    }

    /// Told once, when the transport closes for whatever reason, after every
    /// request still waiting on it has been answered.
    pub fn on_close<F>(&mut self, handler: F) -> &mut Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_close = Some(Arc::new(handler));
        self
    }

    /// A bridge that, when its input ends cleanly (the peer closed it, as a parent
    /// tells a child it is done), goes on writing: the owner has still to say what
    /// it did as it stopped, and ends the output itself ([`Bridge::end_output`]).
    /// Nothing more is admitted from the peer, and the bridge is closed
    /// ([`Bridge::is_closed`]) as ever; only the output is kept. A failure of the
    /// transport ends both, as it always did. Without this the output ends with the
    /// input, and what is said after is dropped.
    pub fn keep_output_after_eof(&mut self) -> &mut Self {
        self.keeps_output = true;
        self
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    mod unix {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::os::fd::{AsRawFd, RawFd};
        use std::os::unix::net::UnixStream;
        use std::sync::{Arc, Barrier, Condvar, Mutex};
        use std::thread;
        use std::time::{Duration, Instant};

        use cf_proto::bridge::Role;
        use serde_json::{json, Value};

        use super::super::{
            BridgeBuilder, BridgeError, Frame, DEFAULT_REQUEST_DEADLINE_MS, WRITER_QUEUE_CAPACITY,
        };

        struct Peer {
            reader: BufReader<UnixStream>,
            writer: UnixStream,
        }

        struct GatedWriter {
            state: Arc<(Mutex<GatedWriterState>, Condvar)>,
            entered: Option<std::sync::mpsc::Sender<()>>,
        }

        struct GatedWriterState {
            released: bool,
            bytes: Vec<u8>,
        }

        struct WriteGate {
            state: Arc<(Mutex<GatedWriterState>, Condvar)>,
            entered: std::sync::mpsc::Receiver<()>,
        }

        struct PartialFailWriter {
            state: Arc<(Mutex<PartialFailState>, Condvar)>,
            entered: Option<std::sync::mpsc::Sender<()>>,
        }

        struct PartialFailState {
            released: bool,
            wrote_prefix: bool,
            bytes: Vec<u8>,
        }

        struct PartialFailGate {
            state: Arc<(Mutex<PartialFailState>, Condvar)>,
            entered: std::sync::mpsc::Receiver<()>,
        }

        struct FragmentedReader {
            chunks: std::sync::mpsc::Receiver<Vec<u8>>,
            current: Vec<u8>,
            at: usize,
        }

        struct TrackedStream {
            stream: UnixStream,
            write_blocked: Option<std::sync::mpsc::Sender<()>>,
            dropped: Option<std::sync::mpsc::Sender<()>>,
        }

        struct EndpointProbe {
            write_blocked: Option<std::sync::mpsc::Receiver<()>>,
            dropped: std::sync::mpsc::Receiver<()>,
        }

        struct DropGatedReader {
            inner: TrackedStream,
            drop_started: Option<std::sync::mpsc::Sender<()>>,
            drop_release: std::sync::mpsc::Receiver<()>,
        }

        struct ReaderDropGate {
            drop_started: std::sync::mpsc::Receiver<()>,
            drop_release: std::sync::mpsc::Sender<()>,
        }

        impl Read for FragmentedReader {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                if bytes.is_empty() {
                    return Ok(0);
                }
                while self.at == self.current.len() {
                    match self.chunks.recv() {
                        Ok(chunk) => {
                            self.current = chunk;
                            self.at = 0;
                        }
                        Err(_) => return Ok(0),
                    }
                }
                let byte_count = (self.current.len() - self.at).min(bytes.len());
                bytes[..byte_count].copy_from_slice(&self.current[self.at..self.at + byte_count]);
                self.at += byte_count;
                Ok(byte_count)
            }
        }

        impl Read for TrackedStream {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                self.stream.read(bytes)
            }
        }

        impl Read for DropGatedReader {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                self.inner.read(bytes)
            }
        }

        impl Write for TrackedStream {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                let result = self.stream.write(bytes);
                if result
                    .as_ref()
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
                {
                    if let Some(write_blocked) = self.write_blocked.take() {
                        let _ = write_blocked.send(());
                    }
                }
                result
            }

            fn flush(&mut self) -> std::io::Result<()> {
                self.stream.flush()
            }
        }

        impl AsRawFd for TrackedStream {
            fn as_raw_fd(&self) -> RawFd {
                self.stream.as_raw_fd()
            }
        }

        impl AsRawFd for DropGatedReader {
            fn as_raw_fd(&self) -> RawFd {
                self.inner.as_raw_fd()
            }
        }

        impl Drop for TrackedStream {
            fn drop(&mut self) {
                if let Some(dropped) = self.dropped.take() {
                    let _ = dropped.send(());
                }
            }
        }

        impl Drop for DropGatedReader {
            fn drop(&mut self) {
                if let Some(drop_started) = self.drop_started.take() {
                    let _ = drop_started.send(());
                }
                let _ = self.drop_release.recv();
            }
        }

        impl EndpointProbe {
            fn wait_until_write_blocked(&mut self) {
                self.write_blocked
                    .take()
                    .expect("blocked-write probe")
                    .recv_timeout(Duration::from_secs(1))
                    .expect("real output descriptor reached WouldBlock");
            }

            fn wait_for_drop(&self, timeout: Duration) -> bool {
                self.dropped.recv_timeout(timeout).is_ok()
            }

            fn is_dropped(&self) -> bool {
                self.dropped.try_recv().is_ok()
            }
        }

        impl ReaderDropGate {
            fn wait_until_drop_started(&self) {
                self.drop_started
                    .recv_timeout(Duration::from_secs(1))
                    .expect("reader endpoint entered gated destruction");
            }

            fn release_after(self, delay: Duration) -> thread::JoinHandle<()> {
                thread::spawn(move || {
                    thread::sleep(delay);
                    let _ = self.drop_release.send(());
                })
            }
        }

        impl Write for GatedWriter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if let Some(entered) = self.entered.take() {
                    let _ = entered.send(());
                }
                let (state, ready) = &*self.state;
                let mut state = state.lock().expect("gated writer lock");
                while !state.released {
                    state = ready.wait(state).expect("gated writer wait");
                }
                state.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl WriteGate {
            fn wait_until_entered(&self) {
                self.entered
                    .recv_timeout(Duration::from_secs(1))
                    .expect("writer entered blocking write");
            }

            fn release(&self) {
                let (state, ready) = &*self.state;
                state.lock().expect("gated writer lock").released = true;
                ready.notify_all();
            }

            fn bytes(&self) -> Vec<u8> {
                self.state
                    .0
                    .lock()
                    .expect("gated writer lock")
                    .bytes
                    .clone()
            }
        }

        impl Write for PartialFailWriter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                let (state, ready) = &*self.state;
                let mut state = state.lock().expect("partial writer lock");
                if !state.wrote_prefix {
                    let byte_count = bytes.len().min(7);
                    state.bytes.extend_from_slice(&bytes[..byte_count]);
                    state.wrote_prefix = true;
                    drop(state);
                    if let Some(entered) = self.entered.take() {
                        let _ = entered.send(());
                    }
                    return Ok(byte_count);
                }
                while !state.released {
                    state = ready.wait(state).expect("partial writer wait");
                }
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "injected partial write failure",
                ))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl PartialFailGate {
            fn wait_until_partial_write(&self) {
                self.entered
                    .recv_timeout(Duration::from_secs(1))
                    .expect("writer emitted a partial frame");
            }

            fn release_failure(&self) {
                let (state, ready) = &*self.state;
                state.lock().expect("partial writer lock").released = true;
                ready.notify_all();
            }

            fn bytes(&self) -> Vec<u8> {
                self.state
                    .0
                    .lock()
                    .expect("partial writer lock")
                    .bytes
                    .clone()
            }
        }

        fn gated_writer() -> (GatedWriter, WriteGate) {
            let state = Arc::new((
                Mutex::new(GatedWriterState {
                    released: false,
                    bytes: Vec::new(),
                }),
                Condvar::new(),
            ));
            let (entered_sender, entered) = std::sync::mpsc::channel();
            (
                GatedWriter {
                    state: Arc::clone(&state),
                    entered: Some(entered_sender),
                },
                WriteGate { state, entered },
            )
        }

        fn partial_fail_writer() -> (PartialFailWriter, PartialFailGate) {
            let state = Arc::new((
                Mutex::new(PartialFailState {
                    released: false,
                    wrote_prefix: false,
                    bytes: Vec::new(),
                }),
                Condvar::new(),
            ));
            let (entered_sender, entered) = std::sync::mpsc::channel();
            (
                PartialFailWriter {
                    state: Arc::clone(&state),
                    entered: Some(entered_sender),
                },
                PartialFailGate { state, entered },
            )
        }

        fn tracked_stream(
            stream: UnixStream,
            track_writes: bool,
        ) -> (TrackedStream, EndpointProbe) {
            let (write_blocked, write_receiver) = if track_writes {
                let (sender, receiver) = std::sync::mpsc::channel();
                (Some(sender), Some(receiver))
            } else {
                (None, None)
            };
            let (dropped, drop_receiver) = std::sync::mpsc::channel();
            (
                TrackedStream {
                    stream,
                    write_blocked,
                    dropped: Some(dropped),
                },
                EndpointProbe {
                    write_blocked: write_receiver,
                    dropped: drop_receiver,
                },
            )
        }

        fn connect_tracked_split(
            builder: BridgeBuilder,
        ) -> (
            super::super::ConnectedBridge,
            UnixStream,
            UnixStream,
            EndpointProbe,
            EndpointProbe,
        ) {
            let (rust_input, mut peer_input) =
                UnixStream::pair().expect("create tracked input socket pair");
            let (rust_output, peer_output) =
                UnixStream::pair().expect("create tracked output socket pair");
            serde_json::to_writer(
                &mut peer_input,
                &json!({"url":"http://127.0.0.1:1234","token":"secret"}),
            )
            .expect("serialize tracked handle");
            peer_input
                .write_all(b"\n")
                .expect("terminate tracked handle");
            peer_input.flush().expect("flush tracked handle");
            let (rust_input, input_probe) = tracked_stream(rust_input, false);
            let (rust_output, output_probe) = tracked_stream(rust_output, true);
            let connected = builder
                .connect(rust_input, rust_output)
                .expect("connect tracked bridge");
            (
                connected,
                peer_input,
                peer_output,
                input_probe,
                output_probe,
            )
        }

        fn connect_with_gated_reader_drop() -> (
            super::super::ConnectedBridge,
            UnixStream,
            UnixStream,
            EndpointProbe,
            ReaderDropGate,
        ) {
            let (rust_input, mut peer_input) =
                UnixStream::pair().expect("create gated input socket pair");
            let (rust_output, peer_output) =
                UnixStream::pair().expect("create gated output socket pair");
            serde_json::to_writer(
                &mut peer_input,
                &json!({"url":"http://127.0.0.1:1234","token":"secret"}),
            )
            .expect("serialize gated handle");
            peer_input.write_all(b"\n").expect("terminate gated handle");
            peer_input.flush().expect("flush gated handle");

            let (rust_input, input_probe) = tracked_stream(rust_input, false);
            let (drop_started, drop_started_receiver) = std::sync::mpsc::channel();
            let (drop_release, drop_release_receiver) = std::sync::mpsc::channel();
            let reader = DropGatedReader {
                inner: rust_input,
                drop_started: Some(drop_started),
                drop_release: drop_release_receiver,
            };
            let connected = BridgeBuilder::new(Role::Host, 1024)
                .connect(reader, rust_output)
                .expect("connect bridge with gated reader destruction");
            (
                connected,
                peer_input,
                peer_output,
                input_probe,
                ReaderDropGate {
                    drop_started: drop_started_receiver,
                    drop_release,
                },
            )
        }

        fn occupy_real_output(bridge: &super::super::Bridge, output_probe: &mut EndpointProbe) {
            assert!(bridge
                .event("occupy", json!({"payload":"x".repeat(8 * 1024 * 1024)}))
                .expect("queue oversized real output payload"));
            output_probe.wait_until_write_blocked();
        }

        fn connect_with_writer<W: Write + Send + 'static>(
            builder: BridgeBuilder,
            output: W,
        ) -> (super::super::ConnectedBridge, UnixStream) {
            let (rust_input, mut peer_input) =
                UnixStream::pair().expect("create input socket pair");
            serde_json::to_writer(
                &mut peer_input,
                &json!({"url":"http://127.0.0.1:1234","token":"secret"}),
            )
            .expect("serialize handle");
            peer_input.write_all(b"\n").expect("terminate handle");
            peer_input.flush().expect("flush handle");
            let connected = builder
                .connect_uninterruptible(rust_input, output)
                .expect("connect Rust bridge");
            (connected, peer_input)
        }

        fn send_frame(output: &mut UnixStream, frame: Value) {
            let frame: Frame = serde_json::from_value(frame).expect("parse peer frame");
            serde_json::to_writer(&mut *output, &frame).expect("serialize peer frame");
            output.write_all(b"\n").expect("terminate peer frame");
            output.flush().expect("flush peer frame");
        }

        impl Peer {
            fn send(&mut self, frame: Value) {
                serde_json::to_writer(&mut self.writer, &frame).expect("serialize peer frame");
                self.writer.write_all(b"\n").expect("terminate peer frame");
                self.writer.flush().expect("flush peer frame");
            }

            fn receive(&mut self) -> Value {
                let mut line = String::new();
                self.reader.read_line(&mut line).expect("read bridge frame");
                assert!(!line.is_empty(), "bridge reached EOF before a frame");
                serde_json::from_str(line.trim_end()).expect("parse bridge frame")
            }

            fn set_read_timeout(&self, timeout: Duration) {
                self.reader
                    .get_ref()
                    .set_read_timeout(Some(timeout))
                    .expect("set peer read timeout");
            }
        }

        fn connect(
            builder: BridgeBuilder,
            initial_after_handle: Option<Value>,
        ) -> (super::super::ConnectedBridge, Peer) {
            let (rust, peer) = UnixStream::pair().expect("create bridge socket pair");
            let mut peer_writer = peer.try_clone().expect("clone peer writer");
            let handle = json!({"url":"http://127.0.0.1:1234","token":"secret"});
            serde_json::to_writer(&mut peer_writer, &handle).expect("serialize handle");
            peer_writer.write_all(b"\n").expect("terminate handle");
            if let Some(frame) = initial_after_handle {
                serde_json::to_writer(&mut peer_writer, &frame).expect("serialize first frame");
                peer_writer.write_all(b"\n").expect("terminate first frame");
            }
            peer_writer.flush().expect("flush initial bytes");

            let connected = builder
                .connect(rust.try_clone().expect("clone Rust reader"), rust)
                .expect("connect Rust bridge");
            assert_eq!(connected.handle, handle);
            (
                connected,
                Peer {
                    reader: BufReader::new(peer),
                    writer: peer_writer,
                },
            )
        }

        /// Whether the bridge closes within `within`: one that does not is a failure of the
        /// test that asks, not a test that never ends.
        fn closes_within(bridge: &super::super::Bridge, within: Duration) -> bool {
            let (closed, closing) = std::sync::mpsc::channel();
            let waiting = bridge.clone();
            thread::spawn(move || {
                let _ = closed.send(waiting.wait_closed().is_ok());
            });
            closing.recv_timeout(within).unwrap_or(false)
        }

        fn connect_split(builder: BridgeBuilder) -> (super::super::ConnectedBridge, Peer) {
            let (rust_input, mut peer_writer) =
                UnixStream::pair().expect("create bridge input socket pair");
            let (rust_output, peer_output) =
                UnixStream::pair().expect("create bridge output socket pair");
            let handle = json!({"url":"http://127.0.0.1:1234","token":"secret"});
            serde_json::to_writer(&mut peer_writer, &handle).expect("serialize handle");
            peer_writer.write_all(b"\n").expect("terminate handle");
            peer_writer.flush().expect("flush handle");

            let connected = builder
                .connect(rust_input, rust_output)
                .expect("connect Rust bridge");
            assert_eq!(connected.handle, handle);
            (
                connected,
                Peer {
                    reader: BufReader::new(peer_output),
                    writer: peer_writer,
                },
            )
        }

        fn connect_fragmented(
            builder: BridgeBuilder,
        ) -> (
            super::super::ConnectedBridge,
            std::sync::mpsc::Sender<Vec<u8>>,
            BufReader<UnixStream>,
        ) {
            let (chunks, fragmented) = std::sync::mpsc::channel();
            let mut handle =
                serde_json::to_vec(&json!({"url":"http://127.0.0.1:1234","token":"secret"}))
                    .expect("serialize handle");
            handle.push(b'\n');
            chunks.send(handle).expect("seed handle line");
            let (rust_output, peer_output) =
                UnixStream::pair().expect("create bridge output socket pair");
            let connected = builder
                .connect_uninterruptible(
                    FragmentedReader {
                        chunks: fragmented,
                        current: Vec::new(),
                        at: 0,
                    },
                    rust_output,
                )
                .expect("connect fragmented Rust bridge");
            (connected, chunks, BufReader::new(peer_output))
        }

        fn send_fragmented(
            chunks: &std::sync::mpsc::Sender<Vec<u8>>,
            frame: Value,
            chunk_bytes: usize,
        ) {
            let frame: Frame = serde_json::from_value(frame).expect("parse fragmented frame");
            let mut encoded = serde_json::to_vec(&frame).expect("serialize fragmented frame");
            encoded.push(b'\n');
            for chunk in encoded.chunks(chunk_bytes) {
                chunks
                    .send(chunk.to_vec())
                    .expect("send fragmented frame chunk");
            }
        }

        #[test]
        fn handle_and_first_frame_in_one_read_keep_the_frame_buffered() {
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            builder.on("ping", |_bridge, body| Ok(json!({"echo":body})));
            let first = json!({
                "v":1,
                "id":"n-1",
                "kind":"req",
                "op":"ping",
                "body":{"value":7}
            });

            let (_connected, mut peer) = connect(builder, Some(first));

            assert_eq!(
                peer.receive(),
                json!({
                    "v":1,
                    "id":"n-1",
                    "kind":"res",
                    "op":"ping",
                    "body":{"echo":{"value":7}}
                })
            );
        }

        #[test]
        fn oversized_fragmented_handle_fails_before_newline_or_eof() {
            let (rust_input, mut peer_input) =
                UnixStream::pair().expect("create handshake socket pair");
            let (result_sender, result_receiver) = std::sync::mpsc::channel();
            let connector = thread::spawn(move || {
                let result = BridgeBuilder::new(Role::Host, 32)
                    .connect_uninterruptible(rust_input, Vec::<u8>::new())
                    .map(|_| ());
                let _ = result_sender.send(result);
            });
            for fragment in [b'x'; 33].chunks(7) {
                peer_input
                    .write_all(fragment)
                    .expect("write oversized handle fragment");
            }
            peer_input.flush().expect("flush oversized handle");
            let failed_before_delimiter = result_receiver.recv_timeout(Duration::from_millis(150));
            let failed_on_time = failed_before_delimiter.is_ok();

            drop(peer_input);
            let result = failed_before_delimiter
                .or_else(|_| result_receiver.recv_timeout(Duration::from_secs(1)))
                .expect("connector eventually finishes");
            connector.join().expect("connector thread");

            assert!(matches!(result, Err(BridgeError::InvalidHandle(_))));
            assert!(failed_on_time, "oversized handshake waited for EOF");
        }

        #[test]
        fn fragmented_oversized_frame_is_discarded_through_newline_then_reader_recovers() {
            let (error_sender, error_receiver) = std::sync::mpsc::channel();
            let mut builder = BridgeBuilder::new(Role::Host, 128);
            builder.on("ping", |_bridge, _body| Ok(json!({"ok":true})));
            builder.on_error(move |error| {
                let _ = error_sender.send(error);
            });
            let (connected, mut peer) = connect(builder, None);

            for fragment in [b'x'; 129].chunks(17) {
                peer.writer
                    .write_all(fragment)
                    .expect("write oversized frame fragment");
                peer.writer.flush().expect("flush frame fragment");
            }
            peer.writer
                .write_all(b"discarded\n")
                .expect("finish discarded line");
            peer.writer.flush().expect("flush discarded line");
            error_receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("oversized line diagnostic");
            peer.send(json!({
                "v":1,
                "id":"n-after-overflow",
                "kind":"req",
                "op":"ping",
                "body":null
            }));
            assert_eq!(
                peer.receive(),
                json!({
                    "v":1,
                    "id":"n-after-overflow",
                    "kind":"res",
                    "op":"ping",
                    "body":{"ok":true}
                })
            );
            assert!(!connected.bridge.is_closed());
        }

        #[test]
        fn rust_requests_events_and_responses_have_the_exact_frame_shape() {
            let (event_sender, event_receiver) = std::sync::mpsc::channel();
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            builder.on_event("state.changed", move |body| {
                event_sender.send(body).expect("record event");
            });
            let (connected, mut peer) = connect(builder, None);

            let requester = connected.bridge.clone();
            let request = thread::spawn(move || {
                requester.request("consult", json!({"name":"nyx"}), Some(500))
            });
            let outgoing = peer.receive();
            assert_eq!(
                outgoing,
                json!({
                    "v":1,
                    "id":"r-1",
                    "kind":"req",
                    "op":"consult",
                    "body":{"name":"nyx"}
                })
            );
            peer.send(json!({
                "v":1,
                "id":"r-1",
                "kind":"res",
                "op":"consult",
                "body":{"ok":true}
            }));
            assert_eq!(
                request
                    .join()
                    .expect("request thread")
                    .expect("request reply"),
                json!({"ok":true})
            );

            assert!(connected
                .bridge
                .event("pane.output", json!({"seq":1}))
                .expect("write event"));
            assert_eq!(
                peer.receive(),
                json!({
                    "v":1,
                    "id":"r-2",
                    "kind":"evt",
                    "op":"pane.output",
                    "body":{"seq":1}
                })
            );

            peer.send(json!({
                "v":1,
                "id":"n-2",
                "kind":"evt",
                "op":"state.changed",
                "body":{"reason":"board"}
            }));
            assert_eq!(
                event_receiver
                    .recv_timeout(Duration::from_secs(1))
                    .expect("receive state.changed"),
                json!({"reason":"board"})
            );
        }

        #[test]
        fn deadline_returns_error_body_and_late_response_is_dropped() {
            let (connected, mut peer) = connect(BridgeBuilder::new(Role::Host, 1024), None);

            assert_eq!(
                connected
                    .bridge
                    .request("slow", json!(null), Some(20))
                    .expect("deadline result"),
                json!({"ok":false,"error":"deadline"})
            );
            assert_eq!(peer.receive()["id"], "r-1");
            peer.send(json!({
                "v":1,
                "id":"r-1",
                "kind":"res",
                "op":"slow",
                "body":{"late":true}
            }));

            let requester = connected.bridge.clone();
            let request = thread::spawn(move || requester.request("next", json!(null), Some(500)));
            let next = peer.receive();
            assert_eq!(next["id"], "r-2");
            peer.send(json!({
                "v":1,
                "id":"r-2",
                "kind":"res",
                "op":"next",
                "body":{"ok":true}
            }));
            assert_eq!(
                request.join().expect("request thread").expect("next reply"),
                json!({"ok":true})
            );
        }

        #[test]
        fn request_deadline_starts_before_a_blocked_writer() {
            let (writer, gate) = gated_writer();
            let (connected, peer) =
                connect_with_writer(BridgeBuilder::new(Role::Host, 1024), writer);
            let first_bridge = connected.bridge.clone();
            let first = thread::spawn(move || first_bridge.request("first", json!(null), None));
            gate.wait_until_entered();

            let second_bridge = connected.bridge.clone();
            let (second_sender, second_receiver) = std::sync::mpsc::channel();
            let second = thread::spawn(move || {
                let _ = second_sender.send(second_bridge.request("second", json!(null), Some(20)));
            });
            // Its own 20 ms deadline answers long before this; a deadline that
            // started only after the write would wait here until the gate
            // opens, and the gate opens only after this. A busy runner once
            // took longer than 150 ms just to schedule the threads.
            let on_time = second_receiver.recv_timeout(Duration::from_secs(2));
            let finished_on_time = on_time.is_ok();

            gate.release();
            drop(peer);
            let eventual = on_time
                .or_else(|_| second_receiver.recv_timeout(Duration::from_secs(1)))
                .expect("second request eventually finishes")
                .expect("deadline result");
            first.join().expect("first request thread").unwrap_err();
            second.join().expect("second request thread");

            assert_eq!(eventual, json!({"ok":false,"error":"deadline"}));
            assert!(
                finished_on_time,
                "deadline did not run while the request waited on the writer"
            );
        }

        #[test]
        fn blocked_unknown_and_oversized_replies_do_not_stall_input_dispatch() {
            let (writer, gate) = gated_writer();
            let (connected, mut peer) =
                connect_with_writer(BridgeBuilder::new(Role::Host, 256), writer);
            let requester = connected.bridge.clone();
            let (result_sender, result_receiver) = std::sync::mpsc::channel();
            let request = thread::spawn(move || {
                let _ = result_sender.send(requester.request("consult", json!(null), None));
            });
            gate.wait_until_entered();

            send_frame(
                &mut peer,
                json!({"v":1,"id":"n-unknown","kind":"req","op":"unknown","body":null}),
            );
            send_frame(
                &mut peer,
                json!({
                    "v":1,
                    "id":"n-large",
                    "kind":"req",
                    "op":"large",
                    "body":{"payload":"x".repeat(500)}
                }),
            );
            send_frame(
                &mut peer,
                json!({
                    "v":1,
                    "id":"r-1",
                    "kind":"res",
                    "op":"consult",
                    "body":{"ok":true}
                }),
            );
            let settled_while_output_blocked =
                result_receiver.recv_timeout(Duration::from_millis(150));
            let settled_on_time = settled_while_output_blocked.is_ok();

            gate.release();
            drop(peer);
            let result = settled_while_output_blocked
                .or_else(|_| result_receiver.recv_timeout(Duration::from_secs(1)))
                .expect("request eventually settles")
                .expect("request response");
            request.join().expect("request thread");

            assert_eq!(result, json!({"ok":true}));
            assert!(
                settled_on_time,
                "reader dispatch stalled behind an outbound refusal"
            );
        }

        #[test]
        fn stream_events_wait_for_a_busy_peer_and_leave_room_for_responses() {
            let (writer, gate) = gated_writer();
            let (connected, mut peer) =
                connect_with_writer(BridgeBuilder::new(Role::Host, 1024), writer);
            assert!(connected
                .bridge
                .event("occupy", json!(null))
                .expect("occupy the writer"));
            gate.wait_until_entered();

            // Three queues' worth of pane output while the peer reads nothing.
            let burst_size = 3 * WRITER_QUEUE_CAPACITY;
            let streamer = connected.bridge.clone();
            let burst = thread::spawn(move || {
                for seq in 0..burst_size {
                    streamer
                        .stream_event("pane.output", json!({"seq":seq}))
                        .expect("stream to a busy peer");
                }
            });
            thread::sleep(Duration::from_millis(50));
            send_frame(
                &mut peer,
                json!({"v":1,"id":"n-mid-burst","kind":"req","op":"unknown","body":null}),
            );
            thread::sleep(Duration::from_millis(50));
            let open_while_busy = !connected.bridge.is_closed();

            gate.release();
            burst.join().expect("burst thread");
            let expected_lines = burst_size + 2;
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut written = gate.bytes();
            while written.iter().filter(|&&byte| byte == b'\n').count() < expected_lines
                && Instant::now() < deadline
            {
                thread::sleep(Duration::from_millis(5));
                written = gate.bytes();
            }
            let frames = written
                .split(|&byte| byte == b'\n')
                .filter(|line| !line.is_empty())
                .map(|line| serde_json::from_slice::<Value>(line).expect("parse a written frame"))
                .collect::<Vec<_>>();
            let streamed = frames
                .iter()
                .filter(|frame| frame["op"] == "pane.output")
                .map(|frame| frame["body"]["seq"].as_u64().expect("seq"))
                .collect::<Vec<_>>();

            assert!(open_while_busy, "a busy peer closed the bridge");
            assert!(!connected.bridge.is_closed());
            assert_eq!(frames.len(), expected_lines);
            assert!(frames
                .iter()
                .any(|frame| frame["kind"] == "res" && frame["id"] == "n-mid-burst"));
            assert_eq!(streamed, (0..burst_size as u64).collect::<Vec<_>>());
        }

        #[test]
        fn eof_interrupts_blocked_real_output_and_releases_both_endpoints() {
            let max_frame_bytes = 9 * 1024 * 1024;
            let (connected, peer_input, peer_output, input_probe, mut output_probe) =
                connect_tracked_split(BridgeBuilder::new(Role::Host, max_frame_bytes));
            occupy_real_output(&connected.bridge, &mut output_probe);

            drop(peer_input);
            connected
                .bridge
                .wait_closed()
                .expect("wait for EOF bridge closure");
            let input_released_before_peer_cleanup = input_probe.is_dropped();
            let output_released_before_peer_cleanup = output_probe.is_dropped();

            drop(peer_output);
            assert!(
                input_released_before_peer_cleanup && output_released_before_peer_cleanup,
                "wait_closed returned before EOF terminated both transport workers"
            );
        }

        /// The peer ends the bridge's input, as a parent tells a child it is done: the
        /// bridge is closed to it as ever, and a bridge that keeps its output can still say
        /// what its owner did as it stopped, until the owner ends the output.
        #[test]
        fn a_bridge_that_keeps_its_output_says_what_is_left_after_its_input_ends_and_ends_it_on_request(
        ) {
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            builder.keep_output_after_eof();
            let (connected, mut peer) = connect_split(builder);
            peer.writer
                .shutdown(std::net::Shutdown::Write)
                .expect("end the bridge's input");
            connected
                .bridge
                .wait_launches_closed()
                .expect("the input ends");
            assert!(connected.bridge.is_closed());

            // Nothing is asked of the peer any more; what the owner tells it still goes.
            assert!(matches!(
                connected.bridge.request("late", json!(null), Some(50)),
                Err(BridgeError::Eof)
            ));
            assert!(connected
                .bridge
                .event("pane.exit", json!({"id":"p1"}))
                .expect("an event after the input ended"));
            let said = peer.receive();
            assert_eq!(
                (said["kind"].as_str(), said["op"].as_str(), &said["body"]),
                (Some("evt"), Some("pane.exit"), &json!({"id":"p1"}))
            );

            // Ended on request, after what was queued: the peer reads to its end.
            assert!(connected
                .bridge
                .event("pane.exit", json!({"id":"p2"}))
                .expect("a second event"));
            assert!(connected.bridge.end_output(Duration::from_secs(2)));
            assert_eq!(peer.receive()["body"], json!({"id":"p2"}));
            let mut rest = String::new();
            assert_eq!(
                peer.reader
                    .read_line(&mut rest)
                    .expect("the output ends cleanly"),
                0,
                "{rest}"
            );
            assert!(closes_within(&connected.bridge, Duration::from_secs(5)));
            assert!(!connected
                .bridge
                .event("pane.exit", json!({"id":"p3"}))
                .expect("an event after the output ended"));
        }

        /// The ordinary end: the output goes with the input, and what is said after is dropped.
        #[test]
        fn a_bridge_that_does_not_keep_its_output_ends_it_with_its_input() {
            let (connected, mut peer) = connect_split(BridgeBuilder::new(Role::Host, 1024));
            peer.set_read_timeout(Duration::from_secs(5));
            peer.writer
                .shutdown(std::net::Shutdown::Write)
                .expect("end the bridge's input");
            assert!(closes_within(&connected.bridge, Duration::from_secs(5)));
            assert!(!connected
                .bridge
                .event("pane.exit", json!({"id":"p1"}))
                .expect("an event after the input ended"));
            let mut rest = String::new();
            assert_eq!(
                peer.reader.read_line(&mut rest).expect("the output ends"),
                0,
                "{rest}"
            );
        }

        /// A peer that reads nothing is not waited for: the output is cut off after the
        /// time it was given, and the blocked write is let go of.
        #[test]
        fn ending_the_output_of_a_peer_that_reads_nothing_gives_up_after_its_time() {
            let mut builder = BridgeBuilder::new(Role::Host, 9 * 1024 * 1024);
            builder.keep_output_after_eof();
            let (connected, peer_input, peer_output, _input_probe, mut output_probe) =
                connect_tracked_split(builder);
            occupy_real_output(&connected.bridge, &mut output_probe);
            drop(peer_input);
            connected
                .bridge
                .wait_launches_closed()
                .expect("the input ends");

            let began = Instant::now();
            assert!(!connected.bridge.end_output(Duration::from_millis(300)));
            assert!(
                began.elapsed() < Duration::from_secs(5),
                "the output was waited for: {:?}",
                began.elapsed()
            );
            assert!(
                closes_within(&connected.bridge, Duration::from_secs(5)),
                "the blocked write was not let go of"
            );
            drop(peer_output);
        }

        /// The peer's end of the output breaks after the input has ended: the output ends
        /// with the failure, as it would have with the input, and the bridge closes.
        #[test]
        fn a_failed_write_ends_an_output_the_bridge_kept() {
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            builder.keep_output_after_eof();
            let (connected, peer_input, peer_output, _input_probe, _output_probe) =
                connect_tracked_split(builder);
            drop(peer_input);
            connected
                .bridge
                .wait_launches_closed()
                .expect("the input ends");
            drop(peer_output);

            let _ = connected.bridge.event("pane.exit", json!({"id":"p1"}));
            assert!(
                closes_within(&connected.bridge, Duration::from_secs(5)),
                "the failed output did not close the bridge"
            );
            // The output is ended, and nothing more is said to a queue that is gone.
            assert!(!connected
                .bridge
                .event("pane.exit", json!({"id":"p2"}))
                .expect("an event after the output failed"));
        }

        #[test]
        fn wait_closed_returns_only_after_the_reader_endpoint_is_destroyed() {
            let (connected, peer_input, peer_output, input_probe, drop_gate) =
                connect_with_gated_reader_drop();

            drop(peer_input);
            drop_gate.wait_until_drop_started();
            let releaser = drop_gate.release_after(Duration::from_millis(200));
            connected
                .bridge
                .wait_closed()
                .expect("wait for gated reader closure");
            let reader_was_released_before_wait_returned = input_probe.is_dropped();

            releaser.join().expect("reader-drop releaser");
            if !reader_was_released_before_wait_returned {
                let _ = input_probe.wait_for_drop(Duration::from_secs(1));
            }
            drop(peer_output);
            assert!(
                reader_was_released_before_wait_returned,
                "wait_closed returned while the reader endpoint destructor was still gated"
            );
        }

        #[test]
        fn eof_rejects_a_request_while_its_output_write_is_blocked() {
            let (writer, gate) = gated_writer();
            let (connected, peer) =
                connect_with_writer(BridgeBuilder::new(Role::Host, 1024), writer);
            let requester = connected.bridge.clone();
            let (result_sender, result_receiver) = std::sync::mpsc::channel();
            let request = thread::spawn(move || {
                let _ = result_sender.send(requester.request("blocked", json!(null), None));
            });
            gate.wait_until_entered();

            drop(peer);
            let rejected_while_output_blocked =
                result_receiver.recv_timeout(Duration::from_millis(150));
            let rejected_on_time = rejected_while_output_blocked.is_ok();
            gate.release();
            let result = rejected_while_output_blocked
                .or_else(|_| result_receiver.recv_timeout(Duration::from_secs(1)))
                .expect("request eventually rejects");
            request.join().expect("request thread");

            assert!(matches!(result, Err(BridgeError::Eof)));
            assert!(
                rejected_on_time,
                "EOF rejection waited for the blocked output writer"
            );
        }

        #[test]
        fn outgoing_oversized_frames_are_refused_without_crossing_the_wire() {
            let mut builder = BridgeBuilder::new(Role::Host, 160);
            builder.on("echo", |_bridge, body| Ok(body));
            let (connected, mut peer) = connect(builder, None);
            peer.set_read_timeout(Duration::from_millis(30));
            let large = "x".repeat(500);

            assert_eq!(
                connected
                    .bridge
                    .request("echo", json!({"large":large}), Some(50))
                    .expect("oversized request result"),
                json!({"ok":false,"error":"too-large"})
            );
            assert!(!connected
                .bridge
                .event("echo", json!({"large":"x".repeat(500)}))
                .expect("oversized event result"));
            let mut absent = String::new();
            assert!(peer.reader.read_line(&mut absent).is_err());
        }

        #[test]
        fn fragmented_valid_oversized_request_is_discarded_and_reader_recovers() {
            let handled = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut builder = BridgeBuilder::new(Role::Host, 128);
            let handled_by_handler = Arc::clone(&handled);
            builder.on("large", move |_bridge, _body| {
                handled_by_handler.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(json!({"unexpected":true}))
            });
            builder.on("ping", |_bridge, _body| Ok(json!({"ok":true})));
            let (_connected, chunks, mut output) = connect_fragmented(builder);
            output
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(1)))
                .expect("set bridge output timeout");

            send_fragmented(
                &chunks,
                json!({
                    "v":1,
                    "id":"n-large-fragmented",
                    "kind":"req",
                    "op":"large",
                    "body":{"payload":"x".repeat(500)},
                }),
                17,
            );
            output
                .get_ref()
                .set_read_timeout(Some(Duration::from_millis(50)))
                .expect("set no-response timeout");
            let mut line = String::new();
            assert!(
                output.read_line(&mut line).is_err(),
                "discarded oversized request received a response"
            );
            assert_eq!(handled.load(std::sync::atomic::Ordering::SeqCst), 0);

            output
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(1)))
                .expect("restore bridge output timeout");
            send_fragmented(
                &chunks,
                json!({"v":1,"id":"n-ping","kind":"req","op":"ping","body":null}),
                11,
            );
            line.clear();
            output
                .read_line(&mut line)
                .expect("read response after overflow");
            assert_eq!(
                serde_json::from_str::<Value>(line.trim_end()).expect("parse ping response"),
                json!({
                    "v":1,
                    "id":"n-ping",
                    "kind":"res",
                    "op":"ping",
                    "body":{"ok":true},
                })
            );
        }

        #[test]
        fn fragmented_oversized_response_is_discarded_then_defaults_to_deadline() {
            assert_eq!(DEFAULT_REQUEST_DEADLINE_MS, 30_000);
            let mut builder =
                BridgeBuilder::new(Role::Host, 128).with_default_request_deadline_ms(25);
            builder.on("ping", |_bridge, _body| Ok(json!({"ok":true})));
            let (connected, chunks, mut output) = connect_fragmented(builder);
            output
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(1)))
                .expect("set bridge output timeout");
            let requester = connected.bridge.clone();
            let (result_sender, result_receiver) = std::sync::mpsc::channel();
            let started = Instant::now();
            let request = thread::spawn(move || {
                let _ = result_sender.send(requester.request("consult", json!(null), None));
            });
            let mut outgoing = String::new();
            output
                .read_line(&mut outgoing)
                .expect("read outgoing request");
            let outgoing: Value =
                serde_json::from_str(outgoing.trim_end()).expect("parse outgoing request");

            send_fragmented(
                &chunks,
                json!({
                    "v":1,
                    "id":outgoing["id"],
                    "kind":"res",
                    "op":"consult",
                    "body":{"payload":"x".repeat(500)},
                }),
                13,
            );
            let result = result_receiver
                .recv_timeout(Duration::from_millis(250))
                .expect("discarded oversized response reaches default deadline");
            request.join().expect("request thread");

            assert_eq!(
                result.expect("default deadline result"),
                json!({"ok":false,"error":"deadline"})
            );
            assert!(started.elapsed() >= Duration::from_millis(25));
            assert!(!connected.bridge.is_closed());

            send_fragmented(
                &chunks,
                json!({"v":1,"id":"n-after-large","kind":"req","op":"ping","body":null}),
                9,
            );
            let mut line = String::new();
            output
                .read_line(&mut line)
                .expect("read response after discarded response");
            assert_eq!(
                serde_json::from_str::<Value>(line.trim_end()).expect("parse ping response"),
                json!({
                    "v":1,
                    "id":"n-after-large",
                    "kind":"res",
                    "op":"ping",
                    "body":{"ok":true},
                })
            );
        }

        #[test]
        fn oversized_handler_result_gets_a_bounded_reply_or_fails_the_transport() {
            let mut builder = BridgeBuilder::new(Role::Host, 160);
            builder.on("large", |_bridge, _body| {
                Ok(json!({"payload":"x".repeat(500)}))
            });
            let (_connected, mut peer) = connect(builder, None);
            peer.set_read_timeout(Duration::from_millis(150));
            peer.send(json!({
                "v":1,
                "id":"n-large-result",
                "kind":"req",
                "op":"large",
                "body":null
            }));
            assert_eq!(
                peer.receive(),
                json!({
                    "v":1,
                    "id":"n-large-result",
                    "kind":"res",
                    "op":"large",
                    "body":{"ok":false,"error":"too-large"}
                })
            );

            let mut builder = BridgeBuilder::new(Role::Host, 64);
            builder.on("x", |_bridge, _body| Ok(json!({"payload":"x".repeat(100)})));
            let (connected, mut peer) = connect(builder, None);
            peer.send(json!({"v":1,"id":"n-","kind":"req","op":"x","body":null}));
            let deadline = std::time::Instant::now() + Duration::from_secs(1);
            while !connected.bridge.is_closed() && std::time::Instant::now() < deadline {
                thread::yield_now();
            }
            let closed_before_eof = connected.bridge.is_closed();
            drop(peer);
            connected
                .bridge
                .wait_closed()
                .expect("wait for fallback failure close");
            assert!(
                closed_before_eof,
                "an unrepresentable too-large fallback left the bridge open"
            );
        }

        #[test]
        fn unrepresentable_fallback_closes_the_output_seen_by_the_live_peer() {
            let mut builder = BridgeBuilder::new(Role::Host, 64);
            builder.on("x", |_bridge, _body| Ok(json!({"payload":"x".repeat(100)})));
            let (connected, mut peer) = connect_split(builder);
            peer.set_read_timeout(Duration::from_millis(250));
            peer.send(json!({"v":1,"id":"n-","kind":"req","op":"x","body":null}));

            let deadline = Instant::now() + Duration::from_secs(1);
            while !connected.bridge.is_closed() && Instant::now() < deadline {
                thread::yield_now();
            }
            assert!(
                connected.bridge.is_closed(),
                "fallback did not close bridge"
            );

            let mut line = String::new();
            let read = peer.reader.read_line(&mut line);
            assert_eq!(read.expect("peer observes bridge output closure"), 0);
        }

        #[test]
        fn blocked_reader_does_not_keep_a_failed_transport_alive() {
            let mut builder = BridgeBuilder::new(Role::Host, 64);
            builder.on("x", |_bridge, _body| Ok(json!({"payload":"x".repeat(100)})));
            let (connected, mut peer) = connect_split(builder);
            let bridge_lifetime = Arc::downgrade(&connected.bridge.inner);
            peer.set_read_timeout(Duration::from_millis(250));
            peer.send(json!({"v":1,"id":"n-","kind":"req","op":"x","body":null}));

            let mut line = String::new();
            assert_eq!(
                peer.reader
                    .read_line(&mut line)
                    .expect("peer observes failed output closure"),
                0
            );
            drop(connected);
            let deadline = Instant::now() + Duration::from_millis(250);
            while bridge_lifetime.upgrade().is_some() && Instant::now() < deadline {
                thread::yield_now();
            }
            assert!(
                bridge_lifetime.upgrade().is_none(),
                "blocked reader retained the failed bridge"
            );
        }

        #[test]
        fn partial_output_failure_closes_and_rejects_all_pending_requests() {
            let (writer, gate) = partial_fail_writer();
            let (connected, peer) =
                connect_with_writer(BridgeBuilder::new(Role::Host, 1024), writer);
            let first_bridge = connected.bridge.clone();
            let (first_sender, first_receiver) = std::sync::mpsc::channel();
            let first = thread::spawn(move || {
                let _ = first_sender.send(first_bridge.request("one", json!(null), None));
            });
            gate.wait_until_partial_write();
            let second_bridge = connected.bridge.clone();
            let (second_sender, second_receiver) = std::sync::mpsc::channel();
            let second = thread::spawn(move || {
                let _ = second_sender.send(second_bridge.request("two", json!(null), None));
            });
            thread::sleep(Duration::from_millis(25));

            gate.release_failure();
            let first_result = first_receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("first request rejected");
            let second_result = second_receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("second request rejected");
            let closed_before_eof = connected.bridge.is_closed();
            drop(peer);
            connected
                .bridge
                .wait_closed()
                .expect("wait for write failure");
            first.join().expect("first request thread");
            second.join().expect("second request thread");

            assert!(matches!(first_result, Err(BridgeError::Io(_))));
            assert!(matches!(second_result, Err(BridgeError::Io(_))));
            assert!(closed_before_eof, "partial output failure left bridge open");
            assert_eq!(gate.bytes(), b"{\"v\":1,");
        }

        /// Whoever started the peer hears that the transport closed, once,
        /// whether the peer ended it or a write to the peer failed.
        #[test]
        fn the_close_is_told_once_whatever_closed_it() {
            let (closes, closed) = std::sync::mpsc::channel();
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            builder.on_close(move || {
                let _ = closes.send(());
            });
            let (connected, peer) = connect(builder, None);
            drop(peer);
            closed
                .recv_timeout(Duration::from_secs(1))
                .expect("the peer's end is told");
            connected.bridge.wait_closed().expect("bridge closes");

            let (closes, closed) = std::sync::mpsc::channel();
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            builder.on_close(move || {
                let _ = closes.send(());
            });
            let (writer, gate) = partial_fail_writer();
            let (connected, peer) = connect_with_writer(builder, writer);
            let failing = connected.bridge.clone();
            let request = thread::spawn(move || failing.request("op", json!(null), None));
            gate.wait_until_partial_write();
            gate.release_failure();
            assert!(request.join().expect("request thread").is_err());
            closed
                .recv_timeout(Duration::from_secs(1))
                .expect("a failed write is told");
            drop(peer);
            connected.bridge.wait_closed().expect("bridge closes");
            assert!(
                closed.recv_timeout(Duration::from_millis(100)).is_err(),
                "a close is told once"
            );
        }

        #[test]
        fn eof_rejects_every_outstanding_and_future_request() {
            let (connected, mut peer) = connect(BridgeBuilder::new(Role::Host, 1024), None);
            let first_bridge = connected.bridge.clone();
            let first = thread::spawn(move || first_bridge.request("one", json!(null), None));
            let second_bridge = connected.bridge.clone();
            let second = thread::spawn(move || second_bridge.request("two", json!(null), None));
            let _ = peer.receive();
            let _ = peer.receive();

            drop(peer);

            assert!(matches!(
                first.join().expect("first thread"),
                Err(BridgeError::Eof)
            ));
            assert!(matches!(
                second.join().expect("second thread"),
                Err(BridgeError::Eof)
            ));
            assert!(matches!(
                connected.bridge.request("future", json!(null), None),
                Err(BridgeError::Eof)
            ));
        }

        #[test]
        fn eof_requires_delimiters_at_each_handle_and_frame_boundary() {
            let (rust_input, mut peer_input) =
                UnixStream::pair().expect("create partial handle socket");
            peer_input
                .write_all(br#"{"v":1,"kind":"partial"}"#)
                .expect("write unterminated handle");
            drop(peer_input);
            assert!(matches!(
                BridgeBuilder::new(Role::Host, 1024)
                    .connect_uninterruptible(rust_input, Vec::<u8>::new()),
                Err(BridgeError::InvalidHandle(_))
            ));

            let (connected, peer) = connect(BridgeBuilder::new(Role::Host, 1024), None);
            drop(peer);
            connected
                .bridge
                .wait_closed()
                .expect("EOF immediately after handle");

            let dispatched = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            let handler_dispatched = Arc::clone(&dispatched);
            builder.on("ping", move |_bridge, _body| {
                handler_dispatched.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(json!({"ok":true}))
            });
            let (connected, mut peer) = connect(builder, None);
            peer.writer
                .write_all(br#"{"v":1,"id":"n-partial","kind":"req","op":"ping","body":null}"#)
                .expect("write unterminated frame");
            peer.writer.flush().expect("flush unterminated frame");
            drop(peer);
            connected
                .bridge
                .wait_closed()
                .expect("EOF after partial frame");
            assert_eq!(dispatched.load(std::sync::atomic::Ordering::SeqCst), 0);

            let (dispatched_sender, dispatched_receiver) = std::sync::mpsc::channel();
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            builder.on("ping", move |_bridge, _body| {
                dispatched_sender.send(()).expect("record dispatched frame");
                Ok(json!({"ok":true}))
            });
            let (connected, mut peer) = connect(builder, None);
            peer.send(json!({
                "v":1,
                "id":"n-complete",
                "kind":"req",
                "op":"ping",
                "body":null
            }));
            drop(peer);
            dispatched_receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("delimited frame dispatched before EOF");
            connected
                .bridge
                .wait_closed()
                .expect("EOF after complete frame");
        }

        #[test]
        fn eof_waits_for_an_admitted_open_before_the_final_reap_snapshot() {
            let panes = Arc::new(Mutex::new(Vec::new()));
            let release_launch = Arc::new(Barrier::new(2));
            let (admitted_sender, admitted_receiver) = std::sync::mpsc::channel();
            let (finished_sender, finished_receiver) = std::sync::mpsc::channel();
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            let handler_panes = Arc::clone(&panes);
            let handler_release = Arc::clone(&release_launch);
            builder.on_launch("pane.open", move |_bridge, _body| {
                admitted_sender.send(()).expect("announce launch admission");
                handler_release.wait();
                handler_panes.lock().expect("pane list lock").push("child");
                finished_sender.send(()).expect("announce completed launch");
                Ok(json!({"ok":true}))
            });
            let (connected, mut peer) = connect(builder, None);
            peer.send(json!({
                "v":1,
                "id":"n-open",
                "kind":"req",
                "op":"pane.open",
                "body":{}
            }));
            admitted_receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("handler admitted before EOF");

            drop(peer);
            let reaper_bridge = connected.bridge.clone();
            let reaper_panes = Arc::clone(&panes);
            let (reaped_sender, reaped_receiver) = std::sync::mpsc::channel();
            thread::spawn(move || {
                reaper_bridge
                    .wait_launches_closed()
                    .expect("wait for launch admission to close");
                reaper_panes.lock().expect("pane list lock").clear();
                reaped_sender.send(()).expect("announce final reap");
            });
            let reaped_before_launch_finished = reaped_receiver
                .recv_timeout(Duration::from_millis(100))
                .is_ok();

            release_launch.wait();
            finished_receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("launch handler finishes");
            if !reaped_before_launch_finished {
                reaped_receiver
                    .recv_timeout(Duration::from_secs(1))
                    .expect("final reap after handler");
            }

            assert!(
                !reaped_before_launch_finished,
                "EOF released the final reap before the admitted launch finished"
            );
            assert!(panes.lock().expect("pane list lock").is_empty());
        }

        #[test]
        fn nested_reverse_request_does_not_block_dispatch() {
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            builder.on("consult", |bridge, body| {
                let opened = bridge
                    .request("pane.open", body, Some(500))
                    .map_err(|error| error.to_string())?;
                Ok(json!({"opened":opened}))
            });
            let (connected, mut peer) = connect(builder, None);

            peer.send(json!({
                "v":1,
                "id":"n-consult",
                "kind":"req",
                "op":"consult",
                "body":{"pane":"worker"}
            }));
            let nested = peer.receive();
            assert_eq!(nested["id"], "r-1");
            assert_eq!(nested["op"], "pane.open");
            peer.send(json!({
                "v":1,
                "id":"r-1",
                "kind":"res",
                "op":"pane.open",
                "body":{"ok":true,"id":"worker"}
            }));

            assert_eq!(
                peer.receive(),
                json!({
                    "v":1,
                    "id":"n-consult",
                    "kind":"res",
                    "op":"consult",
                    "body":{"opened":{"ok":true,"id":"worker"}}
                })
            );
            assert!(!connected.bridge.is_closed());
        }

        #[test]
        fn unknown_operation_gets_the_agreed_error_response() {
            let (_connected, mut peer) = connect(BridgeBuilder::new(Role::Host, 1024), None);

            peer.send(json!({
                "v":1,
                "id":"n-unknown",
                "kind":"req",
                "op":"does.not.exist",
                "body":null
            }));

            assert_eq!(
                peer.receive(),
                json!({
                    "v":1,
                    "id":"n-unknown",
                    "kind":"res",
                    "op":"does.not.exist",
                    "body":{"ok":false,"error":"unknown-op"}
                })
            );
        }

        #[test]
        fn a_host_and_a_daemon_bridge_answer_each_other_each_with_its_own_ids() {
            // Each end refuses a request whose id is not the other's prefix and a
            // response whose id is not its own: both directions answering is the
            // proof that each mints its own and accepts the other's.
            let (host_end, daemon_end) = UnixStream::pair().expect("create bridge socket pair");
            let mut daemon = BridgeBuilder::new(Role::Daemon, 1024);
            daemon.on("board.get", |_bridge, body| {
                Ok(json!({ "ok": true, "asked": body }))
            });
            let daemon = daemon
                .serve(
                    daemon_end.try_clone().expect("clone daemon end"),
                    daemon_end,
                    &json!({ "url": "http://127.0.0.1:1", "token": "t" }),
                )
                .expect("serve the daemon end");
            let mut host = BridgeBuilder::new(Role::Host, 1024);
            host.on("pane.open", |_bridge, body| {
                Ok(json!({ "ok": true, "opened": body }))
            });
            let connected = host
                .connect(host_end.try_clone().expect("clone host end"), host_end)
                .expect("connect the host end");
            assert_eq!(connected.handle["token"], "t");
            assert_eq!(
                connected
                    .bridge
                    .request("board.get", json!({ "project": 1 }), Some(5_000))
                    .unwrap(),
                json!({ "ok": true, "asked": { "project": 1 } })
            );
            assert_eq!(
                daemon
                    .request("pane.open", json!({ "id": "p1" }), Some(5_000))
                    .unwrap(),
                json!({ "ok": true, "opened": { "id": "p1" } })
            );
        }
    }

    #[cfg(windows)]
    mod windows {
        use std::io::{BufRead, BufReader, Write};
        use std::time::{Duration, Instant};

        use cf_proto::bridge::Role;
        use serde_json::{json, Value};

        use super::super::BridgeBuilder;

        /// The daemon's side of the app's pipes: its handle first, then a
        /// frame per line each way, and the bridge closes when its input ends.
        #[test]
        fn frames_cross_os_pipes_and_the_bridge_closes_at_their_end() {
            let (bridge_input, mut to_bridge) = std::io::pipe().expect("input pipe");
            let (from_bridge, bridge_output) = std::io::pipe().expect("output pipe");
            let handle = json!({"url":"http://127.0.0.1:1234","token":"secret"});
            writeln!(to_bridge, "{handle}").expect("send the handle");
            let connected = BridgeBuilder::new(Role::Host, 1024)
                .connect(bridge_input, bridge_output)
                .expect("connect over pipes");
            assert_eq!(connected.handle, handle);
            let mut lines = BufReader::new(from_bridge);

            let request =
                json!({"v":1,"id":"n-unknown","kind":"req","op":"does.not.exist","body":null});
            writeln!(to_bridge, "{request}").expect("send a frame");
            let mut line = String::new();
            lines.read_line(&mut line).expect("read a frame");
            assert_eq!(
                serde_json::from_str::<Value>(&line).expect("a JSON frame"),
                json!({"v":1,"id":"n-unknown","kind":"res","op":"does.not.exist","body":{"ok":false,"error":"unknown-op"}})
            );

            drop(to_bridge);
            let deadline = Instant::now() + Duration::from_secs(5);
            while !connected.bridge.is_closed() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                connected.bridge.is_closed(),
                "the bridge outlived its input"
            );
        }
    }
}
