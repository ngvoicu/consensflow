use std::collections::HashMap;
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const PROTOCOL_VERSION: u8 = 1;
const RUST_ID_PREFIX: &str = "r-";
const NODE_ID_PREFIX: &str = "n-";
const WRITER_QUEUE_CAPACITY: usize = 32;
const DEFAULT_REQUEST_DEADLINE_MS: u64 = 30_000;

type RequestHandler = dyn Fn(Bridge, Value) -> Result<Value, String> + Send + Sync;
type EventHandler = dyn Fn(Value) + Send + Sync;
type ErrorHandler = dyn Fn(BridgeError) + Send + Sync;

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

#[derive(Debug, Deserialize, Serialize)]
struct Frame {
    v: u8,
    id: String,
    kind: String,
    op: String,
    body: Value,
}

struct PendingRequest {
    op: String,
    sender: mpsc::Sender<Result<Value, BridgeError>>,
}

struct WriteJob {
    bytes: Vec<u8>,
    canceled: Arc<AtomicBool>,
}

#[derive(Clone)]
struct RequestHandlerEntry {
    handler: Arc<RequestHandler>,
    launches_panes: bool,
}

struct Lifecycle {
    closed: bool,
    in_flight_handlers: usize,
    in_flight_launches: usize,
    transport_workers: usize,
}

struct TransportShutdown {
    requested: AtomicBool,
    wait_lock: Mutex<()>,
    ready: Condvar,
}

struct BridgeInner {
    writer: Mutex<Option<mpsc::SyncSender<WriteJob>>>,
    pending: Mutex<HashMap<String, PendingRequest>>,
    handlers: HashMap<String, RequestHandlerEntry>,
    event_handlers: HashMap<String, Vec<Arc<EventHandler>>>,
    on_error: Option<Arc<ErrorHandler>>,
    next_id: AtomicU64,
    closed: AtomicBool,
    lifecycle: Mutex<Lifecycle>,
    closed_ready: Condvar,
    transport_shutdown: Arc<TransportShutdown>,
    max_frame_bytes: usize,
    default_request_deadline: Duration,
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
    handlers: HashMap<String, RequestHandlerEntry>,
    event_handlers: HashMap<String, Vec<Arc<EventHandler>>>,
    on_error: Option<Arc<ErrorHandler>>,
    max_frame_bytes: usize,
    default_request_deadline: Duration,
}

impl BridgeBuilder {
    pub fn new(max_frame_bytes: usize) -> Self {
        Self {
            handlers: HashMap::new(),
            event_handlers: HashMap::new(),
            on_error: None,
            max_frame_bytes,
            default_request_deadline: Duration::from_millis(DEFAULT_REQUEST_DEADLINE_MS),
        }
    }

    #[cfg(test)]
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

    #[cfg(unix)]
    pub fn connect<R, W>(self, input: R, output: W) -> Result<ConnectedBridge, BridgeError>
    where
        R: Read + Send + AsRawFd + 'static,
        W: Write + Send + AsRawFd + 'static,
    {
        set_nonblocking(input.as_raw_fd())?;
        set_nonblocking(output.as_raw_fd())?;
        let shutdown = Arc::new(TransportShutdown::new());
        self.connect_inner(
            InterruptibleReader::new(input, Arc::clone(&shutdown)),
            InterruptibleWriter::new(output, Arc::clone(&shutdown)),
            shutdown,
        )
    }

    #[cfg(not(unix))]
    pub fn connect<R, W>(self, input: R, output: W) -> Result<ConnectedBridge, BridgeError>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        self.connect_inner(input, output, Arc::new(TransportShutdown::new()))
    }

    #[cfg(test)]
    fn connect_uninterruptible<R, W>(
        self,
        input: R,
        output: W,
    ) -> Result<ConnectedBridge, BridgeError>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        self.connect_inner(input, output, Arc::new(TransportShutdown::new()))
    }

    fn connect_inner<R, W>(
        self,
        input: R,
        output: W,
        shutdown: Arc<TransportShutdown>,
    ) -> Result<ConnectedBridge, BridgeError>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        let mut reader = BufReader::new(input);
        let mut discarding = false;
        let handle_line =
            match read_bounded_line(&mut reader, self.max_frame_bytes, &mut discarding)? {
                BoundedLine::Complete(line) => line,
                BoundedLine::Eof => return Err(BridgeError::Eof),
                BoundedLine::Unterminated => {
                    return Err(BridgeError::InvalidHandle(
                        "handle line is missing its newline".to_string(),
                    ));
                }
                BoundedLine::Overflow => {
                    return Err(BridgeError::InvalidHandle(format!(
                        "handle exceeds maxFrameBytes ({})",
                        self.max_frame_bytes
                    )));
                }
            };
        let handle = serde_json::from_slice(&handle_line)
            .map_err(|error| BridgeError::InvalidHandle(error.to_string()))?;
        let bridge = self.build(output, shutdown);
        bridge.start_reader(reader);
        Ok(ConnectedBridge { handle, bridge })
    }

    #[cfg(unix)]
    pub fn serve<R, W>(self, input: R, output: W, handle: &Value) -> Result<Bridge, BridgeError>
    where
        R: Read + Send + AsRawFd + 'static,
        W: Write + Send + AsRawFd + 'static,
    {
        set_nonblocking(input.as_raw_fd())?;
        set_nonblocking(output.as_raw_fd())?;
        let shutdown = Arc::new(TransportShutdown::new());
        let input = InterruptibleReader::new(input, Arc::clone(&shutdown));
        let output = InterruptibleWriter::new(output, Arc::clone(&shutdown));
        self.serve_inner(input, output, handle, shutdown)
    }

    #[cfg(not(unix))]
    pub fn serve<R, W>(self, input: R, output: W, handle: &Value) -> Result<Bridge, BridgeError>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        self.serve_inner(input, output, handle, Arc::new(TransportShutdown::new()))
    }

    fn serve_inner<R, W>(
        self,
        input: R,
        mut output: W,
        handle: &Value,
        shutdown: Arc<TransportShutdown>,
    ) -> Result<Bridge, BridgeError>
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        serde_json::to_writer(&mut output, handle)
            .map_err(|error| BridgeError::InvalidHandle(error.to_string()))?;
        output.write_all(b"\n")?;
        output.flush()?;
        let bridge = self.build(output, shutdown);
        bridge.start_reader(BufReader::new(input));
        Ok(bridge)
    }

    fn build<W>(self, output: W, shutdown: Arc<TransportShutdown>) -> Bridge
    where
        W: Write + Send + 'static,
    {
        let (writer, jobs) = mpsc::sync_channel(WRITER_QUEUE_CAPACITY);
        let bridge = Bridge {
            inner: Arc::new(BridgeInner {
                writer: Mutex::new(Some(writer)),
                pending: Mutex::new(HashMap::new()),
                handlers: self.handlers,
                event_handlers: self.event_handlers,
                on_error: self.on_error,
                next_id: AtomicU64::new(0),
                closed: AtomicBool::new(false),
                lifecycle: Mutex::new(Lifecycle {
                    closed: false,
                    in_flight_handlers: 0,
                    in_flight_launches: 0,
                    transport_workers: 1,
                }),
                closed_ready: Condvar::new(),
                transport_shutdown: shutdown,
                max_frame_bytes: self.max_frame_bytes,
                default_request_deadline: self.default_request_deadline,
            }),
        };
        let inner = Arc::downgrade(&bridge.inner);
        let worker = TransportWorker::new(Weak::clone(&inner));
        thread::spawn(move || {
            let _worker = worker;
            writer_loop(Box::new(output), jobs, inner);
        });
        bridge
    }
}

impl Bridge {
    pub fn request(
        &self,
        op: impl Into<String>,
        body: Value,
        deadline_ms: Option<u64>,
    ) -> Result<Value, BridgeError> {
        let started = Instant::now();
        let timeout = deadline_ms
            .map(Duration::from_millis)
            .unwrap_or(self.inner.default_request_deadline);
        let deadline = started + timeout;
        if self.is_closed() {
            return Err(BridgeError::Eof);
        }
        let op = op.into();
        let id = self.next_id();
        let frame = Frame {
            v: PROTOCOL_VERSION,
            id: id.clone(),
            kind: "req".to_string(),
            op: op.clone(),
            body,
        };
        let encoded = self.encode(&frame)?;
        if encoded.len() > self.inner.max_frame_bytes {
            return Ok(too_large_body());
        }

        let (sender, receiver) = mpsc::channel();
        {
            let mut pending = self
                .inner
                .pending
                .lock()
                .map_err(|_| BridgeError::LockPoisoned)?;
            if self.is_closed() {
                return Err(BridgeError::Eof);
            }
            pending.insert(id.clone(), PendingRequest { op, sender });
        }
        let canceled = Arc::new(AtomicBool::new(false));
        let admitted = self.enqueue_encoded(encoded, Arc::clone(&canceled), Some(deadline))?;
        if !admitted {
            canceled.store(true, Ordering::Release);
            self.remove_pending(&id);
            return Ok(json!({"ok":false,"error":"deadline"}));
        }

        let received = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        match received {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                canceled.store(true, Ordering::Release);
                self.remove_pending(&id);
                Ok(json!({"ok":false,"error":"deadline"}))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(BridgeError::Eof),
        }
    }

    pub fn event(&self, op: impl Into<String>, body: Value) -> Result<bool, BridgeError> {
        if self.is_closed() {
            return Ok(false);
        }
        let frame = Frame {
            v: PROTOCOL_VERSION,
            id: self.next_id(),
            kind: "evt".to_string(),
            op: op.into(),
            body,
        };
        let encoded = self.encode(&frame)?;
        if encoded.len() > self.inner.max_frame_bytes {
            return Ok(false);
        }
        self.write_encoded(encoded)?;
        Ok(true)
    }

    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    pub fn wait_closed(&self) -> Result<(), BridgeError> {
        let mut guard = self
            .inner
            .lifecycle
            .lock()
            .map_err(|_| BridgeError::LockPoisoned)?;
        while !guard.closed || guard.in_flight_handlers != 0 || guard.transport_workers != 0 {
            guard = self
                .inner
                .closed_ready
                .wait(guard)
                .map_err(|_| BridgeError::LockPoisoned)?;
        }
        Ok(())
    }

    pub fn wait_launches_closed(&self) -> Result<(), BridgeError> {
        let mut guard = self
            .inner
            .lifecycle
            .lock()
            .map_err(|_| BridgeError::LockPoisoned)?;
        while !guard.closed || guard.in_flight_launches != 0 {
            guard = self
                .inner
                .closed_ready
                .wait(guard)
                .map_err(|_| BridgeError::LockPoisoned)?;
        }
        Ok(())
    }

    fn next_id(&self) -> String {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("{RUST_ID_PREFIX}{id}")
    }

    fn start_reader<R>(&self, reader: BufReader<R>)
    where
        R: Read + Send + 'static,
    {
        self.begin_transport_worker();
        let inner = Arc::downgrade(&self.inner);
        let max_frame_bytes = self.inner.max_frame_bytes;
        let worker = TransportWorker::new(Weak::clone(&inner));
        thread::spawn(move || {
            let _worker = worker;
            let mut reader = reader;
            let mut discarding = false;
            loop {
                let Some(current) = inner.upgrade() else {
                    return;
                };
                if current.closed.load(Ordering::Acquire) {
                    return;
                }
                drop(current);
                let outcome = read_bounded_line(&mut reader, max_frame_bytes, &mut discarding);
                let Some(current) = inner.upgrade() else {
                    return;
                };
                let bridge = Bridge { inner: current };
                match outcome {
                    Ok(BoundedLine::Eof) => {
                        bridge.close_with_eof();
                        return;
                    }
                    Ok(BoundedLine::Complete(line)) => {
                        if !line.is_empty() {
                            bridge.dispatch_line(&line);
                        }
                    }
                    Ok(BoundedLine::Overflow) => {
                        bridge.report(BridgeError::MalformedFrame(
                            "frame exceeds maxFrameBytes".to_string(),
                        ));
                    }
                    Ok(BoundedLine::Unterminated) => {
                        bridge.report(BridgeError::MalformedFrame(
                            "unterminated frame at EOF".to_string(),
                        ));
                        bridge.close_with_eof();
                        return;
                    }
                    Err(error) => {
                        bridge.report(error.into());
                        bridge.close_with_eof();
                        return;
                    }
                }
            }
        });
    }

    fn begin_transport_worker(&self) {
        let mut lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.transport_workers += 1;
    }

    fn dispatch_line(&self, raw: &[u8]) {
        if self.is_closed() {
            return;
        }
        if raw.len() > self.inner.max_frame_bytes {
            self.report(BridgeError::MalformedFrame(
                "frame exceeds maxFrameBytes".to_string(),
            ));
            return;
        }
        let frame = match serde_json::from_slice::<Frame>(raw) {
            Ok(frame) if valid_frame(&frame) => frame,
            Ok(_) => {
                self.report(BridgeError::MalformedFrame(
                    String::from_utf8_lossy(raw).into_owned(),
                ));
                return;
            }
            Err(error) => {
                self.report(BridgeError::MalformedFrame(error.to_string()));
                return;
            }
        };

        let valid_namespace = match frame.kind.as_str() {
            "res" => frame.id.starts_with(RUST_ID_PREFIX),
            "req" | "evt" => frame.id.starts_with(NODE_ID_PREFIX),
            _ => false,
        };
        if !valid_namespace {
            self.report(BridgeError::MalformedFrame(format!(
                "id {} has the wrong namespace for {}",
                frame.id, frame.kind
            )));
            return;
        }

        match frame.kind.as_str() {
            "req" => self.dispatch_request(frame),
            "res" => self.dispatch_response(frame),
            "evt" => self.dispatch_event(frame),
            _ => unreachable!("validated frame kind"),
        }
    }

    fn dispatch_request(&self, frame: Frame) {
        let Some(entry) = self.inner.handlers.get(&frame.op).cloned() else {
            self.respond_or_report(frame.id, frame.op, unknown_op_body());
            return;
        };
        let Some(admission) = self.admit_handler(entry.launches_panes) else {
            return;
        };
        let handler = entry.handler;
        let bridge = self.clone();
        thread::spawn(move || {
            let op = frame.op;
            let body = match handler(bridge.clone(), frame.body) {
                Ok(body) => body,
                Err(error) => json!({"ok":false,"error":error}),
            };
            drop(admission);
            bridge.respond_or_report(frame.id, op, body);
        });
    }

    fn admit_handler(&self, launches_panes: bool) -> Option<HandlerAdmission> {
        let mut lifecycle = match self.inner.lifecycle.lock() {
            Ok(lifecycle) => lifecycle,
            Err(_) => {
                self.report(BridgeError::LockPoisoned);
                return None;
            }
        };
        if lifecycle.closed {
            return None;
        }
        lifecycle.in_flight_handlers += 1;
        if launches_panes {
            lifecycle.in_flight_launches += 1;
        }
        Some(HandlerAdmission {
            bridge: self.clone(),
            launches_panes,
        })
    }

    fn finish_handler(&self, launches_panes: bool) {
        let mut lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.in_flight_handlers = lifecycle.in_flight_handlers.saturating_sub(1);
        if launches_panes {
            lifecycle.in_flight_launches = lifecycle.in_flight_launches.saturating_sub(1);
        }
        if lifecycle.closed {
            self.inner.closed_ready.notify_all();
        }
    }

    fn dispatch_response(&self, frame: Frame) {
        let pending = match self.inner.pending.lock() {
            Ok(mut pending) => match pending.get(&frame.id) {
                Some(request) if request.op != frame.op => {
                    let expected_op = request.op.clone();
                    drop(pending);
                    self.report(BridgeError::MalformedFrame(format!(
                        "response {} op {} does not match pending op {expected_op}",
                        frame.id, frame.op
                    )));
                    return;
                }
                Some(_) => pending.remove(&frame.id),
                None => None,
            },
            Err(_) => {
                self.report(BridgeError::LockPoisoned);
                return;
            }
        };
        if let Some(pending) = pending {
            let _ = pending.sender.send(Ok(frame.body));
        }
    }

    fn dispatch_event(&self, frame: Frame) {
        let Some(handlers) = self.inner.event_handlers.get(&frame.op) else {
            return;
        };
        for handler in handlers {
            let handler = Arc::clone(handler);
            let body = frame.body.clone();
            thread::spawn(move || handler(body));
        }
    }

    fn respond(&self, id: String, op: String, body: Value) -> Result<(), BridgeError> {
        let frame = Frame {
            v: PROTOCOL_VERSION,
            id,
            kind: "res".to_string(),
            op,
            body,
        };
        let mut encoded = self.encode(&frame)?;
        if encoded.len() > self.inner.max_frame_bytes {
            encoded = self.encode(&Frame {
                v: PROTOCOL_VERSION,
                id: frame.id,
                kind: frame.kind,
                op: frame.op,
                body: too_large_body(),
            })?;
            if encoded.len() > self.inner.max_frame_bytes {
                let error = BridgeError::MalformedFrame(
                    "outgoing too-large response cannot fit maxFrameBytes".to_string(),
                );
                self.fail_transport(error.clone());
                return Err(error);
            }
        }
        self.write_encoded(encoded)
    }

    fn respond_or_report(&self, id: String, op: String, body: Value) {
        if let Err(error) = self.respond(id, op, body) {
            if !self.is_closed() {
                self.report(error);
            }
        }
    }

    fn encode(&self, frame: &Frame) -> Result<Vec<u8>, BridgeError> {
        serde_json::to_vec(frame).map_err(|error| BridgeError::MalformedFrame(error.to_string()))
    }

    fn write_encoded(&self, encoded: Vec<u8>) -> Result<(), BridgeError> {
        if self.enqueue_encoded(
            encoded,
            Arc::new(AtomicBool::new(false)),
            Some(Instant::now()),
        )? {
            return Ok(());
        }
        let error = BridgeError::Io("bridge writer queue is full".to_string());
        self.fail_transport(error.clone());
        Err(error)
    }

    fn enqueue_encoded(
        &self,
        mut encoded: Vec<u8>,
        canceled: Arc<AtomicBool>,
        deadline: Option<Instant>,
    ) -> Result<bool, BridgeError> {
        encoded.push(b'\n');
        let mut job = WriteJob {
            bytes: encoded,
            canceled,
        };
        loop {
            if self.is_closed() {
                return Err(BridgeError::Eof);
            }
            let send_result = {
                let writer = self
                    .inner
                    .writer
                    .lock()
                    .map_err(|_| BridgeError::LockPoisoned)?;
                let Some(writer) = writer.as_ref() else {
                    return Err(BridgeError::Eof);
                };
                writer.try_send(job)
            };
            match send_result {
                Ok(()) => return Ok(true),
                Err(mpsc::TrySendError::Full(returned)) => {
                    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        return Ok(false);
                    }
                    job = returned;
                    thread::sleep(Duration::from_millis(1));
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    let error = BridgeError::Io("bridge writer queue is closed".to_string());
                    self.fail_transport(error.clone());
                    return Err(error);
                }
            }
        }
    }

    fn remove_pending(&self, id: &str) -> Option<PendingRequest> {
        match self.inner.pending.lock() {
            Ok(mut pending) => pending.remove(id),
            Err(_) => {
                self.report(BridgeError::LockPoisoned);
                None
            }
        }
    }

    fn close_with_eof(&self) {
        self.close_transport(BridgeError::Eof, false);
    }

    fn fail_transport(&self, error: BridgeError) {
        self.close_transport(error, true);
    }

    fn close_transport(&self, error: BridgeError, report: bool) {
        let mut lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if lifecycle.closed {
            return;
        }
        lifecycle.closed = true;
        self.inner.closed.store(true, Ordering::Release);
        self.inner.transport_shutdown.request();
        match self.inner.writer.lock() {
            Ok(mut writer) => drop(writer.take()),
            Err(poisoned) => drop(poisoned.into_inner().take()),
        }
        let pending = match self.inner.pending.lock() {
            Ok(mut pending) => pending
                .drain()
                .map(|(_, request)| request)
                .collect::<Vec<_>>(),
            Err(poisoned) => poisoned
                .into_inner()
                .drain()
                .map(|(_, request)| request)
                .collect(),
        };
        for request in pending {
            let _ = request.sender.send(Err(error.clone()));
        }
        self.inner.closed_ready.notify_all();
        drop(lifecycle);
        if report {
            self.report(error);
        }
    }

    fn report(&self, error: BridgeError) {
        if let Some(handler) = &self.inner.on_error {
            handler(error);
        }
    }
}

impl Drop for BridgeInner {
    fn drop(&mut self) {
        self.transport_shutdown.request();
    }
}

struct HandlerAdmission {
    bridge: Bridge,
    launches_panes: bool,
}

struct TransportWorker {
    inner: Weak<BridgeInner>,
}

impl TransportWorker {
    fn new(inner: Weak<BridgeInner>) -> Self {
        Self { inner }
    }
}

impl Drop for TransportWorker {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let mut lifecycle = inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.transport_workers = lifecycle.transport_workers.saturating_sub(1);
        if lifecycle.closed {
            inner.closed_ready.notify_all();
        }
    }
}

impl Drop for HandlerAdmission {
    fn drop(&mut self) {
        self.bridge.finish_handler(self.launches_panes);
    }
}

fn writer_loop(
    mut writer: Box<dyn Write + Send>,
    jobs: mpsc::Receiver<WriteJob>,
    weak_inner: Weak<BridgeInner>,
) {
    while let Ok(job) = jobs.recv() {
        if job.canceled.load(Ordering::Acquire) {
            continue;
        }
        let Some(inner) = weak_inner.upgrade() else {
            return;
        };
        if inner.closed.load(Ordering::Acquire) {
            return;
        }
        drop(inner);
        if let Err(error) = writer.write_all(&job.bytes).and_then(|()| writer.flush()) {
            if let Some(inner) = weak_inner.upgrade() {
                Bridge { inner }.fail_transport(error.into());
            }
            return;
        }
    }
}

impl TransportShutdown {
    fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            wait_lock: Mutex::new(()),
            ready: Condvar::new(),
        }
    }

    fn request(&self) {
        self.requested.store(true, Ordering::Release);
        self.ready.notify_all();
    }

    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    fn wait_for_retry(&self) {
        let guard = self
            .wait_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !self.is_requested() {
            let _ = self.ready.wait_timeout(guard, Duration::from_millis(10));
        }
    }
}

#[cfg(unix)]
struct InterruptibleReader<R> {
    inner: R,
    shutdown: Arc<TransportShutdown>,
}

#[cfg(unix)]
impl<R> InterruptibleReader<R> {
    fn new(inner: R, shutdown: Arc<TransportShutdown>) -> Self {
        Self { inner, shutdown }
    }
}

#[cfg(unix)]
impl<R: Read> Read for InterruptibleReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.shutdown.is_requested() {
                return Ok(0);
            }
            match self.inner.read(bytes) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    self.shutdown.wait_for_retry();
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
}

#[cfg(unix)]
struct InterruptibleWriter<W> {
    inner: W,
    shutdown: Arc<TransportShutdown>,
}

#[cfg(unix)]
impl<W> InterruptibleWriter<W> {
    fn new(inner: W, shutdown: Arc<TransportShutdown>) -> Self {
        Self { inner, shutdown }
    }
}

#[cfg(unix)]
impl<W: Write> Write for InterruptibleWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        loop {
            if self.shutdown.is_requested() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "bridge transport is closed",
                ));
            }
            match self.inner.write(bytes) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    self.shutdown.wait_for_retry();
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        loop {
            if self.shutdown.is_requested() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "bridge transport is closed",
                ));
            }
            match self.inner.flush() {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    self.shutdown.wait_for_retry();
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
}

#[cfg(unix)]
fn set_nonblocking(file_descriptor: RawFd) -> std::io::Result<()> {
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const O_NONBLOCK: i32 = 0x800;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const O_NONBLOCK: i32 = 0x0004;

    unsafe extern "C" {
        fn fcntl(file_descriptor: i32, command: i32, ...) -> i32;
    }

    // SAFETY: fcntl only inspects or updates flags for the valid descriptor
    // borrowed from the caller; ownership is unchanged.
    let flags = unsafe { fcntl(file_descriptor, F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { fcntl(file_descriptor, F_SETFL, flags | O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
pub fn stdin_is_pipe() -> bool {
    use std::os::fd::FromRawFd;
    use std::os::unix::fs::FileTypeExt;

    unsafe extern "C" {
        fn dup(file_descriptor: i32) -> i32;
    }

    // SAFETY: a successful dup returns a new descriptor owned by this call;
    // File closes that duplicate without affecting the process stdin.
    let file_descriptor = unsafe { dup(0) };
    if file_descriptor < 0 {
        return false;
    }
    let input = unsafe { std::fs::File::from_raw_fd(file_descriptor) };
    input.metadata().is_ok_and(|metadata| {
        let file_type = metadata.file_type();
        file_type.is_fifo() || file_type.is_socket()
    })
}

#[cfg(not(unix))]
pub fn stdin_is_pipe() -> bool {
    false
}

fn valid_frame(frame: &Frame) -> bool {
    frame.v == PROTOCOL_VERSION
        && !frame.id.is_empty()
        && matches!(frame.kind.as_str(), "req" | "res" | "evt")
}

enum BoundedLine {
    Complete(Vec<u8>),
    Overflow,
    Unterminated,
    Eof,
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
    discarding: &mut bool,
) -> std::io::Result<BoundedLine> {
    if *discarding {
        loop {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return Ok(BoundedLine::Eof);
            }
            let newline = available.iter().position(|byte| *byte == b'\n');
            let consumed = newline.map_or(available.len(), |index| index + 1);
            reader.consume(consumed);
            if newline.is_some() {
                *discarding = false;
                break;
            }
        }
    }

    let mut line = Vec::with_capacity(max_bytes.min(8 * 1024));
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(BoundedLine::Eof)
            } else {
                Ok(BoundedLine::Unterminated)
            };
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            if line.len().saturating_add(newline) > max_bytes {
                reader.consume(newline + 1);
                return Ok(BoundedLine::Overflow);
            }
            line.extend_from_slice(&available[..newline]);
            reader.consume(newline + 1);
            trim_line_ending(&mut line);
            return Ok(BoundedLine::Complete(line));
        }

        let available_len = available.len();
        if line.len().saturating_add(available_len) > max_bytes {
            reader.consume(available_len);
            *discarding = true;
            return Ok(BoundedLine::Overflow);
        }
        line.extend_from_slice(available);
        reader.consume(available_len);
    }
}

fn trim_line_ending(line: &mut Vec<u8>) {
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
}

fn too_large_body() -> Value {
    json!({"ok":false,"error":"too-large"})
}

fn unknown_op_body() -> Value {
    json!({"ok":false,"error":"unknown-op"})
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
            let connected = BridgeBuilder::new(1024)
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
            let mut builder = BridgeBuilder::new(1024);
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
                let result = BridgeBuilder::new(32)
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
            let mut builder = BridgeBuilder::new(128);
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
            let mut builder = BridgeBuilder::new(1024);
            builder.on_event("pane.enter", move |body| {
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
                "op":"pane.enter",
                "body":{"epoch":9}
            }));
            assert_eq!(
                event_receiver
                    .recv_timeout(Duration::from_secs(1))
                    .expect("receive pane.enter"),
                json!({"epoch":9})
            );
        }

        #[test]
        fn deadline_returns_error_body_and_late_response_is_dropped() {
            let (connected, mut peer) = connect(BridgeBuilder::new(1024), None);

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
            let (connected, peer) = connect_with_writer(BridgeBuilder::new(1024), writer);
            let first_bridge = connected.bridge.clone();
            let first = thread::spawn(move || first_bridge.request("first", json!(null), None));
            gate.wait_until_entered();

            let second_bridge = connected.bridge.clone();
            let (second_sender, second_receiver) = std::sync::mpsc::channel();
            let second = thread::spawn(move || {
                let _ = second_sender.send(second_bridge.request("second", json!(null), Some(20)));
            });
            let on_time = second_receiver.recv_timeout(Duration::from_millis(150));
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
            let (connected, mut peer) = connect_with_writer(BridgeBuilder::new(256), writer);
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
        fn saturated_response_queue_closes_instead_of_accumulating_waiters() {
            let max_frame_bytes = 9 * 1024 * 1024;
            let (connected, mut peer_input, peer_output, input_probe, mut output_probe) =
                connect_tracked_split(BridgeBuilder::new(max_frame_bytes));
            occupy_real_output(&connected.bridge, &mut output_probe);

            for index in 0..=WRITER_QUEUE_CAPACITY {
                send_frame(
                    &mut peer_input,
                    json!({
                        "v":1,
                        "id":format!("n-unknown-{index}"),
                        "kind":"req",
                        "op":"unknown",
                        "body":null
                    }),
                );
            }
            let deadline = Instant::now() + Duration::from_millis(250);
            while !connected.bridge.is_closed() && Instant::now() < deadline {
                thread::yield_now();
            }
            let closed_while_output_was_blocked = connected.bridge.is_closed();
            connected
                .bridge
                .wait_closed()
                .expect("wait for queue-failed bridge closure");
            let input_released_before_peer_cleanup = input_probe.is_dropped();
            let output_released_before_peer_cleanup = output_probe.is_dropped();

            drop(peer_input);
            drop(peer_output);
            assert!(
                closed_while_output_was_blocked,
                "response admission accumulated waiters beyond its bounded queue"
            );
            assert!(
                input_released_before_peer_cleanup && output_released_before_peer_cleanup,
                "wait_closed returned before queue failure terminated both transport workers"
            );
        }

        #[test]
        fn eof_interrupts_blocked_real_output_and_releases_both_endpoints() {
            let max_frame_bytes = 9 * 1024 * 1024;
            let (connected, peer_input, peer_output, input_probe, mut output_probe) =
                connect_tracked_split(BridgeBuilder::new(max_frame_bytes));
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
            let (connected, peer) = connect_with_writer(BridgeBuilder::new(1024), writer);
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
            let mut builder = BridgeBuilder::new(160);
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
            let mut builder = BridgeBuilder::new(128);
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
            let mut builder = BridgeBuilder::new(128).with_default_request_deadline_ms(25);
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
            let mut builder = BridgeBuilder::new(160);
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

            let mut builder = BridgeBuilder::new(64);
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
            let mut builder = BridgeBuilder::new(64);
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
            let mut builder = BridgeBuilder::new(64);
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
            let (connected, peer) = connect_with_writer(BridgeBuilder::new(1024), writer);
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

        #[test]
        fn eof_rejects_every_outstanding_and_future_request() {
            let (connected, mut peer) = connect(BridgeBuilder::new(1024), None);
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
                BridgeBuilder::new(1024).connect_uninterruptible(rust_input, Vec::<u8>::new()),
                Err(BridgeError::InvalidHandle(_))
            ));

            let (connected, peer) = connect(BridgeBuilder::new(1024), None);
            drop(peer);
            connected
                .bridge
                .wait_closed()
                .expect("EOF immediately after handle");

            let dispatched = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut builder = BridgeBuilder::new(1024);
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
            let mut builder = BridgeBuilder::new(1024);
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
            let mut builder = BridgeBuilder::new(1024);
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
            let mut builder = BridgeBuilder::new(1024);
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
            let (_connected, mut peer) = connect(BridgeBuilder::new(1024), None);

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
    }

    #[cfg(windows)]
    mod windows {
        #[test]
        #[ignore = "macOS-first pipe protocol contract"]
        fn child_stdio_protocol_contract() {
            panic!("exercise with Windows child pipes");
        }
    }
}
