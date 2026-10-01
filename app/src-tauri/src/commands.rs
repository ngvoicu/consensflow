use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use futures_channel::oneshot;
use portable_pty::PtySize;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::arbiter::{ArbiterError, InputArbiter, OutputClock};
use crate::bridge::{Bridge, BridgeBuilder, BridgeError};
use crate::pty::{
    validate_drop_env, PaneEnvironment, PaneKey, PaneOutput, PaneTable, StreamedPane,
};

const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// How long the daemon gets to stop on its own before it is killed.
const EDITOR_STOP_GRACE: Duration = Duration::from_secs(2);
/// The page-side name of Node's `state.changed`. No dot: Tauri rejects it.
const PAGE_STATE_EVENT: &str = "state-changed";
/// What the page is told of the daemon (see `CoreStatus`).
const CORE_STATUS_EVENT: &str = "core-status";
/// Between starts of a daemon that failed to start.
const CORE_RESTART: Backoff = Backoff {
    first: Duration::from_secs(1),
    most: Duration::from_secs(30),
};
const CORE_STOPPED: &str = "ConsensFlow's core stopped while the app was running";
/// How long the human's login shell has to say its PATH.
const LOGIN_PATH_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_BACKLOG_BYTES: usize = 1024 * 1024;
const MAX_INPUT_BYTES: usize = 64 * 1024;
const INPUT_QUEUE_CAPACITY: usize = 1024;
const MAX_PENDING_INPUT_BYTES_PER_PANE: usize = 4 * 1024 * 1024;
const MAX_PENDING_INPUT_TICKETS: usize = 4096;
const INPUT_QUEUE_FULL: &str = "pane-input-queue-full";
const INPUT_SEQUENCE_GAP: &str = "pane-input-sequence-gap";
const INPUT_SEQUENCE_REGRESSION: &str = "pane-input-sequence-regression";
const MAX_TERMINAL_DIMENSION: u16 = 4096;
const ENTER_DELAY_MS: u64 = 10;

type PageEventSink = Arc<dyn Fn(&str, Value) + Send + Sync>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneOutputMessage {
    id: String,
    generation: u64,
    seq: u64,
    bytes: Vec<u8>,
}

impl From<PaneOutput> for PaneOutputMessage {
    fn from(output: PaneOutput) -> Self {
        Self {
            id: output.key.id,
            generation: output.key.generation,
            seq: output.seq,
            bytes: output.bytes,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RosterHandle {
    url: String,
    token: String,
}

impl RosterHandle {
    fn from_value(value: Value) -> Result<Self, String> {
        let mut handle: Self = serde_json::from_value(value)
            .map_err(|error| format!("the editor returned an invalid handle: {error}"))?;
        if !handle.url.starts_with("http://127.0.0.1:")
            && !handle.url.starts_with("http://localhost:")
        {
            return Err("the editor handle is not a loopback HTTP address".to_string());
        }
        if handle.token.is_empty() {
            return Err("the editor handle omitted its UI token".to_string());
        }
        handle.url = handle.url.replace("http://127.0.0.1:", "http://localhost:");
        Ok(handle)
    }
}

struct OutputHubState {
    sink: Option<OutputSink>,
    pending: VecDeque<PaneOutputMessage>,
}

/// Where one pane's bytes go. `false` means the destination is gone, and the
/// hub parks what follows until a new one arrives. The sink runs under the
/// hub's lock and the headless one waits while its peer is busy, so publish
/// only from a pane's own output thread, never from a bridge handler.
type OutputSink = Arc<dyn Fn(PaneOutputMessage) -> bool + Send + Sync>;

struct OutputHub {
    state: Mutex<OutputHubState>,
}

/// What a pane's worker does, in order: the human's keys and the emulator's
/// replies, a delivery's paste, a native send's claim.
enum InputWork {
    Write(Vec<u8>),
    Paste(Vec<u8>),
    Claim,
}

impl InputWork {
    fn byte_count(&self) -> usize {
        match self {
            Self::Write(bytes) => bytes.len(),
            Self::Paste(body) => body.len().saturating_add(13),
            Self::Claim => 0,
        }
    }
}

type InputResponse = Result<(), String>;

struct InputJob {
    work: InputWork,
    response: oneshot::Sender<InputResponse>,
    reserved_bytes: usize,
    pending_bytes: Arc<AtomicUsize>,
}

struct PageInputCompletion {
    receiver: oneshot::Receiver<InputResponse>,
    human: bool,
}

struct PageInputState {
    last_sequences: HashMap<PaneKey, u64>,
    completions: HashMap<String, PageInputCompletion>,
    next_ticket: u64,
}

struct InputRoute {
    sender: mpsc::SyncSender<InputJob>,
    pending_bytes: Arc<AtomicUsize>,
}

/// Every open pane's input, in order: one worker and one bounded queue per
/// pane, from its `pane.open` until it leaves the table.
struct InputQueue {
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    senders: Mutex<HashMap<PaneKey, InputRoute>>,
    workers: Mutex<HashMap<PaneKey, JoinHandle<()>>>,
    page: Mutex<PageInputState>,
    accepting: AtomicBool,
}

struct LaunchSlot {
    outcome: Mutex<Option<Result<PaneKey, String>>>,
    ready: Condvar,
}

impl LaunchSlot {
    fn new() -> Self {
        Self {
            outcome: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    fn complete(&self, outcome: Result<PaneKey, String>) {
        *self
            .outcome
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(outcome);
        self.ready.notify_all();
    }

    fn wait(&self) -> Result<PaneKey, String> {
        let mut outcome = self
            .outcome
            .lock()
            .map_err(|_| "launch result lock is poisoned".to_string())?;
        while outcome.is_none() {
            outcome = self
                .ready
                .wait(outcome)
                .map_err(|_| "launch result lock is poisoned".to_string())?;
        }
        outcome
            .as_ref()
            .expect("launch outcome checked above")
            .clone()
    }
}

enum LaunchClaim {
    Owner(Arc<LaunchSlot>),
    Duplicate(Arc<LaunchSlot>),
}

struct LaunchRegistry {
    entries: Mutex<HashMap<String, Arc<LaunchSlot>>>,
}

impl LaunchRegistry {
    fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn reserve(&self, id: &str) -> Result<LaunchClaim, String> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "launch table lock is poisoned".to_string())?;
        if let Some(slot) = entries.get(id) {
            return Ok(LaunchClaim::Duplicate(Arc::clone(slot)));
        }
        let slot = Arc::new(LaunchSlot::new());
        entries.insert(id.to_string(), Arc::clone(&slot));
        Ok(LaunchClaim::Owner(slot))
    }

    fn remove_id(&self, id: &str) {
        self.entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(id);
    }

    fn remove_key(&self, key: &PaneKey) {
        self.entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|_, slot| {
                slot.outcome
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .as_ref()
                    .and_then(|outcome| outcome.as_ref().ok())
                    != Some(key)
            });
    }
}

impl InputQueue {
    fn new(panes: Arc<PaneTable>, arbiter: Arc<InputArbiter>) -> Self {
        Self {
            panes,
            arbiter,
            senders: Mutex::new(HashMap::new()),
            workers: Mutex::new(HashMap::new()),
            page: Mutex::new(PageInputState {
                last_sequences: HashMap::new(),
                completions: HashMap::new(),
                next_ticket: 1,
            }),
            accepting: AtomicBool::new(true),
        }
    }

    fn enqueue_page(
        &self,
        key: PaneKey,
        sequence: u64,
        work: InputWork,
        human: bool,
    ) -> Result<String, String> {
        let mut page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        let expected = page
            .last_sequences
            .get(&key)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| "pane-input-sequence-overflow".to_string())?;
        if sequence < expected {
            return Err(INPUT_SEQUENCE_REGRESSION.to_string());
        }
        if sequence > expected {
            return Err(INPUT_SEQUENCE_GAP.to_string());
        }

        page.last_sequences.insert(key.clone(), sequence);
        match &work {
            InputWork::Write(bytes) | InputWork::Paste(bytes) => validate_input(bytes)?,
            InputWork::Claim => {}
        }
        if page.completions.len() >= MAX_PENDING_INPUT_TICKETS {
            return Err("pane-input-ticket-capacity".to_string());
        }
        let next_ticket = page
            .next_ticket
            .checked_add(1)
            .ok_or_else(|| "pane-input-ticket-overflow".to_string())?;
        let receiver = self.submit(key, work)?;
        let ticket = format!("pane-input-{}", page.next_ticket);
        page.next_ticket = next_ticket;
        page.completions
            .insert(ticket.clone(), PageInputCompletion { receiver, human });
        Ok(ticket)
    }

    /// A new page counts every pane's input from 1: the page that sent the
    /// old numbers is gone (a reload replaced it), and so are the tickets it
    /// never came back for. What it had admitted still goes in, in order.
    fn begin_page(&self) {
        let mut page = self.page.lock().unwrap_or_else(|error| error.into_inner());
        page.last_sequences.clear();
        page.completions.clear();
    }

    fn take_page_completion(&self, ticket: &str) -> Result<PageInputCompletion, String> {
        self.page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?
            .completions
            .remove(ticket)
            .ok_or_else(|| "pane-input-ticket-not-found".to_string())
    }

    fn write(
        &self,
        key: PaneKey,
        bytes: Vec<u8>,
    ) -> Result<oneshot::Receiver<InputResponse>, String> {
        self.submit(key, InputWork::Write(bytes))
    }

    fn paste(
        &self,
        key: PaneKey,
        body: Vec<u8>,
    ) -> Result<oneshot::Receiver<InputResponse>, String> {
        self.submit(key, InputWork::Paste(body))
    }

    /// Admit a native-channel send: the pane is current and its input works,
    /// and no paste is going in, since the pane's worker runs one job at a
    /// time.
    fn claim(&self, key: PaneKey) -> Result<oneshot::Receiver<InputResponse>, String> {
        self.submit(key, InputWork::Claim)
    }

    /// A pane just opened gets its worker and queue.
    fn open(&self, key: &PaneKey) -> Result<(), String> {
        let mut senders = self
            .senders
            .lock()
            .map_err(|_| "pane input queue lock is poisoned".to_string())?;
        if !self.accepting.load(Ordering::Acquire) {
            return Err("pane input admission is closed".to_string());
        }
        let (sender, jobs) = mpsc::sync_channel(INPUT_QUEUE_CAPACITY);
        let panes = Arc::clone(&self.panes);
        let arbiter = Arc::clone(&self.arbiter);
        let worker_key = key.clone();
        let worker = thread::Builder::new()
            .name(format!("consensflow-input-{}", worker_key.id))
            .spawn(move || input_worker(jobs, panes, arbiter, worker_key))
            .map_err(|error| format!("could not start pane input queue: {error}"))?;
        self.workers
            .lock()
            .map_err(|_| "pane input worker lock is poisoned".to_string())?
            .insert(key.clone(), worker);
        senders.insert(
            key.clone(),
            InputRoute {
                sender,
                pending_bytes: Arc::new(AtomicUsize::new(0)),
            },
        );
        Ok(())
    }

    /// A pane gone from the table, killed or retired when its program ended,
    /// takes its input with it: its queue closes (the worker ends once what
    /// was queued has gone in or failed), and its page count and arbiter
    /// state go. They used to stay until the app quit, a parked thread per
    /// window ever opened.
    fn retire(&self, key: &PaneKey) {
        self.senders
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(key);
        self.workers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(key);
        self.page
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .last_sequences
            .remove(key);
        self.arbiter.retire(key);
    }

    fn submit(
        &self,
        key: PaneKey,
        work: InputWork,
    ) -> Result<oneshot::Receiver<InputResponse>, String> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err("pane input admission is closed".to_string());
        }
        let senders = self
            .senders
            .lock()
            .map_err(|_| "pane input queue lock is poisoned".to_string())?;
        if !self.accepting.load(Ordering::Acquire) {
            return Err("pane input admission is closed".to_string());
        }
        // A pane the table does not hold has no queue: it was never opened,
        // or it is gone.
        let route = senders
            .get(&key)
            .ok_or_else(|| ArbiterError::Stale.to_string())?;
        let reserved_bytes = work.byte_count();
        route
            .pending_bytes
            .try_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                pending
                    .checked_add(reserved_bytes)
                    .filter(|next| *next <= MAX_PENDING_INPUT_BYTES_PER_PANE)
            })
            .map_err(|_| INPUT_QUEUE_FULL.to_string())?;
        let (response, receiver) = oneshot::channel();
        let job = InputJob {
            work,
            response,
            reserved_bytes,
            pending_bytes: Arc::clone(&route.pending_bytes),
        };
        match route.sender.try_send(job) {
            Ok(()) => Ok(receiver),
            Err(mpsc::TrySendError::Full(job)) => {
                job.pending_bytes
                    .fetch_sub(job.reserved_bytes, Ordering::AcqRel);
                Err(INPUT_QUEUE_FULL.to_string())
            }
            Err(mpsc::TrySendError::Disconnected(job)) => {
                job.pending_bytes
                    .fetch_sub(job.reserved_bytes, Ordering::AcqRel);
                Err(format!("pane input queue for {} is closed", key.id))
            }
        }
    }

    fn close_and_drain(&self) {
        if !self.accepting.swap(false, Ordering::AcqRel) {
            return;
        }
        self.senders
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        let workers = std::mem::take(
            &mut *self
                .workers
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        for worker in workers.into_values() {
            let _ = worker.join();
        }
    }
}

fn input_worker(
    jobs: mpsc::Receiver<InputJob>,
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    key: PaneKey,
) {
    for job in jobs {
        let result = match job.work {
            InputWork::Write(bytes) => arbiter.write(&panes, &key, &bytes),
            InputWork::Paste(body) => arbiter.write_paste(&panes, &key, &body),
            InputWork::Claim => arbiter.claim(&key),
        }
        .map_err(|error| error.to_string());
        job.pending_bytes
            .fetch_sub(job.reserved_bytes, Ordering::AcqRel);
        let _ = job.response.send(result);
    }
}

async fn wait_for_input(receiver: oneshot::Receiver<InputResponse>) -> InputResponse {
    receiver
        .await
        .map_err(|_| "pane input queue ended before answering".to_string())?
}

fn wait_for_input_blocking(receiver: oneshot::Receiver<InputResponse>) -> InputResponse {
    tauri::async_runtime::block_on(wait_for_input(receiver))
}

impl OutputHub {
    fn new() -> Self {
        Self {
            state: Mutex::new(OutputHubState {
                sink: None,
                pending: VecDeque::new(),
            }),
        }
    }

    /// The window's destination: a webview channel the page reads.
    fn register(&self, channel: Channel<PaneOutputMessage>) {
        self.attach(Arc::new(move |message| channel.send(message).is_ok()));
    }

    /// The headless destination: back over the bridge the request came in on.
    ///
    /// The hub exists so `register_pane_handlers` need not know which of the
    /// two it is feeding — that is what lets the window and the helper share
    /// one set of handlers instead of two that drift.
    fn register_sink(&self, sink: OutputSink) {
        self.attach(sink);
    }

    fn attach(&self, sink: OutputSink) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.sink = Some(sink);
        let pending = std::mem::take(&mut state.pending);
        for message in pending {
            Self::deliver_or_park(&mut state, message);
        }
    }

    fn deliver_or_park(state: &mut OutputHubState, message: PaneOutputMessage) {
        if state
            .sink
            .as_ref()
            .is_some_and(|sink| sink(message.clone()))
        {
            return;
        }
        state.sink = None;
        state.pending.push_back(message);
    }

    fn publish(&self, message: PaneOutputMessage) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        Self::deliver_or_park(&mut state, message);
    }
}

pub(crate) fn window_command_allowed(window: &str, _command: &str) -> bool {
    window == "main"
}

/// What the page is told about the daemon (its `core-status`): up, or down,
/// why, and whether the app is starting it again.
#[derive(Clone, Debug, PartialEq, Serialize)]
struct CoreStatus {
    available: bool,
    cause: Option<String>,
    retrying: bool,
}

impl CoreStatus {
    fn up() -> Self {
        Self {
            available: true,
            cause: None,
            retrying: false,
        }
    }

    fn down(cause: &str, retrying: bool) -> Self {
        Self {
            available: false,
            cause: Some(cause.to_string()),
            retrying,
        }
    }
}

/// Why a start of the daemon failed, and whether another start could fare
/// better: a runtime missing from the app does not come back.
struct CoreFailure {
    cause: String,
    retry: bool,
}

impl CoreFailure {
    fn transient(cause: String) -> Self {
        Self { cause, retry: true }
    }
}

/// A daemon that started: its process, its bridge, and the address of the
/// agents screens it handed the app.
struct StartedCore {
    editor: Child,
    bridge: Bridge,
    roster: RosterHandle,
}

impl StartedCore {
    fn stop(mut self) {
        self.bridge.close_input();
        stop_editor(&mut self.editor);
    }
}

/// Starts the daemon once. What it is given is told when that daemon's bridge
/// closes.
type CoreStarter =
    Arc<dyn Fn(Box<dyn Fn() + Send + Sync>) -> Result<StartedCore, CoreFailure> + Send + Sync>;
type CoreReport = Arc<dyn Fn(&CoreStatus) + Send + Sync>;

/// The waits between starts: the first, then twice as long each time, up to
/// the most.
#[derive(Clone, Copy)]
struct Backoff {
    first: Duration,
    most: Duration,
}

/// The daemon as the app holds it, and what the page knows of it.
///
/// A start that failed (the ledger still held by a daemon finishing its stop
/// after a force-quit, a migration that throws, a missing runtime) left the
/// app without its core for the session, the cause in a detail the page never
/// read; a daemon that died mid-session closed its bridge without a word and
/// the board froze. Now the page is told every change, and a failed start is
/// tried again: it ended before its handle line, so it left no window behind
/// it. A daemon that stops later is not started again, because its windows
/// still run in the pane host.
struct Core {
    state: Mutex<CoreState>,
    report: CoreReport,
}

struct CoreState {
    editor: Option<Child>,
    bridge: Option<Bridge>,
    roster: Option<RosterHandle>,
    status: CoreStatus,
    /// Starts so far, and the one whose bridge is in hand: only that bridge
    /// closing is the core stopping.
    starts: u64,
    current: u64,
    stopping: bool,
}

impl Core {
    fn new(report: CoreReport) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(CoreState {
                editor: None,
                bridge: None,
                roster: None,
                status: CoreStatus::down("ConsensFlow's core has not started", false),
                starts: 0,
                current: 0,
                stopping: false,
            }),
            report,
        })
    }

    fn lock(&self) -> MutexGuard<'_, CoreState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Starts the daemon. A start that fails is tried again from a thread of
    /// its own, waiting longer each time, while no pane is open and the app
    /// is not stopping.
    fn start(self: &Arc<Self>, starter: CoreStarter, panes: Arc<PaneTable>, backoff: Backoff) {
        let Err(failure) = self.attempt(&starter) else {
            return;
        };
        self.fail(&failure);
        if !failure.retry {
            return;
        }
        let core = Arc::clone(self);
        if thread::Builder::new()
            .name("consensflow-core-start".to_string())
            .spawn(move || core.retry(&starter, &panes, backoff))
            .is_err()
        {
            self.tell(CoreStatus::down(&failure.cause, false));
        }
    }

    fn retry(self: &Arc<Self>, starter: &CoreStarter, panes: &PaneTable, backoff: Backoff) {
        let mut wait = backoff.first;
        loop {
            thread::sleep(wait);
            wait = (wait * 2).min(backoff.most);
            let status = {
                let state = self.lock();
                if state.stopping {
                    return;
                }
                state.status.clone()
            };
            if !matches!(panes.list(), Ok(open) if open.is_empty()) {
                self.tell(CoreStatus {
                    retrying: false,
                    ..status
                });
                return;
            }
            match self.attempt(starter) {
                Ok(()) => return,
                Err(failure) => {
                    self.fail(&failure);
                    if !failure.retry {
                        return;
                    }
                }
            }
        }
    }

    /// One start. What it starts is the core from then on, unless the app
    /// began to stop meanwhile, which stops it again.
    fn attempt(self: &Arc<Self>, starter: &CoreStarter) -> Result<(), CoreFailure> {
        let start = {
            let mut state = self.lock();
            state.starts += 1;
            state.starts
        };
        let core = Arc::downgrade(self);
        let started = starter(Box::new(move || {
            if let Some(core) = core.upgrade() {
                core.closed(start);
            }
        }))?;
        let mut state = self.lock();
        if state.stopping {
            drop(state);
            started.stop();
            return Ok(());
        }
        // A daemon that ended before it was in hand was not the core's yet
        // when its bridge closed, so its end is read here.
        state.status = if started.bridge.is_closed() {
            CoreStatus::down(CORE_STOPPED, false)
        } else {
            CoreStatus::up()
        };
        state.current = start;
        state.editor = Some(started.editor);
        state.bridge = Some(started.bridge);
        state.roster = Some(started.roster);
        (self.report)(&state.status);
        Ok(())
    }

    fn fail(&self, failure: &CoreFailure) {
        eprintln!("consensflow: {}", failure.cause);
        self.tell(CoreStatus::down(&failure.cause, failure.retry));
    }

    /// A start's bridge closed: if it is the core's, the daemon stopped.
    fn closed(&self, start: u64) {
        let mut state = self.lock();
        if state.stopping || state.current != start || !state.status.available {
            return;
        }
        eprintln!("consensflow: {CORE_STOPPED}");
        state.status = CoreStatus::down(CORE_STOPPED, false);
        (self.report)(&state.status);
    }

    /// A change, told to the page, unless the app is stopping.
    fn tell(&self, status: CoreStatus) {
        let mut state = self.lock();
        if state.stopping {
            return;
        }
        state.status = status;
        (self.report)(&state.status);
    }

    /// Where the core stands, told again: a page that has just loaded missed
    /// what was told before it listened. Under the same lock as every change,
    /// so the last the page hears is the current one.
    fn tell_again(&self) {
        let state = self.lock();
        (self.report)(&state.status);
    }

    /// The bridge to ask while the core is up; why not otherwise.
    fn connection(&self) -> Result<Bridge, String> {
        let state = self.lock();
        match &state.bridge {
            Some(bridge) if state.status.available => Ok(bridge.clone()),
            _ => Err(state.status.cause.clone().unwrap_or_default()),
        }
    }

    fn roster(&self) -> Option<RosterHandle> {
        let state = self.lock();
        state
            .status
            .available
            .then(|| state.roster.clone())
            .flatten()
    }

    /// The app is stopping: no start is taken in, nothing more is told. The
    /// first call has the daemon and its bridge handed over to be stopped.
    fn stop(&self) -> Option<(Option<Child>, Option<Bridge>)> {
        let mut state = self.lock();
        if state.stopping {
            return None;
        }
        state.stopping = true;
        Some((state.editor.take(), state.bridge.clone()))
    }

    fn bridge(&self) -> Option<Bridge> {
        self.lock().bridge.clone()
    }
}

pub struct AppRuntime {
    panes: Arc<PaneTable>,
    core: Arc<Core>,
    output: Arc<OutputHub>,
    inputs: Arc<InputQueue>,
}

impl AppRuntime {
    pub(crate) fn pane_table(&self) -> Arc<PaneTable> {
        Arc::clone(&self.panes)
    }

    pub fn start(app: &AppHandle) -> Self {
        let panes = Arc::new(PaneTable::new());
        let output = Arc::new(OutputHub::new());
        let launches = Arc::new(LaunchRegistry::new());
        let arbiter = Arc::new(InputArbiter::new(ENTER_DELAY_MS));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));

        let reporter = app.clone();
        let core = Core::new(Arc::new(move |status: &CoreStatus| {
            if let Err(error) = reporter.emit(CORE_STATUS_EVENT, status) {
                eprintln!("consensflow page event {CORE_STATUS_EVENT}: {error}");
            }
        }));
        let starter: CoreStarter = {
            let app = app.clone();
            let panes = Arc::clone(&panes);
            let output = Arc::clone(&output);
            let inputs = Arc::clone(&inputs);
            Arc::new(move |closed| {
                let command = core_command(&app)?;
                let mut builder = BridgeBuilder::new(MAX_FRAME_BYTES);
                let page_app = app.clone();
                let page_events: PageEventSink = Arc::new(move |name, body| {
                    if let Err(error) = page_app.emit(name, body) {
                        eprintln!("consensflow page event {name}: {error}");
                    }
                });
                register_page_events(&mut builder, page_events);
                register_pane_handlers(
                    &mut builder,
                    Arc::clone(&panes),
                    Arc::clone(&arbiter),
                    Arc::clone(&output),
                    Arc::clone(&launches),
                    Arc::clone(&inputs),
                );
                builder.on_error(|error| eprintln!("consensflow bridge: {error}"));
                builder.on_close(closed);
                connect_core(command, builder)
            })
        };
        core.start(starter, Arc::clone(&panes), CORE_RESTART);
        Self {
            panes,
            core,
            output,
            inputs,
        }
    }

    pub fn shutdown(&self) {
        if self.begin_shutdown() {
            self.finish_shutdown();
        }
    }

    pub(crate) fn begin_shutdown(&self) -> bool {
        let Some((editor, bridge)) = self.core.stop() else {
            return false;
        };
        if let Some(mut editor) = editor {
            if let Some(bridge) = &bridge {
                bridge.close_input();
            }
            stop_editor(&mut editor);
        }
        true
    }

    pub(crate) fn finish_shutdown(&self) {
        let bridge = self.core.bridge();
        if let Some(bridge) = &bridge {
            let _ = bridge.wait_launches_closed();
        }
        reap_all(&self.panes);
        self.inputs.close_and_drain();
        if let Some(bridge) = &bridge {
            let _ = bridge.wait_closed();
        }
    }
}

fn reap_all(panes: &PaneTable) {
    if let Ok(open) = panes.list() {
        for pane in open {
            let _ = panes.kill(&PaneKey::new(pane.id, pane.generation));
        }
    }
}

fn request_node(connection: Result<Bridge, String>, operation: String, body: Value) -> Value {
    let bridge = match connection {
        Ok(bridge) => bridge,
        Err(cause) => return not_available(&operation, &cause),
    };
    match bridge.request(operation.clone(), body, None) {
        Ok(response) => normalize_node_response(&operation, response),
        Err(error) => json!({"ok":false,"error":error.to_string(),"operation":operation}),
    }
}

impl Drop for AppRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Gives the daemon a moment to stop on its own once its input has ended:
/// it writes down why it stopped and closes its ledger. The same on every
/// platform, since an input ending is the one stop Windows can deliver too.
/// Only what has not gone by then is killed, so a start with no stop after it
/// in the daemon's log means it was killed from outside, never by the app.
fn stop_editor(editor: &mut Child) {
    // The bridge holds the daemon's input and has let it go; a child whose
    // input is still ours (a stand-in in a test) gets its EOF here.
    drop(editor.stdin.take());
    let deadline = Instant::now() + EDITOR_STOP_GRACE;
    while Instant::now() < deadline {
        if matches!(editor.try_wait(), Ok(Some(_))) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let _ = editor.kill();
    let _ = editor.wait();
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OpenRequest {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    generation: Option<u64>,
    #[serde(default, alias = "launch")]
    launch_id: Option<String>,
    cwd: PathBuf,
    argv: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    drop_env: Vec<String>,
    #[serde(default)]
    size: SizeRequest,
    #[serde(default = "default_backlog_bytes")]
    backlog_bytes: usize,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SizeRequest {
    rows: u16,
    cols: u16,
}

impl Default for SizeRequest {
    fn default() -> Self {
        Self { rows: 24, cols: 80 }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaneRequest {
    id: String,
    generation: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResizeRequest {
    id: String,
    generation: u64,
    rows: u16,
    cols: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BytesRequest {
    id: String,
    generation: u64,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasteRequest {
    id: String,
    generation: u64,
    body: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PeerSendRequest {
    id: String,
    generation: u64,
    socket: PathBuf,
    peer_pid: i32,
    #[serde(default)]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    allow_descendant: bool,
    body: String,
    timeout_ms: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClaimRequest {
    pane: String,
    generation: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AckRequest {
    id: String,
    generation: u64,
    seq: u64,
}

fn default_backlog_bytes() -> usize {
    DEFAULT_BACKLOG_BYTES
}

/// The bundled daemon, `cf ui --json`, on the human's login PATH. A runtime
/// or CLI missing from the app is not worth another start.
fn core_command(app: &AppHandle) -> Result<Command, CoreFailure> {
    let (node, cli) = bundled_cli(app).map_err(|cause| CoreFailure {
        cause,
        retry: false,
    })?;
    let mut command = Command::new(node);
    command.arg(cli).args(["ui", "--json", "--no-open"]);
    if let Some(path) = login_path() {
        command.env("PATH", path);
    }
    // node.exe is a console program: started from a windowed app it gets a
    // console window of its own, and every console program it starts shows in
    // it. The daemon runs without one; its windows are the app's panes.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    Ok(command)
}

/// Starts a daemon and connects to it: its output carries the handle line,
/// then the bridge's frames; its input carries the app's.
fn connect_core(mut command: Command, builder: BridgeBuilder) -> Result<StartedCore, CoreFailure> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(daemon_stderr())
        .spawn()
        .map_err(|error| {
            CoreFailure::transient(format!(
                "the bundled ConsensFlow could not be started: {error}"
            ))
        })?;
    let input = child.stdout.take().ok_or_else(|| {
        CoreFailure::transient("the editor process gave no output to read".to_string())
    })?;
    let writer = child.stdin.take().ok_or_else(|| {
        CoreFailure::transient("the editor process gave no input pipe".to_string())
    })?;
    let failed = |child: &mut Child, cause: String| {
        let _ = child.kill();
        let _ = child.wait();
        CoreFailure::transient(cause)
    };
    let connected = match builder.connect(input, writer) {
        Ok(connected) => connected,
        Err(BridgeError::Eof) => {
            return Err(failed(
                &mut child,
                "ConsensFlow's core stopped before it was ready".to_string(),
            ));
        }
        Err(error) => {
            return Err(failed(
                &mut child,
                format!("could not connect to the bundled ConsensFlow: {error}"),
            ));
        }
    };
    match RosterHandle::from_value(connected.handle) {
        Ok(roster) => Ok(StartedCore {
            editor: child,
            bridge: connected.bridge,
            roster,
        }),
        Err(cause) => Err(failed(&mut child, cause)),
    }
}

/// Where the daemon's error output goes. On Windows a windowed app has no
/// stderr to hand down (inheriting an invalid handle fails the spawn), so the
/// daemon writes to `<home>/app/app.log`, the file the macOS build redirects
/// the app's own stderr to; elsewhere the daemon inherits the app's.
#[cfg(windows)]
fn daemon_stderr() -> Stdio {
    let home = std::env::var_os("CONSENSFLOW_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".consensflow")));
    let Some(home) = home else {
        return Stdio::null();
    };
    let directory = home.join("app");
    if std::fs::create_dir_all(&directory).is_err() {
        return Stdio::null();
    }
    match std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(directory.join("app.log"))
    {
        Ok(file) => Stdio::from(file),
        Err(_) => Stdio::null(),
    }
}

#[cfg(not(windows))]
fn daemon_stderr() -> Stdio {
    Stdio::inherit()
}

/// Node's `state.changed` becomes the page's `state-changed`.
///
/// The two names are not the same namespace and cannot be. Tauri 2 accepts
/// only alphanumerics, `-`, `/`, `:` and `_` in an event name, so the dotted
/// bridge name is REFUSED on the page side — `listen` rejects, and the
/// rejection took the page's whole start-up with it. The bridge keeps its
/// name; only the hop into the webview is renamed.
fn register_page_events(builder: &mut BridgeBuilder, sink: PageEventSink) {
    builder.on_event("state.changed", move |body| sink(PAGE_STATE_EVENT, body));
}

fn bundled_cli(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let resources = app
        .path()
        .resource_dir()
        .map_err(|error| format!("the app could not find its own resources: {error}"))?;
    // Tauri strips the target triple from a sidecar's name and, on Windows,
    // keeps the `.exe`: `node` on macOS, `node.exe` beside the app there.
    let sidecar = if cfg!(windows) { "node.exe" } else { "node" };
    let resource_node = resources.join("binaries").join(sidecar);
    let node = if resource_node.exists() {
        resource_node
    } else {
        std::env::current_exe()
            .map_err(|error| format!("the app could not find itself: {error}"))?
            .parent()
            .ok_or_else(|| "the app executable has no directory".to_string())?
            .join(sidecar)
    };
    let cli = resources.join("cli").join("bin").join("cf.mjs");
    // Tauri may answer its resource directory in Windows' verbatim form
    // (`\\?\C:\…`), which Node cannot take as a script path: it stops at
    // the drive with `lstat 'C:'`. The plain spelling names the same file.
    let node = plain_path(node);
    let cli = plain_path(cli);
    if !node.is_absolute() || !node.exists() {
        return Err(format!(
            "the bundled runtime is missing from this app ({node:?})"
        ));
    }
    if !cli.is_absolute() || !cli.exists() {
        return Err(format!(
            "the bundled ConsensFlow is missing from this app ({cli:?})"
        ));
    }
    Ok((node, cli))
}

/// A Windows path without the `\\?\` verbatim prefix; any other path as it is.
fn plain_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
        return PathBuf::from(format!("\\\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix("\\\\?\\") {
        return PathBuf::from(rest);
    }
    path
}

/// The PATH the human's login shell sets up, where their harness CLIs live,
/// for the daemon and every pane it opens.
fn login_path() -> Option<String> {
    login_path_in(Path::new(&std::env::var_os("SHELL")?), LOGIN_PATH_TIMEOUT)
}

/// A login file may print (`nvm use` does), wait for input or never finish,
/// and the PATH used to be the shell's whole output, read on the main thread
/// before the window existed, for as long as the shell took. So the PATH is
/// read between markers no login file prints, from a shell that exits
/// cleanly within `timeout`; otherwise there is none, and the daemon keeps
/// the PATH the app was started with.
fn login_path_in(shell: &Path, timeout: Duration) -> Option<String> {
    use std::hash::BuildHasher;
    use std::io::Read;

    let marker = format!(
        "<consensflow-path-{:016x}>",
        std::hash::RandomState::new().hash_one(std::process::id())
    );
    let mut child = Command::new(shell)
        .args(["-lc", &format!("printf '{marker}%s{marker}' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut output = child.stdout.take()?;
    let (chunks, printed) = mpsc::channel();
    // Not joined: a process a login file started may hold the output open
    // long after the shell has gone.
    thread::spawn(move || {
        let mut chunk = [0; 4096];
        while let Ok(read) = output.read(&mut chunk) {
            if read == 0 || chunks.send(chunk[..read].to_vec()).is_err() {
                return;
            }
        }
    });
    let deadline = Instant::now() + timeout;
    let succeeded = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    if !succeeded {
        return None;
    }
    // The shell has gone, so what it printed is in the pipe, a moment from
    // the reader at most.
    let mut bytes = Vec::new();
    loop {
        let text = String::from_utf8_lossy(&bytes);
        let mut parts = text.split(marker.as_str());
        if let (Some(_), Some(path), Some(_)) = (parts.next(), parts.next(), parts.next()) {
            return (!path.is_empty()).then(|| path.to_string());
        }
        bytes.extend(printed.recv_timeout(Duration::from_secs(1)).ok()?);
    }
}

fn register_pane_handlers(
    builder: &mut BridgeBuilder,
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    output: Arc<OutputHub>,
    launches: Arc<LaunchRegistry>,
    inputs: Arc<InputQueue>,
) {
    let open_panes = Arc::clone(&panes);
    let open_arbiter = Arc::clone(&arbiter);
    let open_inputs = Arc::clone(&inputs);
    let open_output = Arc::clone(&output);
    let open_launches = Arc::clone(&launches);
    builder.on_launch("pane.open", move |bridge, body| {
        let request: OpenRequest = parse_body(body)?;
        validate_open_request(&request)?;
        let owner_slot = match request.launch_id.as_deref() {
            Some(launch_id) => match open_launches.reserve(launch_id)? {
                LaunchClaim::Owner(slot) => Some(slot),
                LaunchClaim::Duplicate(slot) => {
                    return launch_response(slot.wait(), true);
                }
            },
            None => None,
        };

        let size = PtySize {
            rows: request.size.rows,
            cols: request.size.cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let opened = match (&request.id, request.generation) {
            (Some(id), Some(generation)) => open_panes.open_streamed_at(
                pane_key(id, generation)?,
                &request.cwd,
                &request.argv,
                PaneEnvironment::new(&request.env, &request.drop_env),
                size,
                request.backlog_bytes,
            ),
            (None, None) => open_panes.open_streamed(
                &request.cwd,
                &request.argv,
                PaneEnvironment::new(&request.env, &request.drop_env),
                size,
                request.backlog_bytes,
            ),
            _ => return Err("pane.open needs both id and generation, or neither".to_string()),
        }
        .map_err(|error| error.to_string());
        let streamed = match opened {
            Ok(streamed) => streamed,
            Err(error) => {
                if let Some(slot) = owner_slot {
                    slot.complete(Err(error.clone()));
                }
                return Err(error);
            }
        };
        let printed = match open_arbiter
            .register(&streamed.key)
            .map_err(|error| error.to_string())
            .and_then(|printed| open_inputs.open(&streamed.key).map(|()| printed))
        {
            Ok(printed) => printed,
            Err(error) => {
                let _ = open_panes.kill(&streamed.key);
                open_inputs.retire(&streamed.key);
                if let Some(slot) = owner_slot {
                    slot.complete(Err(error.clone()));
                }
                return Err(error);
            }
        };
        let key = streamed.key.clone();
        stream_to_page(
            streamed,
            bridge,
            Arc::clone(&open_panes),
            printed,
            Arc::clone(&open_inputs),
            Arc::clone(&open_output),
            Arc::clone(&open_launches),
            request.launch_id,
        );
        if let Some(slot) = owner_slot {
            slot.complete(Ok(key.clone()));
        }
        Ok(json!({"ok":true,"id":key.id,"generation":key.generation}))
    });

    // Keys typed into a pane and an emulator's replies (a page-less peer
    // answers a cursor query itself) are written alike.
    for operation in ["pane.input", "pane.reply"] {
        let input_queue = Arc::clone(&inputs);
        builder.on(operation, move |_bridge, body| {
            let request: BytesRequest = parse_body(body)?;
            validate_input(&request.bytes)?;
            let key = pane_key(&request.id, request.generation)?;
            wait_for_input_blocking(input_queue.write(key, request.bytes)?)?;
            Ok(json!({"ok":true}))
        });
    }

    let paste_queue = Arc::clone(&inputs);
    builder.on("pane.write_paste", move |_bridge, body| {
        let request: PasteRequest = parse_body(body)?;
        let key = pane_key(&request.id, request.generation)?;
        wait_for_input_blocking(paste_queue.paste(key, request.body.into_bytes())?)?;
        Ok(json!({"ok":true}))
    });

    let claim_queue = Arc::clone(&inputs);
    builder.on("pane.claim", move |_bridge, body| {
        let request: ClaimRequest = parse_body(body)?;
        let key = pane_key(&request.pane, request.generation)?;
        wait_for_input_blocking(claim_queue.claim(key)?)?;
        Ok(json!({"ok":true}))
    });

    let peer_panes = Arc::clone(&panes);
    let peer_queue = Arc::clone(&inputs);
    builder.on("pane.send_peer", move |_bridge, body| {
        let request: PeerSendRequest = parse_body(body)?;
        let key = pane_key(&request.id, request.generation)?;
        if !request.socket.is_absolute() || request.peer_pid <= 0
            || request.body.len() > MAX_INPUT_BYTES || !(1..=3000).contains(&request.timeout_ms) {
            return Ok(json!({"ok":false,"admitted":false,"bytesWritten":0,"error":"invalid native peer request"}));
        }
        #[cfg(target_os = "macos")]
        {
            let result = peer_panes.send_peer(&key, &request.socket, request.peer_pid, request.allow_descendant,
                request.body.as_bytes(), std::time::Duration::from_millis(request.timeout_ms), || {
                    wait_for_input_blocking(peer_queue.claim(key.clone())?)
                });
            Ok(match result {
                Ok(()) => json!({"ok":true}),
                Err(error) if error.uncertain => json!({"ok":false,"admitted":null,"error":"uncertain","cause":error.reason}),
                Err(error) => json!({"ok":false,"admitted":false,"bytesWritten":0,"error":error.code,"cause":error.reason}),
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (&peer_panes, &peer_queue, key);
            Ok(json!({"ok":false,"admitted":false,"bytesWritten":0,"error":"native peer identity is unsupported on this platform"}))
        }
    });

    let resize_panes = Arc::clone(&panes);
    builder.on("pane.resize", move |_bridge, body| {
        let request: ResizeRequest = parse_body(body)?;
        validate_size(request.cols, request.rows)?;
        resize_panes
            .resize(
                &pane_key(&request.id, request.generation)?,
                request.rows,
                request.cols,
            )
            .map_err(|error| error.to_string())?;
        Ok(json!({"ok":true}))
    });

    let ack_panes = Arc::clone(&panes);
    builder.on("pane.ack", move |_bridge, body| {
        let request: AckRequest = parse_body(body)?;
        validate_seq(request.seq)?;
        ack_panes
            .ack(&pane_key(&request.id, request.generation)?, request.seq)
            .map_err(|error| error.to_string())?;
        Ok(json!({"ok":true}))
    });

    let kill_panes = Arc::clone(&panes);
    let kill_inputs = Arc::clone(&inputs);
    let kill_launches = Arc::clone(&launches);
    builder.on("pane.kill", move |_bridge, body| {
        let request: PaneRequest = parse_body(body)?;
        let key = pane_key(&request.id, request.generation)?;
        kill_panes.kill(&key).map_err(|error| error.to_string())?;
        kill_inputs.retire(&key);
        kill_launches.remove_key(&key);
        Ok(json!({"ok":true}))
    });

    let list_panes = Arc::clone(&panes);
    builder.on("pane.list", move |_bridge, body| {
        let _: EmptyBody = parse_body(body)?;
        let panes = list_panes
            .list()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|pane| {
                json!({
                    "id":pane.id,
                    "generation":pane.generation,
                    "alive":pane.alive,
                    "idleMs":pane.idle_ms,
                    "processGroupId":list_panes.process_group_id(&PaneKey { id: pane.id.clone(), generation: pane.generation }).ok(),
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({"ok":true,"panes":panes}))
    });

    let snapshot_arbiter = Arc::clone(&arbiter);
    builder.on("pane.snapshot", move |_bridge, body| {
        let request: PaneRequest = parse_body(body)?;
        let snapshot = snapshot_arbiter
            .snapshot(&pane_key(&request.id, request.generation)?)
            .map_err(|error| error.to_string())?;
        Ok(json!({
            "ok":true,
            "generation":snapshot.generation,
            "pasteInFlight":snapshot.paste_in_flight,
            "inputFailed":snapshot.input_failed,
            "outputQuietMs":snapshot.output_quiet_ms,
        }))
    });
}

fn launch_response(result: Result<PaneKey, String>, deduplicated: bool) -> Result<Value, String> {
    result.map(|key| {
        json!({
            "ok":true,
            "id":key.id,
            "generation":key.generation,
            "deduplicated":deduplicated,
        })
    })
}

/// A pane's output, on to the page, and its end: `pane.exit`, after which a
/// pane whose program has gone leaves the table.
#[allow(
    clippy::too_many_arguments,
    reason = "A pane's output thread holds everything its end touches"
)]
fn stream_to_page(
    streamed: StreamedPane,
    bridge: Bridge,
    panes: Arc<PaneTable>,
    printed: Arc<OutputClock>,
    inputs: Arc<InputQueue>,
    output: Arc<OutputHub>,
    launches: Arc<LaunchRegistry>,
    launch_id: Option<String>,
) {
    thread::spawn(move || {
        let key = streamed.key;
        for message in streamed.output {
            printed.note();
            output.publish(message.into());
        }
        if let Some(launch_id) = launch_id {
            launches.remove_id(&launch_id);
        }
        let _ = bridge.event(
            "pane.exit",
            json!({"id":key.id,"generation":key.generation}),
        );
        if matches!(panes.retire_exited(&key), Ok(true)) {
            inputs.retire(&key);
        }
    });
}

/// The headless pane helper, running the WINDOW's handlers.
///
/// `consensflow-bridge` used to carry its own copy of the pane operations, and
/// a copy is a contract that drifts: it had no launch deduplication, no pane
/// id or generation on `pane.open`, and it never reported a natural
/// `pane.exit`. The real Node side speaks to the window, so against the helper
/// it could only be refused. There is nothing to keep in step here: this is
/// `register_pane_handlers`, the same `InputQueue`, the same `LaunchRegistry`
/// and the same shutdown drain the window uses, over stdin and stdout instead
/// of a webview.
///
/// Serves until the peer closes the transport, then reaps what it opened.
pub fn run_headless() -> Result<(), String> {
    let panes = Arc::new(PaneTable::new());
    let output = Arc::new(OutputHub::new());
    let launches = Arc::new(LaunchRegistry::new());
    let arbiter = Arc::new(InputArbiter::new(ENTER_DELAY_MS));
    let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));

    let mut builder = BridgeBuilder::new(MAX_FRAME_BYTES);
    register_pane_handlers(
        &mut builder,
        Arc::clone(&panes),
        Arc::clone(&arbiter),
        Arc::clone(&output),
        Arc::clone(&launches),
        Arc::clone(&inputs),
    );
    builder.on_error(|error| eprintln!("consensflow-bridge: {error}"));

    let bridge = builder
        .serve(
            std::io::stdin(),
            std::io::stdout(),
            &json!({"v":1,"kind":"consensflow-bridge"}),
        )
        .map_err(|error| error.to_string())?;

    // No page to draw into, so a pane's bytes go back over the same bridge, as
    // a stream: a burst waits for the peer to read instead of closing the
    // bridge. Registered after `serve` on purpose: whatever a pane produced in
    // between is parked in the hub and drains into this sink the moment it
    // attaches.
    let sink = bridge.clone();
    output.register_sink(Arc::new(move |message: PaneOutputMessage| {
        sink.stream_event("pane.output", json!(message)).is_ok()
    }));

    // The same order the window shuts down in, and for the same reason: the
    // peer's EOF is what closes admission, so the drain can only run after it.
    bridge
        .wait_launches_closed()
        .map_err(|error| error.to_string())?;
    reap_all(&panes);
    inputs.close_and_drain();
    bridge.wait_closed().map_err(|error| error.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyBody {}

fn parse_body<T: DeserializeOwned>(body: Value) -> Result<T, String> {
    serde_json::from_value(body).map_err(|error| format!("invalid-body: {error}"))
}

fn validate_open_request(request: &OpenRequest) -> Result<(), String> {
    if request.id.is_some() != request.generation.is_some() {
        return Err("pane.open needs both id and generation, or neither".to_string());
    }
    if !request.cwd.is_absolute() {
        return Err("pane cwd must be absolute".to_string());
    }
    if request.argv.is_empty() || !Path::new(&request.argv[0]).is_absolute() {
        return Err("pane argv[0] must be absolute".to_string());
    }
    if request.backlog_bytes == 0 {
        return Err("backlogBytes must be greater than zero".to_string());
    }
    validate_drop_env(&request.drop_env).map_err(|error| error.to_string())?;
    validate_size(request.size.cols, request.size.rows)?;
    if let Some(id) = &request.id {
        validate_text(id, "pane id")?;
    }
    if request.generation == Some(0) {
        return Err("generation must be a positive integer".to_string());
    }
    if let Some(launch_id) = &request.launch_id {
        validate_text(launch_id, "launch id")?;
    }
    Ok(())
}

fn pane_key(id: &str, generation: u64) -> Result<PaneKey, String> {
    validate_text(id, "pane id")?;
    if generation == 0 {
        return Err("generation must be a positive integer".to_string());
    }
    Ok(PaneKey::new(id, generation))
}

fn validate_text(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} is required"))
    } else {
        Ok(())
    }
}

fn validate_input(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_INPUT_BYTES {
        Err(format!("pane input exceeds {MAX_INPUT_BYTES} bytes"))
    } else {
        Ok(())
    }
}

fn validate_size(cols: u16, rows: u16) -> Result<(), String> {
    if cols == 0 || rows == 0 || cols > MAX_TERMINAL_DIMENSION || rows > MAX_TERMINAL_DIMENSION {
        Err(format!(
            "terminal size must be between 1 and {MAX_TERMINAL_DIMENSION}"
        ))
    } else {
        Ok(())
    }
}

fn validate_seq(seq: u64) -> Result<(), String> {
    if seq == 0 {
        Err("ack seq must be a positive integer".to_string())
    } else {
        Ok(())
    }
}

fn normalize_node_response(operation: &str, response: Value) -> Value {
    let unavailable = response
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|error| matches!(error, "unknown-op" | "not yet" | "not-yet"));
    if unavailable {
        not_available(operation, "the Node handler has not landed yet")
    } else {
        response
    }
}

fn not_available(operation: &str, detail: &str) -> Value {
    json!({
        "ok":false,
        "error":"not-available-yet",
        "operation":operation,
        "detail":detail,
    })
}

async fn run_blocking<F>(operation: &'static str, task: F) -> Value
where
    F: FnOnce() -> Value + Send + 'static,
{
    match tauri::async_runtime::spawn_blocking(task).await {
        Ok(value) => value,
        Err(error) => {
            json!({"ok":false,"error":format!("{operation} worker failed: {error}"),"operation":operation})
        }
    }
}

async fn input_result(result: Result<PageInputCompletion, String>) -> Value {
    let completion = match result {
        Ok(completion) => completion,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    match wait_for_input(completion.receiver).await {
        Ok(()) => json!({"ok":true}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}

fn enqueue_page_input<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    sequence: u64,
    work: InputWork,
    human: bool,
) -> Value {
    let key = match pane_key(&id, generation) {
        Ok(key) => key,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let inputs = {
        let state = app.state::<AppRuntime>();
        Arc::clone(&state.inputs)
    };
    match inputs.enqueue_page(key, sequence, work, human) {
        Ok(ticket) => json!({"ok":true,"ticket":ticket}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}

#[tauri::command]
pub fn pane_input_enqueue<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    sequence: u64,
    bytes: Vec<u8>,
) -> Value {
    enqueue_page_input(app, id, generation, sequence, InputWork::Write(bytes), true)
}

#[tauri::command]
pub fn pane_reply_enqueue<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    sequence: u64,
    bytes: Vec<u8>,
) -> Value {
    enqueue_page_input(app, id, generation, sequence, InputWork::Write(bytes), false)
}

#[tauri::command]
pub async fn pane_input_wait<R: Runtime>(app: AppHandle<R>, ticket: String) -> Value {
    if let Err(error) = validate_text(&ticket, "pane input ticket") {
        return json!({"ok":false,"error":error});
    }
    let inputs = {
        let state = app.state::<AppRuntime>();
        Arc::clone(&state.inputs)
    };
    let completion = inputs.take_page_completion(&ticket);
    let human = completion.as_ref().is_ok_and(|completion| completion.human);
    let result = input_result(completion).await;
    if human && result["ok"] == true {
        let _ = app.emit(PAGE_STATE_EVENT, json!({"reason":"human-input"}));
    }
    result
}

#[tauri::command]
pub async fn pane_resize<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    cols: u16,
    rows: u16,
) -> Value {
    if let Err(error) = validate_size(cols, rows) {
        return json!({"ok":false,"error":error});
    }
    let key = match pane_key(&id, generation) {
        Ok(key) => key,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let panes = {
        let state = app.state::<AppRuntime>();
        Arc::clone(&state.panes)
    };
    run_blocking("pane_resize", move || {
        match panes.resize(&key, rows, cols) {
            Ok(()) => json!({"ok":true}),
            Err(error) => json!({"ok":false,"error":error.to_string()}),
        }
    })
    .await
}

#[tauri::command]
pub async fn pane_ack<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    seq: u64,
) -> Value {
    if let Err(error) = validate_seq(seq) {
        return json!({"ok":false,"error":error});
    }
    let key = match pane_key(&id, generation) {
        Ok(key) => key,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let panes = {
        let state = app.state::<AppRuntime>();
        Arc::clone(&state.panes)
    };
    match panes.ack(&key, seq) {
        Ok(()) => json!({"ok":true}),
        Err(error) => json!({"ok":false,"error":error.to_string()}),
    }
}

async fn task_operation<R: Runtime>(
    app: AppHandle<R>,
    operation: &'static str,
    body: Value,
) -> Value {
    let connection = app.state::<AppRuntime>().core.connection();
    run_blocking(operation, move || {
        request_node(connection, operation.to_string(), body)
    })
    .await
}

/// What the board page may ask the new core. The page names the operation and
/// its body; anything else is refused here, before it reaches the daemon.
const CORE_OPERATIONS: &[&str] = &[
    "projects.list",
    "chief.switch",
    "project.open",
    "project.resume",
    "project.close",
    "project.delete",
    "project.gate",
    "board.get",
    "inbox.get",
    "member.add",
    "member.remove",
    "member.roles",
    "session.open",
    "session.close",
    "session.end",
    "task.get",
    "task.transcript",
    "task.cancel",
    "task.pause",
    "task.reassign",
    "task.resume",
    "message.read",
    "message.approve",
    "message.decline",
    "agents.list",
    "staff.last",
];

#[tauri::command]
pub async fn core_request<R: Runtime>(app: AppHandle<R>, operation: String, body: Value) -> Value {
    let Some(operation) = CORE_OPERATIONS
        .iter()
        .find(|allowed| **allowed == operation)
    else {
        return json!({"ok":false,"error":format!("unknown core operation {operation}")});
    };
    if !body.is_object() {
        return json!({"ok":false,"error":"a core request body is an object"});
    }
    task_operation(app, operation, body).await
}

/// The page's pane-output subscription, taken ONCE for the life of the page.
///
/// It used to ride on every `state.list`, and that was silently destructive:
/// Tauri builds a fresh `Channel` for each invocation carrying one, and
/// dropping the previous one emits `{end:true}` to the JavaScript callback
/// the page reuses. So the SECOND refresh tore down the live subscription and
/// every pane went blank for the rest of the session — with no error
/// anywhere, because the sends that followed still returned ok into a channel
/// nothing was listening to. Only the packaged app could show it: a shimmed
/// page test has no real channel to end.
///
/// Once per page is also what makes it the start of the page's input count.
/// The page numbers its input in its own memory, so a reload (WebKit's content
/// process replaced, or the human's Reload) starts again from 1; every pane's
/// next keystrokes were refused as a regression until the app restarted.
///
/// And it is when a new page hears where the core stands: what was told
/// before it listened (a first start fails before the window exists) is told
/// again, so the page listens to `core-status` before it subscribes.
#[tauri::command]
pub async fn subscribe_output<R: Runtime>(
    app: AppHandle<R>,
    on_output: Channel<PaneOutputMessage>,
) -> Value {
    let (output, inputs, core) = {
        let state = app.state::<AppRuntime>();
        (
            Arc::clone(&state.output),
            Arc::clone(&state.inputs),
            Arc::clone(&state.core),
        )
    };
    inputs.begin_page();
    output.register(on_output);
    core.tell_again();
    json!({"ok":true})
}

/// Where the human's agents screens are: the daemon's URL and the UI token it
/// handed the app, or null while the daemon is not up.
#[tauri::command]
pub fn roster_handle<R: Runtime>(app: AppHandle<R>) -> Value {
    match app.state::<AppRuntime>().core.roster() {
        Some(roster) => serde_json::to_value(roster).unwrap_or(Value::Null),
        None => Value::Null,
    }
}

/// The agents screens (the agents, the harnesses) in their own
/// window at the daemon's address. The board's page cannot frame them: it
/// is served over the app's secure scheme and WebKit blocks a plain-HTTP
/// frame inside it as mixed content. A second window loads the address as a
/// top-level page, which is allowed. One window, reused: a later call turns
/// it to the asked page and brings it forward.
// Async on purpose: on Windows a window built from a synchronous command
// deadlocks with the main thread (a white window that neither loads nor closes).
#[tauri::command]
pub async fn open_agents_window<R: Runtime>(app: AppHandle<R>, page: String) -> Value {
    let Some(roster) = app.state::<AppRuntime>().core.roster() else {
        return json!({"ok":false,"error":"the agents screens are not available: the daemon is not up"});
    };
    let url = match agents_url(&roster, &page) {
        Ok(url) => url,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    if let Some(window) = app.get_webview_window(AGENTS_WINDOW) {
        if let Err(error) = window.navigate(url.clone()) {
            return json!({"ok":false,"error":format!("the agents window could not turn to {page:?}: {error}")});
        }
        let _ = window.set_focus();
        return json!({"ok":true,"label":AGENTS_WINDOW,"url":url.as_str(),"reused":true});
    }
    match tauri::WebviewWindowBuilder::new(&app, AGENTS_WINDOW, tauri::WebviewUrl::External(url.clone()))
        .title("ConsensFlow agents")
        .inner_size(1120.0, 820.0)
        .build()
    {
        Ok(_) => json!({"ok":true,"label":AGENTS_WINDOW,"url":url.as_str(),"reused":false}),
        Err(error) => json!({"ok":false,"error":format!("the agents window could not open: {error}")}),
    }
}

pub(crate) const AGENTS_WINDOW: &str = "agents";
const AGENTS_PAGES: &[&str] = &["", "harnesses"];

/// The daemon's page for one agents screen, carrying the UI token.
fn agents_url(roster: &RosterHandle, page: &str) -> Result<tauri::Url, String> {
    if !AGENTS_PAGES.contains(&page) {
        return Err(format!("no agents screen {page:?}"));
    }
    let mut url = tauri::Url::parse(&roster.url)
        .map_err(|error| format!("the editor handle is not an address: {error}"))?;
    url.set_path(&format!("/{page}"));
    url.query_pairs_mut()
        .clear()
        .append_pair("token", &roster.token);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::io::Read;
    #[cfg(unix)]
    use std::time::Duration;

    /// A runtime around what a test stands up: its panes and input queue, and
    /// a daemon and its bridge when the test has them.
    fn runtime(
        panes: Arc<PaneTable>,
        inputs: Arc<InputQueue>,
        editor: Option<Child>,
        bridge: Option<Bridge>,
    ) -> AppRuntime {
        let core = Core::new(Arc::new(|_: &CoreStatus| {}));
        {
            let mut state = core.lock();
            if bridge.is_some() {
                state.status = CoreStatus::up();
            }
            state.editor = editor;
            state.bridge = bridge;
        }
        AppRuntime {
            panes,
            core,
            output: Arc::new(OutputHub::new()),
            inputs,
        }
    }

    /// Starts between failures, short enough for a test.
    const QUICK: Backoff = Backoff {
        first: Duration::from_millis(10),
        most: Duration::from_millis(40),
    };

    /// What the page hears of the core, as a test hears it.
    fn listener() -> (CoreReport, mpsc::Receiver<CoreStatus>) {
        let (told, heard) = mpsc::channel();
        (
            Arc::new(move |status: &CoreStatus| {
                let _ = told.send(status.clone());
            }),
            heard,
        )
    }

    /// A stand-in for `cf ui --json`, started and connected the way the app
    /// starts the daemon; it prints its handle line when `ready`.
    #[cfg(unix)]
    fn stand_in_core(
        ready: bool,
        then: &str,
        closed: Box<dyn Fn() + Send + Sync>,
    ) -> Result<StartedCore, CoreFailure> {
        let handle = r#"printf '%s\n' '{"url":"http://localhost:1/","token":"t"}'; "#;
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(format!("{}{then}", if ready { handle } else { "" }));
        let mut builder = BridgeBuilder::new(1024);
        builder.on_close(closed);
        connect_core(command, builder)
    }

    /// A daemon that stops before it is ready (its ledger still held by one
    /// finishing its stop) is started again, waiting longer each time, and
    /// the page hears each failure and the start that worked.
    #[cfg(unix)]
    #[test]
    fn a_failed_daemon_start_is_tried_again_and_the_page_is_told() {
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: CoreStarter = Arc::new(move |closed| {
            if counted.fetch_add(1, Ordering::SeqCst) < 2 {
                stand_in_core(false, "exit 1", closed)
            } else {
                stand_in_core(true, "while read -r line; do :; done", closed)
            }
        });
        let (report, heard) = listener();
        let core = Core::new(report);
        let panes = Arc::new(PaneTable::new());
        core.start(starter, Arc::clone(&panes), QUICK);

        let not_ready = CoreStatus::down("ConsensFlow's core stopped before it was ready", true);
        for told in [not_ready.clone(), not_ready, CoreStatus::up()] {
            assert_eq!(
                heard.recv_timeout(Duration::from_secs(5)).expect("told"),
                told
            );
        }
        assert_eq!(starts.load(Ordering::SeqCst), 3);
        assert!(core.connection().is_ok());
        assert!(core.roster().is_some());

        let arbiter = Arc::new(InputArbiter::new(0));
        let runtime = AppRuntime {
            inputs: Arc::new(InputQueue::new(Arc::clone(&panes), arbiter)),
            panes,
            core,
            output: Arc::new(OutputHub::new()),
        };
        runtime.shutdown();
        assert!(
            heard.recv_timeout(Duration::from_millis(200)).is_err(),
            "an app that is quitting tells the page nothing"
        );
    }

    /// A daemon that stops while the app runs is not started again (its
    /// windows still run in the pane host), and the page is told.
    #[cfg(unix)]
    #[test]
    fn a_daemon_that_stops_mid_session_is_told_and_not_started_again() {
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: CoreStarter = Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            stand_in_core(true, "sleep 0.5", closed)
        });
        let (report, heard) = listener();
        let core = Core::new(report);
        core.start(starter, Arc::new(PaneTable::new()), QUICK);

        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("up"),
            CoreStatus::up()
        );
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("stopped"),
            CoreStatus::down(CORE_STOPPED, false)
        );
        thread::sleep(Duration::from_millis(200));
        assert_eq!(starts.load(Ordering::SeqCst), 1, "started again");
        assert_eq!(core.connection().err().as_deref(), Some(CORE_STOPPED));
        assert!(core.roster().is_none());
    }

    /// A runtime missing from the app does not come back: the page is told,
    /// and nothing starts again.
    #[test]
    fn a_missing_runtime_is_told_and_not_tried_again() {
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: CoreStarter = Arc::new(move |_closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            Err(CoreFailure {
                cause: "the bundled runtime is missing from this app".to_string(),
                retry: false,
            })
        });
        let (report, heard) = listener();
        Core::new(report).start(starter, Arc::new(PaneTable::new()), QUICK);

        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("told"),
            CoreStatus::down("the bundled runtime is missing from this app", false)
        );
        thread::sleep(Duration::from_millis(100));
        assert_eq!(starts.load(Ordering::SeqCst), 1, "tried again");
    }

    /// A start is tried again only while no pane is open: one that is open
    /// may be a window of a daemon that got past its handle line.
    #[cfg(unix)]
    #[test]
    fn no_start_is_tried_again_while_a_pane_is_open() {
        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let opened = panes
            .open(
                Path::new("/tmp"),
                &["/bin/sh".to_string(), "-c".to_string(), "sleep 30".to_string()],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open a pane");
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: CoreStarter = Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            stand_in_core(false, "exit 1", closed)
        });
        let (report, heard) = listener();
        Core::new(report).start(starter, Arc::clone(&panes), QUICK);

        let cause = "ConsensFlow's core stopped before it was ready";
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("failed"),
            CoreStatus::down(cause, true)
        );
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("gave up"),
            CoreStatus::down(cause, false)
        );
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        panes.kill(&opened.key).expect("kill the pane");
    }

    /// A page that loads after the core failed hears it when it subscribes,
    /// in the shape the page reads.
    #[test]
    fn a_new_page_hears_where_the_core_stands() {
        use tauri::Listener;

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let reporter = app.handle().clone();
        let core = Core::new(Arc::new(move |status: &CoreStatus| {
            reporter
                .emit(CORE_STATUS_EVENT, status)
                .expect("emit the core's status");
        }));
        core.tell(CoreStatus::down("the core is held up", true));
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        app.manage(AppRuntime {
            inputs: Arc::new(InputQueue::new(Arc::clone(&panes), arbiter)),
            panes,
            core,
            output: Arc::new(OutputHub::new()),
        });
        let (told, heard) = mpsc::channel();
        app.listen(CORE_STATUS_EVENT, move |event| {
            let _ = told.send(event.payload().to_string());
        });

        let subscribed = tauri::async_runtime::block_on(subscribe_output(
            app.handle().clone(),
            Channel::new(|_| Ok(())),
        ));
        assert_eq!(subscribed["ok"], true);
        let payload = heard
            .recv_timeout(Duration::from_secs(1))
            .expect("the page hears where the core stands");
        assert_eq!(
            serde_json::from_str::<Value>(&payload).expect("JSON"),
            json!({"available":false,"cause":"the core is held up","retrying":true})
        );
        drop(app);
    }

    /// A stand-in for the human's login shell: `body` runs with the command
    /// the app gives it (`-lc <command>`) as `$2`.
    #[cfg(unix)]
    fn login_shell(home: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let shell = home.join("login-shell");
        std::fs::write(&shell, format!("#!/bin/sh\n{body}\n")).expect("write the shell");
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755))
            .expect("make the shell executable");
        shell
    }

    /// What a login file prints (`nvm use` says which Node it took) is not
    /// part of the PATH.
    #[cfg(unix)]
    #[test]
    fn the_login_path_is_read_past_what_login_files_print() {
        let home = tempfile::tempdir().expect("home");
        let shell = login_shell(
            home.path(),
            "echo 'Now using node v22.9.0 (npm v10.8.3)'; exec /bin/sh -c \"$2\"",
        );
        assert_eq!(
            login_path_in(&shell, Duration::from_secs(5)),
            std::env::var("PATH").ok()
        );
    }

    /// A login shell that fails, or takes too long, leaves the daemon on the
    /// PATH the app was started with; the app's start does not wait on it.
    #[cfg(unix)]
    #[test]
    fn a_login_shell_that_fails_or_hangs_gives_no_path() {
        let home = tempfile::tempdir().expect("home");
        let failing = login_shell(home.path(), "/bin/sh -c \"$2\"; exit 3");
        assert_eq!(login_path_in(&failing, Duration::from_secs(5)), None);

        let hanging = login_shell(home.path(), "exec /bin/sleep 3");
        let started = Instant::now();
        assert_eq!(login_path_in(&hanging, Duration::from_millis(300)), None);
        assert!(started.elapsed() < Duration::from_secs(2), "the shell held up the start");
    }

    /// Something a login file starts may keep the shell's output open after
    /// the shell has gone; what the shell printed is read all the same.
    #[cfg(unix)]
    #[test]
    fn a_login_path_is_read_while_a_started_process_holds_the_output() {
        let home = tempfile::tempdir().expect("home");
        let shell = login_shell(home.path(), "/bin/sleep 3 & exec /bin/sh -c \"$2\"");
        let started = Instant::now();
        assert_eq!(
            login_path_in(&shell, Duration::from_secs(5)),
            std::env::var("PATH").ok()
        );
        assert!(started.elapsed() < Duration::from_secs(2), "the read waited for the process");
    }

    #[test]
    fn headless_output_includes_companion_panes() {
        let hub = OutputHub::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let copy = Arc::clone(&seen);
        hub.register_sink(Arc::new(move |message| {
            copy.lock().unwrap().push(message.id);
            true
        }));
        hub.publish(PaneOutputMessage {
            id: "p-pm".into(),
            generation: 1,
            seq: 1,
            bytes: vec![65],
        });
        assert_eq!(*seen.lock().unwrap(), vec!["p-pm"]);
    }

    #[test]
    fn only_main_window_has_application_command_authority() {
        assert!(window_command_allowed("main", "open_pm"));
        assert!(!window_command_allowed("pm-t-2", "open_pm"));
        assert!(!window_command_allowed("stranger", "pane_input_enqueue"));
    }

    #[test]
    fn main_subscription_receives_pm_and_lead_output_without_replacing_either() {
        let hub = OutputHub::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let copy = Arc::clone(&seen);
        hub.attach(Arc::new(move |message| {
            copy.lock().unwrap().push(message.id);
            true
        }));
        for id in ["p-pm", "p-chief", "p-pm"] {
            hub.publish(PaneOutputMessage {
                id: id.into(),
                generation: 1,
                seq: 1,
                bytes: vec![65],
            });
        }
        assert_eq!(*seen.lock().unwrap(), vec!["p-pm", "p-chief", "p-pm"]);
    }

    #[test]
    fn unknown_node_operations_are_explicitly_not_available() {
        assert_eq!(
            normalize_node_response("answers.list", json!({"ok":false,"error":"unknown-op"})),
            json!({
                "ok":false,
                "error":"not-available-yet",
                "operation":"answers.list",
                "detail":"the Node handler has not landed yet",
            })
        );
    }

    #[test]
    fn roster_handle_accepts_only_loopback_http_and_normalizes_localhost() {
        let handle = RosterHandle::from_value(json!({
            "url":"http://127.0.0.1:43123/",
            "token":"secret",
        }))
        .unwrap();
        assert_eq!(handle.url, "http://localhost:43123/");
        assert!(RosterHandle::from_value(json!({
            "url":"https://example.com/",
            "token":"secret",
        }))
        .is_err());
    }

    #[test]
    fn agents_screens_open_at_the_daemon_pages_with_the_token() {
        let roster = RosterHandle::from_value(json!({
            "url":"http://127.0.0.1:43123/",
            "token":"secret",
        }))
        .unwrap();
        assert_eq!(
            agents_url(&roster, "").unwrap().as_str(),
            "http://localhost:43123/?token=secret"
        );
        assert_eq!(
            agents_url(&roster, "harnesses").unwrap().as_str(),
            "http://localhost:43123/harnesses?token=secret"
        );
        assert_eq!(
            agents_url(&roster, "harnesses").unwrap().as_str(),
            "http://localhost:43123/harnesses?token=secret"
        );
        assert!(agents_url(&roster, "admin").is_err());
        assert!(agents_url(&roster, "../etc").is_err());
    }

    #[test]
    fn browser_input_and_dimensions_are_bounded() {
        assert!(validate_input(&vec![0; MAX_INPUT_BYTES]).is_ok());
        assert!(validate_input(&vec![0; MAX_INPUT_BYTES + 1]).is_err());
        assert!(validate_size(80, 24).is_ok());
        assert!(validate_size(0, 24).is_err());
        assert!(validate_size(MAX_TERMINAL_DIMENSION + 1, 24).is_err());
        assert!(pane_key("", 1).is_err());
        assert!(pane_key("pane", 0).is_err());
    }

    #[test]
    fn a_verbatim_windows_path_is_spelled_plainly_for_node() {
        assert_eq!(
            plain_path(PathBuf::from(r"\\?\C:\Users\me\app\cli\bin\cf.mjs")),
            PathBuf::from(r"C:\Users\me\app\cli\bin\cf.mjs")
        );
        assert_eq!(
            plain_path(PathBuf::from(r"\\?\UNC\server\share\cf.mjs")),
            PathBuf::from(r"\\server\share\cf.mjs")
        );
        assert_eq!(
            plain_path(PathBuf::from("/Applications/ConsensFlow.app/node")),
            PathBuf::from("/Applications/ConsensFlow.app/node")
        );
    }

    #[test]
    fn open_request_requires_absolute_launch_inputs() {
        // A directory and a program that are absolute here: `/tmp` and
        // `/bin/sh` are not, on Windows.
        let (directory, program) = if cfg!(windows) {
            ("C:\\Windows", "C:\\Windows\\System32\\cmd.exe")
        } else {
            ("/tmp", "/bin/sh")
        };
        let relative: OpenRequest = parse_body(json!({
            "cwd":"relative",
            "argv":["sh"],
            "size":{"rows":24,"cols":80},
        }))
        .unwrap();
        assert!(validate_open_request(&relative).is_err());

        let absolute: OpenRequest = parse_body(json!({
            "id":"p-1",
            "generation":1,
            "cwd":directory,
            "argv":[program],
            "dropEnv":["OPENAI_API_KEY"],
            "size":{"rows":24,"cols":80},
        }))
        .unwrap();
        assert!(validate_open_request(&absolute).is_ok());

        let half_reserved: OpenRequest = parse_body(json!({
            "id":"p-1",
            "launchId":"launch-1",
            "cwd":directory,
            "argv":[program],
            "size":{"rows":24,"cols":80},
        }))
        .unwrap();
        assert!(validate_open_request(&half_reserved).is_err());

        let invalid_drop_env: OpenRequest = parse_body(json!({
            "cwd":directory,
            "argv":[program],
            "dropEnv":["BAD=NAME"],
        }))
        .unwrap();
        assert!(validate_open_request(&invalid_drop_env)
            .unwrap_err()
            .contains("environment variable name"));

        assert!(parse_body::<OpenRequest>(json!({
            "cwd":directory,
            "argv":[program],
            "dropEnv":[],
            "silentlyIgnoredSecurityField":true,
        }))
        .is_err());
    }

    #[test]
    fn every_tauri_command_that_waits_on_node_or_a_pty_is_async() {
        let source = include_str!("commands.rs");
        for command in ["pane_input_enqueue", "pane_reply_enqueue"] {
            assert!(
                source.contains(&format!("pub fn {command}")),
                "{command} must admit input synchronously on the IPC thread"
            );
            assert!(
                !source.contains(&format!("pub async fn {command}")),
                "{command} must not be scheduled before input admission"
            );
        }
        for command in [
            "pane_input_wait",
            "pane_resize",
            "pane_ack",
        ] {
            assert!(
                source.contains(&format!("pub async fn {command}")),
                "{command} can wait and must leave Tauri's main thread"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn production_ipc_arrivals_admit_1000_human_writes_in_order() {
        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let key = PaneKey::new("ordered-command", 1);
        let mut reader = panes
            .open_at(
                key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "/bin/stty raw -echo; printf ready; /usr/bin/od -An -tx1 -N 5000".to_string(),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open ordered pane");
        arbiter.register(&key).expect("register ordered pane");
        inputs.open(&key).expect("open the pane's input");
        let mut ready = [0; 5];
        reader
            .read_exact(&mut ready)
            .expect("read readiness marker");
        assert_eq!(&ready, b"ready");
        let output_reader = thread::spawn(move || {
            let mut output = Vec::new();
            reader.read_to_end(&mut output).expect("read ordered bytes");
            output
        });

        let runtime = runtime(Arc::clone(&panes), inputs, None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![pane_input_enqueue])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");
        let (responses, received) = mpsc::channel();
        for index in 0..1000 {
            let response_sender = responses.clone();
            webview.clone().on_message(
                tauri::webview::InvokeRequest {
                    cmd: "pane_input_enqueue".into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "tauri://localhost".parse().expect("invoke URL"),
                    body: tauri::ipc::InvokeBody::Json(json!({
                        "id":key.id,
                        "generation":key.generation,
                        "sequence":index + 1,
                        "bytes":format!("{index:04}|").into_bytes(),
                    })),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
                Box::new(move |_webview, _command, response, _callback, _error| {
                    response_sender
                        .send(response)
                        .expect("record command response");
                }),
            );
        }
        drop(responses);
        for response in received.iter().take(1000) {
            let value = match response {
                tauri::ipc::InvokeResponse::Ok(body) => {
                    body.deserialize::<Value>().expect("command response JSON")
                }
                tauri::ipc::InvokeResponse::Err(error) => {
                    panic!("pane_input_enqueue failed: {error:?}")
                }
            };
            assert_eq!(value["ok"], true, "pane_input_enqueue response: {value:?}");
            assert!(
                value["ticket"].is_string(),
                "missing input ticket: {value:?}"
            );
        }

        let output = output_reader.join().expect("join ordered output reader");
        let actual = String::from_utf8(output)
            .expect("od output is UTF-8")
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).expect("hex byte"))
            .collect::<Vec<_>>();
        let expected = (0..1000)
            .flat_map(|index| format!("{index:04}|").into_bytes())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);

        drop(webview);
        drop(app);
    }

    #[cfg(unix)]
    #[test]
    fn production_ipc_consumes_sequence_before_size_refusal() {
        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let key = PaneKey::new("sequenced-command", 1);
        let mut reader = panes
            .open_at(
                key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "/bin/stty raw -echo; printf ready; /usr/bin/od -An -tx1 -N 2".to_string(),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open sequenced pane");
        arbiter.register(&key).expect("register sequenced pane");
        inputs.open(&key).expect("open the pane's input");
        let mut ready = [0; 5];
        reader
            .read_exact(&mut ready)
            .expect("read readiness marker");
        assert_eq!(&ready, b"ready");

        let runtime = runtime(Arc::clone(&panes), inputs, None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![
                pane_input_enqueue,
                pane_reply_enqueue,
                pane_input_wait
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");
        let invoke = |command: &str, args: Value| {
            tauri::test::get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: command.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "tauri://localhost".parse().expect("invoke URL"),
                    body: tauri::ipc::InvokeBody::Json(args),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            )
            .expect("command succeeds")
            .deserialize::<Value>()
            .expect("command response JSON")
        };

        let oversized = invoke(
            "pane_input_enqueue",
            json!({
                "id":key.id,
                "generation":1,
                "sequence":1,
                "bytes":vec![b'P'; MAX_INPUT_BYTES + 1],
            }),
        );
        assert_eq!(
            oversized,
            json!({
                "ok":false,
                "error":format!("pane input exceeds {MAX_INPUT_BYTES} bytes"),
            })
        );
        let first = invoke(
            "pane_input_enqueue",
            json!({"id":key.id,"generation":1,"sequence":2,"bytes":[75]}),
        );
        assert_eq!(first["ok"], true);
        let gap = invoke(
            "pane_input_enqueue",
            json!({"id":key.id,"generation":1,"sequence":4,"bytes":[66]}),
        );
        assert_eq!(gap, json!({"ok":false,"error":"pane-input-sequence-gap"}));
        let regression = invoke(
            "pane_input_enqueue",
            json!({"id":key.id,"generation":1,"sequence":2,"bytes":[82]}),
        );
        assert_eq!(
            regression,
            json!({"ok":false,"error":"pane-input-sequence-regression"})
        );
        let first_completed = invoke("pane_input_wait", json!({"ticket":first["ticket"]}));
        assert_eq!(first_completed["ok"], true);
        // A rejected page input still consumes its sequence, so the next
        // sequence remains valid.
        let rejected = invoke(
            "pane_input_enqueue",
            json!({
                "id":key.id,"generation":1,"sequence":3,"bytes":vec![b'P'; MAX_INPUT_BYTES + 1],
            }),
        );
        assert_eq!(rejected["ok"], false);
        let second = invoke(
            "pane_reply_enqueue",
            json!({"id":key.id,"generation":1,"sequence":4,"bytes":[67]}),
        );
        assert_eq!(second["ok"], true);

        let completed = invoke("pane_input_wait", json!({"ticket":second["ticket"]}));
        assert_eq!(completed["ok"], true);
        let mut output = Vec::new();
        reader
            .read_to_end(&mut output)
            .expect("read sequenced bytes");
        let actual = output
            .split(|byte| byte.is_ascii_whitespace())
            .filter(|field| !field.is_empty())
            .map(|byte| u8::from_str_radix(std::str::from_utf8(byte).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, b"KC");

        drop(webview);
        drop(app);
    }

    /// A reloaded page counts every pane's input from 1 again: its
    /// subscription starts the count anew, and what the page before it was
    /// still owed answers goes with it.
    #[cfg(unix)]
    #[test]
    fn a_reloaded_page_types_into_the_panes_it_finds() {
        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let key = PaneKey::new("reloaded-page", 1);
        let mut reader = panes
            .open_at(
                key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "/bin/stty raw -echo; printf ready; /usr/bin/od -An -tx1 -N 3".to_string(),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open the pane");
        arbiter.register(&key).expect("register the pane");
        inputs.open(&key).expect("open the pane's input");
        let mut ready = [0; 5];
        reader.read_exact(&mut ready).expect("read readiness marker");
        assert_eq!(&ready, b"ready");

        let runtime = runtime(Arc::clone(&panes), inputs, None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let handle = app.handle().clone();
        let subscribe = || {
            let subscribed = tauri::async_runtime::block_on(subscribe_output(
                handle.clone(),
                Channel::new(|_| Ok(())),
            ));
            assert_eq!(subscribed["ok"], true);
        };
        let wait = |ticket: &Value| {
            tauri::async_runtime::block_on(pane_input_wait(
                handle.clone(),
                ticket.as_str().expect("a ticket").to_string(),
            ))
        };

        subscribe();
        let first = pane_input_enqueue(handle.clone(), key.id.clone(), 1, 1, b"A".to_vec());
        assert_eq!(wait(&first["ticket"]), json!({"ok":true}));
        // The page goes away with this one admitted and never waited for.
        let orphaned = pane_input_enqueue(handle.clone(), key.id.clone(), 1, 2, b"B".to_vec());
        assert_eq!(orphaned["ok"], true);

        subscribe();
        let typed = pane_input_enqueue(handle.clone(), key.id.clone(), 1, 1, b"C".to_vec());
        assert_eq!(typed["ok"], true, "the new page's first keystroke: {typed}");
        assert_eq!(wait(&typed["ticket"]), json!({"ok":true}));
        assert_eq!(
            wait(&orphaned["ticket"]),
            json!({"ok":false,"error":"pane-input-ticket-not-found"}),
            "the old page's tickets went with it"
        );

        let mut output = Vec::new();
        reader.read_to_end(&mut output).expect("read what reached the pane");
        assert_eq!(
            String::from_utf8(output)
                .expect("od output is UTF-8")
                .split_whitespace()
                .collect::<String>(),
            "414243",
            "every admitted keystroke reached the pane, in order"
        );
        drop(app);
    }

    #[cfg(unix)]
    #[test]
    fn blocked_command_input_does_not_starve_another_pane_or_output_ack() {
        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let blocked_key = PaneKey::new("blocked-command", 1);
        let mut blocked_reader = panes
            .open_at(
                blocked_key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "/bin/stty raw -echo; /bin/sleep 4; /bin/cat >/dev/null".to_string(),
                ],
                &HashMap::new(),
                size,
            )
            .expect("open blocked pane");
        // Like the production output pump, drain echoed startup bytes. Keeping an
        // unread PTY master open can block macOS child exit even after SIGKILL.
        let blocked_output = thread::spawn(move || {
            let _ = std::io::copy(&mut blocked_reader, &mut std::io::sink());
        });
        arbiter
            .register(&blocked_key)
            .expect("register blocked pane");
        inputs
            .open(&blocked_key)
            .expect("open the blocked pane's input");

        let responsive_key = PaneKey::new("responsive-command", 1);
        let responsive = panes
            .open_streamed_at(
                responsive_key.clone(),
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "printf ready; sleep 30".to_string(),
                ],
                PaneEnvironment::new(&HashMap::new(), &[]),
                size,
                1024,
            )
            .expect("open responsive pane");
        arbiter
            .register(&responsive_key)
            .expect("register responsive pane");
        inputs
            .open(&responsive_key)
            .expect("open the responsive pane's input");
        let first_output = responsive
            .output
            .recv_timeout(Duration::from_secs(2))
            .expect("responsive pane output");

        let runtime = runtime(Arc::clone(&panes), Arc::clone(&inputs), None, None);
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let handle = app.handle().clone();

        let blocked_outcomes = (0..512)
            .map(|index| {
                pane_input_enqueue(
                    handle.clone(),
                    blocked_key.id.clone(),
                    blocked_key.generation,
                    index + 1,
                    vec![b'x'; MAX_INPUT_BYTES],
                )
            })
            .collect::<Vec<_>>();
        thread::sleep(Duration::from_millis(500));

        let (responsive_sender, responsive_receiver) = mpsc::channel();
        let responsive_handle = handle.clone();
        let responsive_admission = pane_input_enqueue(
            handle.clone(),
            responsive_key.id.clone(),
            responsive_key.generation,
            1,
            b"R".to_vec(),
        );
        assert_eq!(responsive_admission["ok"], true);
        let responsive_ticket = responsive_admission["ticket"]
            .as_str()
            .expect("responsive input ticket")
            .to_string();
        let responsive_thread = thread::spawn(move || {
            let value = tauri::async_runtime::block_on(pane_input_wait(
                responsive_handle,
                responsive_ticket,
            ));
            responsive_sender
                .send(value)
                .expect("record responsive input result");
        });

        let (ack_sender, ack_receiver) = mpsc::channel();
        let ack_handle = handle;
        let ack_id = responsive_key.id.clone();
        let ack_generation = responsive_key.generation;
        let ack_thread = thread::spawn(move || {
            let value = tauri::async_runtime::block_on(pane_ack(
                ack_handle,
                ack_id,
                ack_generation,
                first_output.seq,
            ));
            ack_sender.send(value).expect("record ack result");
        });

        let responsive_before_cleanup = responsive_receiver
            .recv_timeout(Duration::from_secs(1))
            .ok();
        let ack_before_cleanup = ack_receiver.recv_timeout(Duration::from_secs(1)).ok();

        inputs.close_and_drain();
        let responsive_result = responsive_before_cleanup.clone().or_else(|| {
            responsive_receiver
                .recv_timeout(Duration::from_secs(5))
                .ok()
        });
        let ack_result = ack_before_cleanup
            .clone()
            .or_else(|| ack_receiver.recv_timeout(Duration::from_secs(5)).ok());
        responsive_thread.join().expect("responsive input thread");
        ack_thread.join().expect("ack thread");
        panes.kill(&blocked_key).expect("kill blocked pane");
        panes.kill(&responsive_key).expect("kill responsive pane");
        blocked_output.join().expect("drain blocked pane output");
        drop(app);

        assert_eq!(
            responsive_before_cleanup,
            Some(json!({"ok":true})),
            "one blocked pane consumed the shared blocking pool"
        );
        assert_eq!(
            ack_before_cleanup,
            Some(json!({"ok":true})),
            "output acks shared the blocked PTY pool"
        );
        assert_eq!(responsive_result, Some(json!({"ok":true})));
        assert_eq!(ack_result, Some(json!({"ok":true})));
        assert!(
            blocked_outcomes
                .iter()
                .any(|value| value["error"] == "pane-input-queue-full"),
            "a production-sized blocked burst must hit the bounded pane queue"
        );
    }

    #[cfg(unix)]
    #[test]
    fn node_state_changed_event_is_forwarded_to_the_page_sink() {
        use std::io::Write;
        use std::os::unix::net::UnixStream;

        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        node_stream.flush().expect("flush bridge handle");
        let (events, received) = mpsc::channel();
        let sink: PageEventSink = Arc::new(move |name, body| {
            events
                .send((name.to_string(), body))
                .expect("record page event");
        });
        let mut builder = BridgeBuilder::new(1024);
        register_page_events(&mut builder, sink);
        let connected = builder
            .connect(
                rust_stream.try_clone().expect("clone bridge socket"),
                rust_stream,
            )
            .expect("connect bridge");

        node_stream
            .write_all(
                b"{\"v\":1,\"id\":\"n-state\",\"kind\":\"evt\",\"op\":\"state.changed\",\"body\":{\"reason\":\"pane.open\"}}\n",
            )
            .expect("write state event");
        node_stream.flush().expect("flush state event");

        assert_eq!(
            received
                .recv_timeout(Duration::from_secs(1))
                .expect("page event"),
            ("state-changed".to_string(), json!({"reason":"pane.open"}))
        );
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }

    #[cfg(unix)]
    #[test]
    fn gui_shutdown_drains_an_admitted_launch_before_reaping_panes() {
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        use std::sync::Barrier;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let release = Arc::new(Barrier::new(2));
        let handler_release = Arc::clone(&release);
        let handler_panes = Arc::clone(&panes);
        let (admitted_sender, admitted_receiver) = mpsc::channel();
        let mut builder = BridgeBuilder::new(1024);
        builder.on_launch("pane.open", move |_bridge, _body| {
            admitted_sender.send(()).expect("announce admitted launch");
            handler_release.wait();
            let key = PaneKey::new("late-gui-pane", 1);
            let _reader = handler_panes
                .open_at(
                    key,
                    Path::new("/tmp"),
                    &[
                        "/bin/sh".to_string(),
                        "-c".to_string(),
                        "sleep 30".to_string(),
                    ],
                    &HashMap::new(),
                    PtySize {
                        rows: 24,
                        cols: 80,
                        pixel_width: 0,
                        pixel_height: 0,
                    },
                )
                .map_err(|error| error.to_string())?;
            Ok(json!({"ok":true}))
        });

        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        node_stream.flush().expect("flush bridge handle");
        let connected = builder
            .connect(
                rust_stream.try_clone().expect("clone bridge socket"),
                rust_stream,
            )
            .expect("connect bridge");
        let runtime = Arc::new(runtime(
            Arc::clone(&panes),
            inputs,
            None,
            Some(connected.bridge),
        ));
        node_stream
            .write_all(
                b"{\"v\":1,\"id\":\"n-open\",\"kind\":\"req\",\"op\":\"pane.open\",\"body\":{}}\n",
            )
            .expect("write pane.open");
        node_stream.flush().expect("flush pane.open");
        admitted_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("launch admitted");
        drop(node_stream);

        let shutdown_runtime = Arc::clone(&runtime);
        let (shutdown_sender, shutdown_receiver) = mpsc::channel();
        let shutdown = thread::spawn(move || {
            shutdown_runtime.shutdown();
            shutdown_sender.send(()).expect("announce shutdown");
        });
        let returned_before_launch = shutdown_receiver
            .recv_timeout(Duration::from_millis(100))
            .is_ok();
        release.wait();
        if !returned_before_launch {
            shutdown_receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("shutdown after launch");
        }
        shutdown.join().expect("shutdown thread");

        assert!(
            !returned_before_launch,
            "GUI shutdown returned before its admitted launch finished"
        );
        assert!(panes.list().expect("pane list after shutdown").is_empty());
    }

    /// Whether a pid is still there — signal 0 delivers nothing and only asks.
    #[cfg(unix)]
    fn process_exists(pid: i32) -> bool {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }

        // SAFETY: signal 0 does not deliver a signal; it only checks whether
        // the process exists and is signalable by this process.
        let result = unsafe { kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(1)
    }

    /// The case the editor-absent test above could not reach: a REAL editor
    /// child, its own pipes carrying the bridge, and a `pane.open` admitted
    /// and still spawning when the app is told to quit.
    ///
    /// This is the ordering proof. `Bridge::admit_handler` refuses every new
    /// handler once the transport is `closed`, and only EOF from the peer
    /// closes it — so ending the editor (its input closed, and the kill for
    /// one that does not stop on that) IS the act that shuts admission, and
    /// nothing else in `shutdown()` can do it. What follows is a drain of what
    /// was ALREADY admitted, and only then the reap, so a pane whose spawn
    /// was in flight is in the table before anything reaps it.
    #[cfg(unix)]
    #[test]
    fn gui_shutdown_kills_a_present_editor_then_drains_its_admitted_launch() {
        use std::sync::Barrier;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let release = Arc::new(Barrier::new(2));
        let handler_release = Arc::clone(&release);
        let handler_panes = Arc::clone(&panes);
        let (admitted_sender, admitted_receiver) = mpsc::channel();
        let mut builder = BridgeBuilder::new(MAX_FRAME_BYTES);
        builder.on_launch("pane.open", move |_bridge, _body| {
            admitted_sender.send(()).expect("announce admitted launch");
            handler_release.wait();
            let key = PaneKey::new("editor-present-pane", 1);
            let _reader = handler_panes
                .open_at(
                    key,
                    Path::new("/tmp"),
                    &[
                        "/bin/sh".to_string(),
                        "-c".to_string(),
                        "sleep 30".to_string(),
                    ],
                    &HashMap::new(),
                    PtySize {
                        rows: 24,
                        cols: 80,
                        pixel_width: 0,
                        pixel_height: 0,
                    },
                )
                .map_err(|error| error.to_string())?;
            Ok(json!({"ok":true}))
        });

        // Stands in for `cf ui --json`: the handshake line, one launch request,
        // then a process that holds both pipes open until something kills it.
        let mut editor = Command::new("/bin/sh")
            .arg("-c")
            .arg(concat!(
                r#"printf '%s\n' '{"url":"http://localhost:1/","token":"test"}'; "#,
                r#"printf '%s\n' '{"v":1,"id":"n-open","kind":"req","op":"pane.open","body":{}}'; "#,
                "exec sleep 60",
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn the stand-in editor");
        let editor_pid = editor.id() as i32;
        let reader = editor.stdout.take().expect("editor stdout");
        let writer = editor.stdin.take().expect("editor stdin");
        let connected = builder.connect(reader, writer).expect("connect bridge");

        let runtime = Arc::new(runtime(
            Arc::clone(&panes),
            inputs,
            Some(editor),
            Some(connected.bridge),
        ));
        admitted_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("launch admitted over the real editor pipe");

        let shutdown_runtime = Arc::clone(&runtime);
        let (shutdown_sender, shutdown_receiver) = mpsc::channel();
        let shutdown = thread::spawn(move || {
            shutdown_runtime.shutdown();
            shutdown_sender.send(()).expect("announce shutdown");
        });
        let returned_before_launch = shutdown_receiver
            .recv_timeout(Duration::from_millis(250))
            .is_ok();
        release.wait();
        if !returned_before_launch {
            shutdown_receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("shutdown returns once the admitted launch has finished");
        }
        shutdown.join().expect("shutdown thread");

        assert!(
            !returned_before_launch,
            "shutdown returned before its admitted launch finished"
        );
        assert!(panes.list().expect("pane list after shutdown").is_empty());
        assert!(
            !process_exists(editor_pid),
            "the editor child outlived shutdown"
        );
    }

    /// The daemon is asked before it is killed: one that stops when its
    /// input ends gets to write its last lines, and one that ignores it is
    /// killed once the grace is over.
    #[cfg(unix)]
    #[test]
    fn gui_shutdown_asks_the_editor_first_and_kills_only_what_stays() {
        let mark = std::env::temp_dir().join(format!("consensflow-stop-{}", std::process::id()));
        let _ = std::fs::remove_file(&mark);
        let runtime_for = |script: String| {
            use std::io::{BufRead, BufReader};
            let mut editor = Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("spawn the stand-in editor");
            let pid = editor.id() as i32;
            let mut ready = String::new();
            BufReader::new(editor.stdout.take().expect("editor stdout"))
                .read_line(&mut ready)
                .expect("the stand-in says it is ready");
            assert_eq!(ready, "ready\n");
            let arbiter = Arc::new(InputArbiter::new(0));
            let panes = Arc::new(PaneTable::new());
            let runtime = runtime(
                Arc::clone(&panes),
                Arc::new(InputQueue::new(panes, arbiter)),
                Some(editor),
                None,
            );
            (runtime, pid)
        };

        let (polite, polite_pid) = runtime_for(format!(
            "echo ready; cat >/dev/null; echo asked > {}; exit 0",
            mark.display()
        ));
        let started = Instant::now();
        polite.shutdown();
        assert!(started.elapsed() < EDITOR_STOP_GRACE, "it went on its own");
        assert_eq!(
            std::fs::read_to_string(&mark).expect("the editor wrote its last line"),
            "asked\n"
        );
        assert!(!process_exists(polite_pid));
        let _ = std::fs::remove_file(&mark);

        let (deaf, deaf_pid) = runtime_for("echo ready; exec sleep 60".to_string());
        let started = Instant::now();
        deaf.shutdown();
        assert!(started.elapsed() >= EDITOR_STOP_GRACE, "it had its grace");
        assert!(!process_exists(deaf_pid), "and was killed after it");
    }

    /// Why the editor is killed FIRST, stated as a test rather than a comment.
    ///
    /// `wait_launches_closed` waits for `closed` AND an empty launch count,
    /// and only the peer's EOF sets `closed`. Draining before the kill would
    /// therefore wait on a peer that is still writing — every app exit would
    /// hang. This is the shape a reordered `shutdown()` would take.
    #[cfg(unix)]
    #[test]
    fn draining_launches_before_the_editor_closes_never_returns() {
        use std::io::Write;
        use std::os::unix::net::UnixStream;

        let mut builder = BridgeBuilder::new(1024);
        builder.on("noop", |_bridge, _body| Ok(json!({"ok":true})));
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        node_stream.flush().expect("flush bridge handle");
        let connected = builder
            .connect(
                rust_stream.try_clone().expect("clone bridge socket"),
                rust_stream,
            )
            .expect("connect bridge");

        let waiting = connected.bridge.clone();
        let (done_sender, done_receiver) = mpsc::channel();
        let wait = thread::spawn(move || {
            let outcome = waiting.wait_launches_closed();
            let _ = done_sender.send(outcome.is_ok());
        });
        assert!(
            done_receiver
                .recv_timeout(Duration::from_millis(300))
                .is_err(),
            "wait_launches_closed returned while the editor peer was still open"
        );

        drop(node_stream);
        assert!(
            done_receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("wait_launches_closed returns once the peer closes"),
            "wait_launches_closed failed after the peer closed"
        );
        wait.join().expect("wait thread");
    }

    /// Over the bridge as the daemon speaks it: text the human typed and never
    /// sent holds neither a paste nor a native send (the owner's choice,
    /// 2026-10-01), and the snapshot has no draft to wait on.
    #[cfg(unix)]
    #[test]
    fn unsent_typing_holds_no_paste_and_no_claim() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let mut builder = BridgeBuilder::new(1024 * 1024);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            arbiter,
            Arc::new(OutputHub::new()),
            Arc::new(LaunchRegistry::new()),
            Arc::clone(&inputs),
        );
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        let connected = builder
            .connect(rust_stream.try_clone().expect("clone socket"), rust_stream)
            .expect("connect bridge");
        let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
        let mut number = 0;
        let mut ask = |op: &str, body: Value| -> Value {
            number += 1;
            let id = format!("n-typing-{number}");
            let mut frame =
                serde_json::to_vec(&json!({"v":1,"id":id,"kind":"req","op":op,"body":body}))
                    .expect("serialize request");
            frame.push(b'\n');
            node_stream.write_all(&frame).expect("write request");
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read response");
                let frame: Value = serde_json::from_str(line.trim()).expect("response JSON");
                if frame["kind"] == "res" && frame["id"] == id.as_str() {
                    return frame["body"].clone();
                }
            }
        };
        let opened = ask(
            "pane.open",
            json!({"id":"typing-pane","generation":1,"cwd":"/tmp","argv":["/bin/sh","-c","sleep 30"],
                   "env":{},"size":{"rows":24,"cols":80},"backlogBytes":1024}),
        );
        assert_eq!(opened["ok"], true, "{opened}");
        let typed = ask(
            "pane.input",
            json!({"id":"typing-pane","generation":1,"bytes":b"half a thought".to_vec()}),
        );
        assert_eq!(typed, json!({"ok":true}));
        assert_eq!(
            ask("pane.claim", json!({"pane":"typing-pane","generation":1})),
            json!({"ok":true})
        );
        assert_eq!(
            ask(
                "pane.write_paste",
                json!({"id":"typing-pane","generation":1,"body":"result"})
            ),
            json!({"ok":true})
        );
        let snapshot = ask("pane.snapshot", json!({"id":"typing-pane","generation":1}));
        assert_eq!(snapshot["pasteInFlight"], false, "{snapshot}");
        assert!(snapshot.get("draftLatched").is_none(), "{snapshot}");

        panes
            .kill(&PaneKey::new("typing-pane", 1))
            .expect("kill the pane");
        inputs.close_and_drain();
        drop(reader);
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }

    /// A pane's input lives as long as the pane: killed, or ended on its own,
    /// it takes its input worker and queue, its page sequence and its arbiter
    /// state with it. Each used to stay until the app quit, a parked thread
    /// per window.
    #[cfg(unix)]
    #[test]
    fn a_pane_gone_from_the_table_takes_its_input_with_it() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let mut builder = BridgeBuilder::new(1024 * 1024);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            Arc::clone(&arbiter),
            Arc::new(OutputHub::new()),
            Arc::new(LaunchRegistry::new()),
            Arc::clone(&inputs),
        );
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        let connected = builder
            .connect(rust_stream.try_clone().expect("clone socket"), rust_stream)
            .expect("connect bridge");
        let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
        let mut number = 0;
        let mut ask = |op: &str, body: Value| -> Value {
            number += 1;
            let id = format!("n-retired-{number}");
            let mut frame =
                serde_json::to_vec(&json!({"v":1,"id":id,"kind":"req","op":op,"body":body}))
                    .expect("serialize request");
            frame.push(b'\n');
            node_stream.write_all(&frame).expect("write request");
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read response");
                let frame: Value = serde_json::from_str(line.trim()).expect("response JSON");
                if frame["kind"] == "res" && frame["id"] == id.as_str() {
                    return frame["body"].clone();
                }
            }
        };
        let gone = |key: &PaneKey| {
            !inputs.senders.lock().unwrap().contains_key(key)
                && !inputs.workers.lock().unwrap().contains_key(key)
                && !inputs.page.lock().unwrap().last_sequences.contains_key(key)
                && arbiter.snapshot(key).is_err()
        };

        let killed = PaneKey::new("killed-pane", 1);
        let ended = PaneKey::new("ended-pane", 1);
        for (key, script) in [(&killed, "sleep 30"), (&ended, "read line")] {
            let opened = ask(
                "pane.open",
                json!({"id":key.id,"generation":1,"cwd":"/tmp","argv":["/bin/sh","-c",script],
                       "env":{},"size":{"rows":24,"cols":80},"backlogBytes":1024}),
            );
            assert_eq!(opened["ok"], true, "{opened}");
            inputs
                .enqueue_page(key.clone(), 1, InputWork::Write(b"x".to_vec()), true)
                .expect("the page types into the pane");
            assert!(!gone(key), "the pane has its input");
        }

        assert_eq!(
            ask("pane.kill", json!({"id":killed.id,"generation":1})),
            json!({"ok":true})
        );
        assert!(gone(&killed), "a killed pane kept its input");

        assert_eq!(
            ask("pane.input", json!({"id":ended.id,"generation":1,"bytes":[13]})),
            json!({"ok":true})
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gone(&ended) {
            assert!(
                Instant::now() < deadline,
                "a pane that ended kept its input"
            );
            thread::sleep(Duration::from_millis(20));
        }

        inputs.close_and_drain();
        drop(reader);
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }

    /// Input for a pane the table never held is refused, and starts no
    /// worker: every key used to get a thread and a queue of its own.
    #[test]
    fn input_for_a_pane_never_opened_is_refused() {
        let panes = Arc::new(PaneTable::new());
        let inputs = InputQueue::new(Arc::clone(&panes), Arc::new(InputArbiter::new(0)));
        let refused = inputs.write(PaneKey::new("never-opened", 1), b"x".to_vec());
        assert_eq!(refused.err().as_deref(), Some("stale pane generation"));
        assert!(inputs.senders.lock().unwrap().is_empty());
        assert!(inputs.workers.lock().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn simultaneous_duplicate_launches_wait_for_and_share_one_result() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let launches = Arc::new(LaunchRegistry::new());
        let mut builder = BridgeBuilder::new(1024 * 1024);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            arbiter,
            Arc::new(OutputHub::new()),
            Arc::clone(&launches),
            Arc::clone(&inputs),
        );
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        node_stream.flush().expect("flush bridge handle");
        let connected = builder
            .connect(
                rust_stream.try_clone().expect("clone bridge socket"),
                rust_stream,
            )
            .expect("connect bridge");

        let body = json!({
            "id":"dedupe-pane",
            "generation":1,
            "launchId":"launch-shared",
            "cwd":"/tmp",
            "argv":["/bin/sh","-c","sleep 30"],
            "env":{},
            "size":{"rows":24,"cols":80},
            "backlogBytes":1024,
        });
        let mut burst = Vec::new();
        for index in 0..8 {
            serde_json::to_writer(
                &mut burst,
                &json!({
                    "v":1,
                    "id":format!("n-dedupe-{index}"),
                    "kind":"req",
                    "op":"pane.open",
                    "body":body,
                }),
            )
            .expect("serialize duplicate request");
            burst.push(b'\n');
        }
        node_stream
            .write_all(&burst)
            .expect("write duplicate burst");
        node_stream.flush().expect("flush duplicate burst");

        let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
        let mut responses = Vec::new();
        while responses.len() < 8 {
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .expect("read duplicate response");
            let frame: Value = serde_json::from_str(line.trim()).expect("response JSON");
            if frame["kind"] == "res" && frame["op"] == "pane.open" {
                responses.push(frame["body"].clone());
            }
        }
        assert!(
            responses.iter().all(|response| {
                response["ok"] == true
                    && response["id"] == "dedupe-pane"
                    && response["generation"] == 1
            }),
            "duplicates did not share the successful launch: {responses:?}"
        );
        assert_eq!(panes.list().expect("one launched pane").len(), 1);

        panes
            .kill(&PaneKey::new("dedupe-pane", 1))
            .expect("kill launched pane");
        inputs.close_and_drain();
        drop(reader);
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }

    #[cfg(unix)]
    #[test]
    fn tauri_commands_send_contract_operations_and_bodies_over_the_live_bridge() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let routes = vec![
            (
                "core_request",
                json!({"operation":"board.get","body":{"project":1}}),
                "board.get",
                json!({"project":1}),
            ),
            (
                "core_request",
                json!({"operation":"task.cancel","body":{"project":1,"task":1}}),
                "task.cancel",
                json!({"project":1,"task":1}),
            ),
        ];

        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        node_stream.flush().expect("flush bridge handle");
        let connected = BridgeBuilder::new(1024 * 1024)
            .connect(
                rust_stream.try_clone().expect("clone bridge socket"),
                rust_stream,
            )
            .expect("connect bridge");

        let expected = routes
            .iter()
            .map(|(_, _, operation, body)| ((*operation).to_string(), body.clone()))
            .collect::<Vec<_>>();
        let node = thread::spawn(move || {
            let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
            for (operation, body) in expected {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read command request");
                let frame: Value = serde_json::from_str(line.trim()).expect("command frame JSON");
                assert_eq!(frame["kind"], "req");
                assert_eq!(frame["op"], operation);
                assert_eq!(frame["body"], body);
                serde_json::to_writer(
                    &mut node_stream,
                    &json!({
                        "v":1,
                        "id":frame["id"],
                        "kind":"res",
                        "op":operation,
                        "body":{"ok":true},
                    }),
                )
                .expect("write command response");
                node_stream.write_all(b"\n").expect("terminate response");
                node_stream.flush().expect("flush command response");
            }
        });

        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(0));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let runtime = runtime(panes, inputs, None, Some(connected.bridge));
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![core_request])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");

        // These must be refused before requesting the bridge: the peer expects only valid routes.
        for (command, args) in [
            ("core_request", json!({"operation":"state.list","body":{}})),
            ("core_request", json!({"operation":"board.get","body":[1]})),
        ] {
            let response = tauri::test::get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: command.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "tauri://localhost".parse().expect("invoke URL"),
                    body: tauri::ipc::InvokeBody::Json(args),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            )
            .expect("validation response")
            .deserialize::<Value>()
            .expect("JSON");
            assert_eq!(
                response["ok"], false,
                "invalid {command} accepted: {response}"
            );
        }
        for (command, args, _, _) in routes {
            let response = tauri::test::get_ipc_response(
                &webview,
                tauri::webview::InvokeRequest {
                    cmd: command.into(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: "tauri://localhost".parse().expect("invoke URL"),
                    body: tauri::ipc::InvokeBody::Json(args),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            )
            .expect("command succeeds")
            .deserialize::<Value>()
            .expect("command response JSON");
            assert_eq!(response, json!({"ok":true}), "{command}");
        }

        node.join().expect("Node peer");
        drop(webview);
        drop(app);
    }
}
