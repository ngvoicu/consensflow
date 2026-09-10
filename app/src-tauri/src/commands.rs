use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use futures_channel::oneshot;
use portable_pty::PtySize;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::arbiter::{ArbiterError, InputArbiter, PaneEvent};
use crate::bridge::{Bridge, BridgeBuilder, ConnectedBridge};
use crate::pty::{
    validate_drop_env, PaneEnvironment, PaneKey, PaneOutput, PaneTable, StreamedPane,
};

const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// The page-side name of Node's `state.changed`. No dot: Tauri rejects it.
const PAGE_STATE_EVENT: &str = "state-changed";
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
/// hub parks what follows until a new one arrives.
type OutputSink = Arc<dyn Fn(PaneOutputMessage) -> bool + Send + Sync>;

struct OutputHub {
    state: Mutex<OutputHubState>,
}

enum InputWork {
    Human(Vec<u8>),
    Reply(Vec<u8>),
    Paste { epoch: u64, body: Vec<u8> },
    ClaimEpoch { epoch: u64, native_editor: bool },
}

impl InputWork {
    fn byte_count(&self) -> usize {
        match self {
            Self::Human(bytes) | Self::Reply(bytes) => bytes.len(),
            Self::Paste { body, .. } => body.len().saturating_add(13),
            Self::ClaimEpoch { .. } => 0,
        }
    }
}

enum InputSuccess {
    Human { epoch: u64 },
    Written,
}

type InputResponse = Result<InputSuccess, String>;

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

#[derive(Clone)]
struct InputRoute {
    sender: mpsc::SyncSender<InputJob>,
    pending_bytes: Arc<AtomicUsize>,
}

struct InputQueue {
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    senders: Mutex<HashMap<PaneKey, InputRoute>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
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
            workers: Mutex::new(Vec::new()),
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
            InputWork::Human(bytes) | InputWork::Reply(bytes) => validate_input(bytes)?,
            InputWork::Paste { body, .. } => validate_input(body)?,
            InputWork::ClaimEpoch { .. } => {}
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

    fn take_page_completion(&self, ticket: &str) -> Result<PageInputCompletion, String> {
        self.page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?
            .completions
            .remove(ticket)
            .ok_or_else(|| "pane-input-ticket-not-found".to_string())
    }

    fn human(
        &self,
        key: PaneKey,
        bytes: Vec<u8>,
    ) -> Result<oneshot::Receiver<InputResponse>, String> {
        self.submit(key, InputWork::Human(bytes))
    }

    fn reply(
        &self,
        key: PaneKey,
        bytes: Vec<u8>,
    ) -> Result<oneshot::Receiver<InputResponse>, String> {
        self.submit(key, InputWork::Reply(bytes))
    }

    fn paste(
        &self,
        key: PaneKey,
        epoch: u64,
        body: Vec<u8>,
    ) -> Result<oneshot::Receiver<InputResponse>, String> {
        self.submit(key, InputWork::Paste { epoch, body })
    }

    fn claim_epoch(
        &self,
        key: PaneKey,
        epoch: u64,
        native_editor: bool,
    ) -> Result<oneshot::Receiver<InputResponse>, String> {
        self.submit(
            key,
            InputWork::ClaimEpoch {
                epoch,
                native_editor,
            },
        )
    }

    fn submit(
        &self,
        key: PaneKey,
        work: InputWork,
    ) -> Result<oneshot::Receiver<InputResponse>, String> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err("pane input admission is closed".to_string());
        }
        let mut senders = self
            .senders
            .lock()
            .map_err(|_| "pane input queue lock is poisoned".to_string())?;
        if !self.accepting.load(Ordering::Acquire) {
            return Err("pane input admission is closed".to_string());
        }
        let route = match senders.get(&key) {
            Some(route) => route.clone(),
            None => {
                let (sender, jobs) = mpsc::sync_channel(INPUT_QUEUE_CAPACITY);
                let pending_bytes = Arc::new(AtomicUsize::new(0));
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
                    .push(worker);
                let route = InputRoute {
                    sender,
                    pending_bytes,
                };
                senders.insert(key.clone(), route.clone());
                route
            }
        };
        let reserved_bytes = work.byte_count();
        route
            .pending_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
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
        for worker in workers {
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
            InputWork::Human(bytes) => arbiter
                .write_human(&panes, &key, &bytes)
                .map(|epoch| InputSuccess::Human { epoch })
                .map_err(|error| error.to_string()),
            InputWork::Reply(bytes) => arbiter
                .write_reply(&panes, &key, &bytes)
                .map(|()| InputSuccess::Written)
                .map_err(|error| error.to_string()),
            InputWork::Paste { epoch, body } => arbiter
                .write_paste(&panes, &key, epoch, &body)
                .map(|()| InputSuccess::Written)
                .map_err(|error| error.to_string()),
            InputWork::ClaimEpoch {
                epoch,
                native_editor,
            } => {
                let claimed = if native_editor {
                    arbiter.claim_native_epoch(&key, epoch)
                } else {
                    arbiter.claim_epoch(&key, epoch)
                };
                claimed
                    .map(|()| InputSuccess::Written)
                    .map_err(|error| match error {
                        ArbiterError::Stale if native_editor => "stale-input-epoch".to_string(),
                        error => error.to_string(),
                    })
            }
        };
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

pub struct AppRuntime {
    panes: Arc<PaneTable>,
    bridge: Option<Bridge>,
    editor: Mutex<Option<Child>>,
    roster: Option<RosterHandle>,
    startup_error: Option<String>,
    output: Arc<OutputHub>,
    inputs: Arc<InputQueue>,
    launches: Arc<LaunchRegistry>,
    shutting_down: AtomicBool,
}

impl AppRuntime {
    pub(crate) fn pane_table(&self) -> Arc<PaneTable> {
        Arc::clone(&self.panes)
    }

    pub fn start(app: &AppHandle) -> Self {
        let panes = Arc::new(PaneTable::new());
        let output = Arc::new(OutputHub::new());
        let launches = Arc::new(LaunchRegistry::new());
        let (event_sender, event_receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(ENTER_DELAY_MS, event_sender));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));

        let started = start_editor(
            app,
            Arc::clone(&panes),
            Arc::clone(&arbiter),
            Arc::clone(&output),
            Arc::clone(&launches),
            Arc::clone(&inputs),
        );

        match started {
            Ok((editor, connected)) => {
                let roster = match RosterHandle::from_value(connected.handle) {
                    Ok(roster) => roster,
                    Err(error) => {
                        let mut editor = editor;
                        let _ = editor.kill();
                        return Self::unavailable(panes, output, inputs, launches, error);
                    }
                };
                forward_input_events(event_receiver, connected.bridge.clone());
                Self {
                    panes,
                    bridge: Some(connected.bridge),
                    editor: Mutex::new(Some(editor)),
                    roster: Some(roster),
                    startup_error: None,
                    output,
                    inputs,
                    launches,
                    shutting_down: AtomicBool::new(false),
                }
            }
            Err(error) => Self::unavailable(panes, output, inputs, launches, error),
        }
    }

    fn unavailable(
        panes: Arc<PaneTable>,
        output: Arc<OutputHub>,
        inputs: Arc<InputQueue>,
        launches: Arc<LaunchRegistry>,
        error: String,
    ) -> Self {
        eprintln!("consensflow: {error}");
        Self {
            panes,
            bridge: None,
            editor: Mutex::new(None),
            roster: None,
            startup_error: Some(error),
            output,
            inputs,
            launches,
            shutting_down: AtomicBool::new(false),
        }
    }

    pub fn shutdown(&self) {
        if self.begin_shutdown() {
            self.finish_shutdown();
        }
    }

    pub(crate) fn begin_shutdown(&self) -> bool {
        if self.shutting_down.swap(true, Ordering::AcqRel) {
            return false;
        }
        if let Some(mut editor) = self
            .editor
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = editor.kill();
            let _ = editor.wait();
        }
        true
    }

    pub(crate) fn finish_shutdown(&self) {
        if let Some(bridge) = &self.bridge {
            let _ = bridge.wait_launches_closed();
        }
        reap_all(&self.panes);
        self.inputs.close_and_drain();
        if let Some(bridge) = &self.bridge {
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

fn request_node(
    bridge: Option<Bridge>,
    startup_error: Option<String>,
    operation: String,
    body: Value,
) -> Value {
    let Some(bridge) = bridge else {
        return not_available(
            &operation,
            startup_error
                .as_deref()
                .unwrap_or("the Node bridge is not running"),
        );
    };
    match bridge.request(operation.clone(), body, None) {
        Ok(response) => normalize_node_response(&operation, response),
        Err(error) => json!({"ok":false,"error":error.to_string(),"operation":operation}),
    }
}

fn compose_state(
    node: Value,
    roster: Option<RosterHandle>,
    startup_error: Option<String>,
) -> Value {
    let mut object = match node {
        Value::Object(object) => object,
        other => Map::from_iter([
            ("ok".to_string(), Value::Bool(true)),
            ("state".to_string(), other),
        ]),
    };
    if let Some(roster) = roster {
        object.insert(
            "roster".to_string(),
            serde_json::to_value(roster).unwrap_or(Value::Null),
        );
    }
    if object.get("error").and_then(Value::as_str) == Some("not-available-yet") {
        object.insert("available".to_string(), Value::Bool(false));
    } else {
        object
            .entry("available".to_string())
            .or_insert(Value::Bool(true));
    }
    if let Some(error) = startup_error {
        object.insert("startupError".to_string(), Value::String(error));
    }
    Value::Object(object)
}

impl Drop for AppRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
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
    epoch: u64,
    body: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PeerSendRequest {
    id: String,
    generation: u64,
    epoch: u64,
    socket: PathBuf,
    peer_pid: i32,
    body: String,
    timeout_ms: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClaimEpochRequest {
    pane: String,
    generation: u64,
    epoch: u64,
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

fn start_editor(
    app: &AppHandle,
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    output: Arc<OutputHub>,
    launches: Arc<LaunchRegistry>,
    inputs: Arc<InputQueue>,
) -> Result<(Child, ConnectedBridge), String> {
    let (node, cli) = bundled_cli(app)?;
    let mut command = Command::new(node);
    command.arg(cli).args(["ui", "--json", "--no-open"]);
    if let Some(path) = login_path() {
        command.env("PATH", path);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("the bundled ConsensFlow could not be started: {error}"))?;
    let input = child
        .stdout
        .take()
        .ok_or_else(|| "the editor process gave no output to read".to_string())?;
    let writer = child
        .stdin
        .take()
        .ok_or_else(|| "the editor process gave no input pipe".to_string())?;

    let mut builder = BridgeBuilder::new(MAX_FRAME_BYTES);
    let page_app = app.clone();
    let page_events: PageEventSink = Arc::new(move |name, body| {
        if let Err(error) = page_app.emit(name, body) {
            eprintln!("consensflow page event {name}: {error}");
        }
    });
    register_page_events(&mut builder, page_events);
    register_pane_handlers(&mut builder, panes, arbiter, output, launches, inputs);
    builder.on_error(|error| eprintln!("consensflow bridge: {error}"));
    match builder.connect(input, writer) {
        Ok(connected) => Ok((child, connected)),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!(
                "could not connect to the bundled ConsensFlow: {error}"
            ))
        }
    }
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
    let resource_node = resources.join("binaries/node");
    let node = if resource_node.exists() {
        resource_node
    } else {
        std::env::current_exe()
            .map_err(|error| format!("the app could not find itself: {error}"))?
            .parent()
            .ok_or_else(|| "the app executable has no directory".to_string())?
            .join("node")
    };
    let cli = resources.join("cli/bin/cf.mjs");
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

fn login_path() -> Option<String> {
    let shell = std::env::var("SHELL").ok()?;
    let output = Command::new(shell)
        .args(["-lc", "printf %s \"$PATH\""])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
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
        if let Err(error) = open_arbiter.register(&streamed.key) {
            let _ = open_panes.kill(&streamed.key);
            let error = error.to_string();
            if let Some(slot) = owner_slot {
                slot.complete(Err(error.clone()));
            }
            return Err(error);
        }
        let key = streamed.key.clone();
        stream_to_page(
            streamed,
            bridge,
            Arc::clone(&open_output),
            Arc::clone(&open_launches),
            request.launch_id,
        );
        if let Some(slot) = owner_slot {
            slot.complete(Ok(key.clone()));
        }
        Ok(json!({"ok":true,"id":key.id,"generation":key.generation}))
    });

    let input_queue = Arc::clone(&inputs);
    builder.on("pane.input", move |_bridge, body| {
        let request: BytesRequest = parse_body(body)?;
        validate_input(&request.bytes)?;
        let key = pane_key(&request.id, request.generation)?;
        match wait_for_input_blocking(input_queue.human(key, request.bytes)?)? {
            InputSuccess::Human { epoch } => Ok(json!({"ok":true,"epoch":epoch})),
            InputSuccess::Written => Err("pane.input returned the wrong outcome".to_string()),
        }
    });

    let reply_queue = Arc::clone(&inputs);
    builder.on("pane.reply", move |_bridge, body| {
        let request: BytesRequest = parse_body(body)?;
        validate_input(&request.bytes)?;
        let key = pane_key(&request.id, request.generation)?;
        match wait_for_input_blocking(reply_queue.reply(key, request.bytes)?)? {
            InputSuccess::Written => Ok(json!({"ok":true})),
            InputSuccess::Human { .. } => Err("pane.reply returned the wrong outcome".to_string()),
        }
    });

    let paste_queue = Arc::clone(&inputs);
    builder.on("pane.write_paste", move |_bridge, body| {
        let request: PasteRequest = parse_body(body)?;
        let key = pane_key(&request.id, request.generation)?;
        match wait_for_input_blocking(paste_queue.paste(
            key,
            request.epoch,
            request.body.into_bytes(),
        )?)? {
            InputSuccess::Written => Ok(json!({"ok":true})),
            InputSuccess::Human { .. } => {
                Err("pane.write_paste returned the wrong outcome".to_string())
            }
        }
    });

    for (operation, native_editor) in [
        ("pane.claim_epoch", false),
        ("pane.claim_native_epoch", true),
    ] {
        let claim_queue = Arc::clone(&inputs);
        builder.on(operation, move |_bridge, body| {
            let request: ClaimEpochRequest = parse_body(body)?;
            let key = pane_key(&request.pane, request.generation)?;
            match wait_for_input_blocking(claim_queue.claim_epoch(
                key,
                request.epoch,
                native_editor,
            )?)? {
                InputSuccess::Written => Ok(json!({"ok":true})),
                InputSuccess::Human { .. } => {
                    Err(format!("{operation} returned the wrong outcome"))
                }
            }
        });
    }

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
            let result = peer_panes.send_peer(&key, &request.socket, request.peer_pid,
                request.body.as_bytes(), std::time::Duration::from_millis(request.timeout_ms), || {
                    wait_for_input_blocking(peer_queue.claim_epoch(key.clone(), request.epoch, true)?)
                        .map(|_| ())
                });
            Ok(match result {
                Ok(()) => json!({"ok":true}),
                Err(error) if error.uncertain => json!({"ok":false,"admitted":null,"error":"uncertain","cause":error.reason}),
                Err(error) => json!({"ok":false,"admitted":false,"bytesWritten":0,"error":error.code,"cause":error.reason}),
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (&peer_panes, &peer_queue, key, request.epoch);
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
    let kill_launches = Arc::clone(&launches);
    builder.on("pane.kill", move |_bridge, body| {
        let request: PaneRequest = parse_body(body)?;
        let key = pane_key(&request.id, request.generation)?;
        kill_panes.kill(&key).map_err(|error| error.to_string())?;
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
            "inputEpoch":snapshot.input_epoch,
            "draftLatched":snapshot.draft_latched,
            "pasteInFlight":snapshot.paste_in_flight,
            "inputFailed":snapshot.input_failed,
            "queuedHumanBytes":snapshot.queued_human_bytes,
            "lastSubmissionId":snapshot.last_submission_id,
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

fn stream_to_page(
    streamed: StreamedPane,
    bridge: Bridge,
    output: Arc<OutputHub>,
    launches: Arc<LaunchRegistry>,
    launch_id: Option<String>,
) {
    thread::spawn(move || {
        let key = streamed.key;
        for message in streamed.output {
            output.publish(message.into());
        }
        if let Some(launch_id) = launch_id {
            launches.remove_id(&launch_id);
        }
        let _ = bridge.event(
            "pane.exit",
            json!({"id":key.id,"generation":key.generation}),
        );
    });
}

/// The headless pane helper, running the WINDOW's handlers.
///
/// `consensflow-bridge` used to carry its own copy of the pane operations, and
/// a copy is a contract that drifts: it had no launch deduplication, no pane
/// id or generation on `pane.open`, no `draft.clear`, it threw the arbiter's
/// event receiver away so no `pane.enter` was ever sent, and it never reported
/// a natural `pane.exit`. The real Node side speaks to the window, so against
/// the helper it could only be refused. There is nothing to keep in step here:
/// this is `register_pane_handlers`, the same `InputQueue`, the same
/// `LaunchRegistry`, the same event forwarding and the same shutdown drain the
/// window uses, over stdin and stdout instead of a webview.
///
/// Serves until the peer closes the transport, then reaps what it opened.
pub fn run_headless() -> Result<(), String> {
    let panes = Arc::new(PaneTable::new());
    let output = Arc::new(OutputHub::new());
    let launches = Arc::new(LaunchRegistry::new());
    let (event_sender, event_receiver) = mpsc::channel();
    let arbiter = Arc::new(InputArbiter::new(ENTER_DELAY_MS, event_sender));
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

    // No page to draw into, so a pane's bytes go back over the same bridge.
    // Registered after `serve` on purpose: whatever a pane produced in between
    // is parked in the hub and drains into this sink the moment it attaches.
    let sink = bridge.clone();
    output.register_sink(Arc::new(move |message: PaneOutputMessage| {
        sink.event("pane.output", json!(message)).is_ok()
    }));
    forward_input_events(event_receiver, bridge.clone());

    // The same order the window shuts down in, and for the same reason: the
    // peer's EOF is what closes admission, so the drain can only run after it.
    bridge
        .wait_launches_closed()
        .map_err(|error| error.to_string())?;
    reap_all(&panes);
    inputs.close_and_drain();
    bridge.wait_closed().map_err(|error| error.to_string())
}

fn forward_input_events(events: mpsc::Receiver<PaneEvent>, bridge: Bridge) {
    thread::spawn(move || {
        for event in events {
            match event {
                PaneEvent::Enter { pane, epoch } => {
                    let _ = bridge.event(
                        "pane.enter",
                        json!({"id":pane.id,"generation":pane.generation,"epoch":epoch}),
                    );
                }
            }
        }
    });
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
        Ok(InputSuccess::Human { epoch }) if completion.human => {
            json!({"ok":true,"epoch":epoch})
        }
        Ok(InputSuccess::Written) if !completion.human => json!({"ok":true}),
        Ok(_) => json!({"ok":false,"error":"pane input returned the wrong outcome"}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}

#[tauri::command]
pub async fn open_pm<R: Runtime>(app: AppHandle<R>, tab: String, harness: String) -> Value {
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking("pm.open", move || {
        request_node(
            bridge,
            startup_error,
            "pm.open".into(),
            json!({"tab":tab,"harness":harness}),
        )
    })
    .await
}

#[tauri::command]
pub async fn open_lead<R: Runtime>(app: AppHandle<R>, dir: String, harness: String) -> Value {
    if let Err(error) =
        validate_text(&dir, "directory").and_then(|()| validate_text(&harness, "harness"))
    {
        return json!({"ok":false,"error":error});
    }
    if !Path::new(&dir).is_absolute() {
        return json!({"ok":false,"error":"directory must be absolute"});
    }
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking("tab.open", move || {
        request_node(
            bridge,
            startup_error,
            "tab.open".to_string(),
            json!({"dir":dir,"harness":harness}),
        )
    })
    .await
}

#[tauri::command]
pub async fn open_shell<R: Runtime>(app: AppHandle<R>, tab: String) -> Value {
    if let Err(error) = validate_text(&tab, "tab") {
        return json!({"ok":false,"error":error});
    }
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking("shell.open", move || {
        request_node(
            bridge,
            startup_error,
            "shell.open".to_string(),
            json!({"tab":tab}),
        )
    })
    .await
}

#[tauri::command]
pub async fn open_consult<R: Runtime>(
    app: AppHandle<R>,
    tab: String,
    agent: String,
    task: Option<String>,
    conversation: Option<String>,
) -> Value {
    if let Err(error) = validate_text(&tab, "tab").and_then(|()| validate_text(&agent, "agent")) {
        return json!({"ok":false,"error":error});
    }
    let (operation, body) = match task {
        Some(task) if !task.trim().is_empty() => {
            ("consult", json!({"tab":tab,"agent":agent,"task":task}))
        }
        Some(_) => return json!({"ok":false,"error":"task is required"}),
        None => match conversation {
            Some(conversation) if !conversation.trim().is_empty() => (
                "attach",
                json!({"tab":tab,"agent":agent,"session":conversation}),
            ),
            _ => return json!({"ok":false,"error":"conversation is required when task is absent"}),
        },
    };
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking(operation, move || {
        request_node(bridge, startup_error, operation.to_string(), body)
    })
    .await
}

#[tauri::command]
pub async fn delete_pane<R: Runtime>(app: AppHandle<R>, id: String, generation: u64) -> Value {
    if let Err(error) = pane_key(&id, generation) {
        return json!({"ok":false,"error":error});
    }
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking("pane.delete", move || {
        request_node(
            bridge,
            startup_error,
            "pane.delete".into(),
            json!({"id":id,"generation":generation}),
        )
    })
    .await
}

#[tauri::command]
pub async fn close_pane<R: Runtime>(app: AppHandle<R>, id: String, generation: u64) -> Value {
    let key = match pane_key(&id, generation) {
        Ok(key) => key,
        Err(error) => return json!({"ok":false,"error":error}),
    };
    let (panes, launches, bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (
            Arc::clone(&state.panes),
            Arc::clone(&state.launches),
            state.bridge.clone(),
            state.startup_error.clone(),
        )
    };
    run_blocking("pane.close", move || {
        if let Err(error) = panes.kill(&key) {
            return json!({"ok":false,"error":error.to_string()});
        }
        launches.remove_key(&key);
        let response = request_node(
            bridge,
            startup_error,
            "pane.close".to_string(),
            json!({"id":id,"generation":generation}),
        );
        if response.get("ok") == Some(&Value::Bool(false)) {
            let mut object = response.as_object().cloned().unwrap_or_default();
            object.insert("paneClosed".to_string(), Value::Bool(true));
            Value::Object(object)
        } else {
            response
        }
    })
    .await
}

fn enqueue_page_input<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    sequence: u64,
    bytes: Vec<u8>,
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
    let work = if human {
        InputWork::Human(bytes)
    } else {
        InputWork::Reply(bytes)
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
    enqueue_page_input(app, id, generation, sequence, bytes, true)
}

#[tauri::command]
pub fn pane_reply_enqueue<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    generation: u64,
    sequence: u64,
    bytes: Vec<u8>,
) -> Value {
    enqueue_page_input(app, id, generation, sequence, bytes, false)
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
    let result = input_result(inputs.take_page_completion(&ticket)).await;
    if result.get("epoch").is_some() {
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

#[tauri::command]
pub async fn set_policy<R: Runtime>(
    app: AppHandle<R>,
    scope: String,
    id: String,
    mode: String,
) -> Value {
    let valid = match scope.as_str() {
        "tab" => matches!(mode.as_str(), "auto" | "manual"),
        "pane" => matches!(mode.as_str(), "auto" | "manual" | "inherit"),
        _ => false,
    };
    if !valid || id.trim().is_empty() {
        return json!({"ok":false,"error":"invalid policy scope, id, or mode"});
    }
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking("notify.set", move || {
        request_node(
            bridge,
            startup_error,
            "notify.set".to_string(),
            json!({"scope":scope,"id":id,"mode":mode}),
        )
    })
    .await
}

#[tauri::command]
pub async fn answers_list<R: Runtime>(
    app: AppHandle<R>,
    tab: String,
    pane: String,
    conversation: String,
) -> Value {
    if let Err(error) = validate_text(&tab, "tab")
        .and_then(|()| validate_text(&pane, "pane"))
        .and_then(|()| validate_text(&conversation, "conversation"))
    {
        return json!({"ok":false,"error":error});
    }
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking("answers.list", move || {
        request_node(
            bridge,
            startup_error,
            "answers.list".to_string(),
            json!({"tab":tab,"pane":pane,"conversation":conversation}),
        )
    })
    .await
}

#[tauri::command]
pub async fn deliver_now<R: Runtime>(
    app: AppHandle<R>,
    delivery: Option<String>,
    tab: Option<String>,
    conversation: Option<String>,
    answer_id: Option<String>,
    resend: Option<bool>,
) -> Value {
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking("deliver.now", move || {
        request_node(
            bridge,
            startup_error,
            "deliver.now".to_string(),
            json!({
                "delivery":delivery,
                "tab":tab,
                "conversation":conversation,
                "answerId":answer_id,
                "resend":resend.unwrap_or(false),
            }),
        )
    })
    .await
}

#[tauri::command]
pub async fn deliver_cancel<R: Runtime>(app: AppHandle<R>, delivery: String) -> Value {
    if let Err(error) = validate_text(&delivery, "delivery") {
        return json!({"ok":false,"error":error});
    }
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking("deliver.cancel", move || {
        request_node(
            bridge,
            startup_error,
            "deliver.cancel".to_string(),
            json!({"delivery":delivery}),
        )
    })
    .await
}

#[tauri::command]
pub async fn held_send<R: Runtime>(app: AppHandle<R>, tab: String) -> Value {
    if let Err(error) = validate_text(&tab, "tab") {
        return json!({"ok":false,"error":error});
    }
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking("held.send", move || {
        request_node(
            bridge,
            startup_error,
            "held.send".to_string(),
            json!({"tab":tab}),
        )
    })
    .await
}

#[tauri::command]
pub async fn rename_session<R: Runtime>(app: AppHandle<R>, tab: String, name: String) -> Value {
    if let Err(error) = validate_text(&tab, "tab") {
        return json!({"ok":false,"error":error});
    }
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking("tab.rename", move || {
        request_node(
            bridge,
            startup_error,
            "tab.rename".to_string(),
            json!({"tab":tab,"name":name}),
        )
    })
    .await
}

#[tauri::command]
pub async fn tab_delete<R: Runtime>(app: AppHandle<R>, tab: String, generation: u64) -> Value {
    if let Err(error) = validate_text(&tab, "tab") {
        return json!({"ok":false,"error":error});
    }
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking("tab.delete", move || {
        request_node(
            bridge,
            startup_error,
            "tab.delete".into(),
            json!({"tab":tab,"generation":generation}),
        )
    })
    .await
}

#[tauri::command]
pub async fn tab_resume<R: Runtime>(app: AppHandle<R>, tab: String) -> Value {
    if let Err(error) = validate_text(&tab, "tab") {
        return json!({"ok":false,"error":error});
    }
    let (bridge, startup_error) = {
        let state = app.state::<AppRuntime>();
        (state.bridge.clone(), state.startup_error.clone())
    };
    run_blocking("tab.resume", move || {
        request_node(
            bridge,
            startup_error,
            "tab.resume".to_string(),
            json!({"tab":tab}),
        )
    })
    .await
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
#[tauri::command]
pub async fn subscribe_output<R: Runtime>(
    app: AppHandle<R>,
    on_output: Channel<PaneOutputMessage>,
) -> Value {
    let output = {
        let state = app.state::<AppRuntime>();
        Arc::clone(&state.output)
    };
    output.register(on_output);
    json!({"ok":true})
}

/// The page's whole picture. Carries no channel, on purpose — see
/// [`subscribe_output`].
#[tauri::command]
pub async fn list_state<R: Runtime>(app: AppHandle<R>) -> Value {
    let (bridge, startup_error, roster) = {
        let state = app.state::<AppRuntime>();
        (
            state.bridge.clone(),
            state.startup_error.clone(),
            state.roster.clone(),
        )
    };
    let state_error = startup_error.clone();
    run_blocking("state.list", move || {
        let node = request_node(bridge, startup_error, "state.list".to_string(), json!({}));
        compose_state(node, roster, state_error)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::time::Duration;

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
        for id in ["p-pm", "p-lead", "p-pm"] {
            hub.publish(PaneOutputMessage {
                id: id.into(),
                generation: 1,
                seq: 1,
                bytes: vec![65],
            });
        }
        assert_eq!(*seen.lock().unwrap(), vec!["p-pm", "p-lead", "p-pm"]);
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
    fn open_request_requires_absolute_launch_inputs() {
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
            "cwd":"/tmp",
            "argv":["/bin/sh"],
            "dropEnv":["OPENAI_API_KEY"],
            "size":{"rows":24,"cols":80},
        }))
        .unwrap();
        assert!(validate_open_request(&absolute).is_ok());

        let half_reserved: OpenRequest = parse_body(json!({
            "id":"p-1",
            "launchId":"launch-1",
            "cwd":"/tmp",
            "argv":["/bin/sh"],
            "size":{"rows":24,"cols":80},
        }))
        .unwrap();
        assert!(validate_open_request(&half_reserved).is_err());

        let invalid_drop_env: OpenRequest = parse_body(json!({
            "cwd":"/tmp",
            "argv":["/bin/sh"],
            "dropEnv":["BAD=NAME"],
        }))
        .unwrap();
        assert!(validate_open_request(&invalid_drop_env)
            .unwrap_err()
            .contains("environment variable name"));

        assert!(parse_body::<OpenRequest>(json!({
            "cwd":"/tmp",
            "argv":["/bin/sh"],
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
            "open_lead",
            "open_shell",
            "open_consult",
            "close_pane",
            "delete_pane",
            "pane_input_wait",
            "pane_resize",
            "pane_ack",
            "set_policy",
            "answers_list",
            "deliver_now",
            "deliver_cancel",
            "held_send",
            "tab_resume",
            "tab_delete",
            "rename_session",
            "list_state",
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
        let (event_sender, _event_receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(0, event_sender));
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

        let runtime = AppRuntime {
            panes: Arc::clone(&panes),
            bridge: None,
            editor: Mutex::new(None),
            roster: None,
            startup_error: None,
            output: Arc::new(OutputHub::new()),
            inputs,
            launches: Arc::new(LaunchRegistry::new()),
            shutting_down: AtomicBool::new(false),
        };
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
        let (event_sender, _event_receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(0, event_sender));
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
        let mut ready = [0; 5];
        reader
            .read_exact(&mut ready)
            .expect("read readiness marker");
        assert_eq!(&ready, b"ready");

        let runtime = AppRuntime {
            panes: Arc::clone(&panes),
            bridge: None,
            editor: Mutex::new(None),
            roster: None,
            startup_error: None,
            output: Arc::new(OutputHub::new()),
            inputs,
            launches: Arc::new(LaunchRegistry::new()),
            shutting_down: AtomicBool::new(false),
        };
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
        // A rejected page input still consumes its sequence, although the
        // epoch does not move, so the next sequence remains valid.
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

    #[cfg(unix)]
    #[test]
    fn blocked_command_input_does_not_starve_another_pane_or_output_ack() {
        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let (events, _receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(0, events));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let blocked_key = PaneKey::new("blocked-command", 1);
        let _blocked_reader = panes
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
        arbiter
            .register(&blocked_key)
            .expect("register blocked pane");

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
        let first_output = responsive
            .output
            .recv_timeout(Duration::from_secs(2))
            .expect("responsive pane output");

        let runtime = AppRuntime {
            panes: Arc::clone(&panes),
            bridge: None,
            editor: Mutex::new(None),
            roster: None,
            startup_error: None,
            output: Arc::new(OutputHub::new()),
            inputs: Arc::clone(&inputs),
            launches: Arc::new(LaunchRegistry::new()),
            shutting_down: AtomicBool::new(false),
        };
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
        drop(app);

        assert_eq!(
            responsive_before_cleanup,
            Some(json!({"ok":true,"epoch":1})),
            "one blocked pane consumed the shared blocking pool"
        );
        assert_eq!(
            ack_before_cleanup,
            Some(json!({"ok":true})),
            "output acks shared the blocked PTY pool"
        );
        assert_eq!(responsive_result, Some(json!({"ok":true,"epoch":1})));
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
        let (event_sender, _event_receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(0, event_sender));
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
        let runtime = Arc::new(AppRuntime {
            panes: Arc::clone(&panes),
            bridge: Some(connected.bridge),
            editor: Mutex::new(None),
            roster: None,
            startup_error: None,
            output: Arc::new(OutputHub::new()),
            inputs,
            launches: Arc::new(LaunchRegistry::new()),
            shutting_down: AtomicBool::new(false),
        });
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
    /// closes it — so killing the editor IS the act that shuts admission, and
    /// nothing else in `shutdown()` can do it. What follows the kill is a
    /// drain of what was ALREADY admitted, and only then the reap, so a pane
    /// whose spawn was in flight is in the table before anything reaps it.
    #[cfg(unix)]
    #[test]
    fn gui_shutdown_kills_a_present_editor_then_drains_its_admitted_launch() {
        use std::sync::Barrier;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let (event_sender, _event_receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(0, event_sender));
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

        let runtime = Arc::new(AppRuntime {
            panes: Arc::clone(&panes),
            bridge: Some(connected.bridge),
            editor: Mutex::new(Some(editor)),
            roster: None,
            startup_error: None,
            output: Arc::new(OutputHub::new()),
            inputs,
            launches: Arc::new(LaunchRegistry::new()),
            shutting_down: AtomicBool::new(false),
        });
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

    #[cfg(unix)]
    #[test]
    fn simultaneous_duplicate_launches_wait_for_and_share_one_result() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let (event_sender, _event_receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(0, event_sender));
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
                "rename_session",
                json!({"tab":"tab-1","name":"Build review"}),
                "tab.rename",
                json!({"tab":"tab-1","name":"Build review"}),
            ),
            (
                "open_lead",
                json!({"dir":"/tmp","harness":"claude-code"}),
                "tab.open",
                json!({"dir":"/tmp","harness":"claude-code"}),
            ),
            (
                "open_shell",
                json!({"tab":"tab-1"}),
                "shell.open",
                json!({"tab":"tab-1"}),
            ),
            (
                "open_consult",
                json!({"tab":"tab-1","agent":"asteria","task":"review","conversation":null}),
                "consult",
                json!({"tab":"tab-1","agent":"asteria","task":"review"}),
            ),
            (
                "open_consult",
                json!({"tab":"tab-1","agent":"asteria","task":null,"conversation":"answer-1"}),
                "attach",
                json!({"tab":"tab-1","agent":"asteria","session":"answer-1"}),
            ),
            (
                "set_policy",
                json!({"scope":"pane","id":"pane-1","mode":"manual"}),
                "notify.set",
                json!({"scope":"pane","id":"pane-1","mode":"manual"}),
            ),
            (
                "answers_list",
                json!({"tab":"tab-1","pane":"pane-1","conversation":"answer-1"}),
                "answers.list",
                json!({"tab":"tab-1","pane":"pane-1","conversation":"answer-1"}),
            ),
            (
                "deliver_now",
                json!({"delivery":"delivery-1","tab":"tab-1","conversation":null,"answerId":null,"resend":false}),
                "deliver.now",
                json!({"delivery":"delivery-1","tab":"tab-1","conversation":null,"answerId":null,"resend":false}),
            ),
            (
                "deliver_cancel",
                json!({"delivery":"delivery-1"}),
                "deliver.cancel",
                json!({"delivery":"delivery-1"}),
            ),
            (
                "held_send",
                json!({"tab":"tab-1"}),
                "held.send",
                json!({"tab":"tab-1"}),
            ),
            (
                "tab_resume",
                json!({"tab":"tab-1"}),
                "tab.resume",
                json!({"tab":"tab-1"}),
            ),
            (
                "tab_delete",
                json!({"tab":"tab-1","generation":7}),
                "tab.delete",
                json!({"tab":"tab-1","generation":7}),
            ),
            (
                "list_state",
                json!({"onOutput":"__CHANNEL__:99"}),
                "state.list",
                json!({}),
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
        let (event_sender, _event_receiver) = mpsc::channel();
        let arbiter = Arc::new(InputArbiter::new(0, event_sender));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), arbiter));
        let runtime = AppRuntime {
            panes,
            bridge: Some(connected.bridge),
            editor: Mutex::new(None),
            roster: None,
            startup_error: None,
            output: Arc::new(OutputHub::new()),
            inputs,
            launches: Arc::new(LaunchRegistry::new()),
            shutting_down: AtomicBool::new(false),
        };
        let app = tauri::test::mock_builder()
            .manage(runtime)
            .invoke_handler(tauri::generate_handler![
                open_lead,
                open_shell,
                open_consult,
                set_policy,
                answers_list,
                deliver_now,
                deliver_cancel,
                held_send,
                tab_resume,
                tab_delete,
                rename_session,
                list_state,
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build mock webview");

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
            if command == "list_state" {
                assert_eq!(response, json!({"ok":true,"available":true}));
            } else {
                assert_eq!(response, json!({"ok":true}));
            }
        }

        node.join().expect("Node peer");
        drop(webview);
        drop(app);
    }
}
