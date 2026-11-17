//! Every open pane's input, in order: one worker and one bounded queue per
//! pane, and a second for the terminal's replies, the page's numbered input
//! admitted before it is answered, and why an input did not go in.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use futures_channel::oneshot;
use futures_util::future::{BoxFuture, FutureExt, Shared};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::arbiter::{ArbiterError, InputArbiter};
use crate::pty::{PaneError, PaneInputWriter, PaneKey, PaneTable};
use crate::validation::{validate_input, INVALID_BODY};

const INPUT_QUEUE_CAPACITY: usize = 1024;
const MAX_PENDING_INPUT_BYTES_PER_PANE: usize = 4 * 1024 * 1024;
const MAX_PENDING_INPUT_TICKETS: usize = 4096;
const INPUT_QUEUE_FULL: &str = "pane-input-queue-full";
/// The host is stopping, and takes no more input.
const INPUT_CLOSED: &str = "input-closed";
const LOCK_POISONED: &str = "lock-poisoned";
const INPUT_SEQUENCE_GAP: &str = "pane-input-sequence-gap";
const INPUT_SEQUENCE_REGRESSION: &str = "pane-input-sequence-regression";
const INPUT_STALE_PAGE: &str = "pane-input-stale-page";

/// What a pane's worker does, in order: the human's own keys (counted for
/// what they leave unsent), the daemon's keys, a delivery's paste (and the
/// count of the human's keys at which its adapter read the input box empty, if
/// it did), a native send's claim. And what its replies' worker does: the
/// terminal's answers to what the program asked of it (where its cursor is),
/// which never wait behind the others. A program may read nothing more until it
/// has its answer: Codex on Windows asks where the cursor is as a paste comes,
/// and while the answer waited behind the human's keys held after that paste
/// (or a delivery's Enter, held the same way), it read none of the paste, and
/// took the Enter for a line break in it (T-1048).
#[derive(Clone, PartialEq, Eq)]
pub enum InputWork {
    Typed(Vec<u8>),
    Write(Vec<u8>),
    Reply(Vec<u8>),
    Paste {
        body: Vec<u8>,
        read_empty: Option<u64>,
    },
    Claim,
}

impl InputWork {
    fn byte_count(&self) -> usize {
        match self {
            Self::Typed(bytes) | Self::Write(bytes) | Self::Reply(bytes) => bytes.len(),
            Self::Paste { body, .. } => body.len().saturating_add(13),
            Self::Claim => 0,
        }
    }
}

/// Why a pane's input did not go in. Refused: nothing of it was written, so
/// it may be sent again. Uncertain: the write itself failed partway, and some
/// of it may have reached the window.
#[derive(Clone, Debug)]
pub enum InputError {
    Refused { code: &'static str, cause: String },
    Uncertain(String),
}

impl InputError {
    pub(crate) fn refused(code: &'static str, cause: impl Into<String>) -> Self {
        Self::Refused {
            code,
            cause: cause.into(),
        }
    }

    fn queue_full() -> Self {
        Self::refused(INPUT_QUEUE_FULL, "the pane's input queue is full")
    }

    /// A paste's answer: whether anything reached the window decides whether
    /// the daemon may paste again or must ask the harness what it got.
    pub(crate) fn paste_answer(&self) -> Value {
        match self {
            Self::Refused { code, cause } => json!({
                "ok":false,
                "admitted":false,
                "bytesWritten":0,
                "error":code,
                "cause":cause,
            }),
            Self::Uncertain(cause) => json!({
                "ok":false,
                "admitted":null,
                "error":"uncertain",
                "cause":cause,
            }),
        }
    }
}

/// The cause, which is all the page and the other operations answer.
impl fmt::Display for InputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { cause, .. } | Self::Uncertain(cause) => formatter.write_str(cause),
        }
    }
}

impl From<ArbiterError> for InputError {
    fn from(error: ArbiterError) -> Self {
        let code = match &error {
            ArbiterError::Pane(_) => return Self::Uncertain(error.to_string()),
            ArbiterError::Stale => "stale-pane",
            ArbiterError::Busy => "paste-in-flight",
            ArbiterError::InputFailed => "input-failed",
            ArbiterError::Unsent => "unsent-text",
            ArbiterError::InvalidBody(_) => INVALID_BODY,
            ArbiterError::LockPoisoned => LOCK_POISONED,
        };
        Self::refused(code, error.to_string())
    }
}

pub type InputResponse = Result<(), InputError>;

struct InputJob {
    work: InputWork,
    group: Option<QueuedGroup>,
    response: oneshot::Sender<InputResponse>,
    reserved_bytes: usize,
    pending_bytes: Arc<AtomicUsize>,
}

#[derive(Clone)]
pub struct PageInputCompletion {
    pub receiver: Shared<BoxFuture<'static, InputResponse>>,
    pub human: bool,
    key: PaneKey,
    notified: Arc<AtomicBool>,
}

impl PageInputCompletion {
    /// A retried wait may finish in more than one caller; tell the page's
    /// state about a successful human write only once.
    pub fn notify_human_once(&self) -> bool {
        self.human && !self.notified.swap(true, Ordering::AcqRel)
    }
}

struct PageInputState {
    current: Option<String>,
    retired: HashSet<String>,
    pending_pages: HashSet<String>,
    page_ordinals: HashMap<String, u64>,
    last_sequences: HashMap<PaneKey, u64>,
    last_tickets: HashMap<PaneKey, String>,
    completions: HashMap<String, PageInputCompletion>,
    next_ticket: u64,
}

/// Consecutive chunks of one page input must not be interrupted by a terminal reply.
#[derive(Clone, Debug, Deserialize)]
pub struct PageInputGroup {
    pub id: u64,
    pub start: bool,
    pub end: bool,
}

struct QueuedGroup {
    epoch: usize,
    group: PageInputGroup,
}

#[derive(Default)]
struct PasteGate {
    state: Mutex<PasteGateState>,
    changed: Condvar,
}

#[derive(Default)]
struct PasteGateState {
    active: Option<(usize, u64)>,
    pending: HashMap<(usize, u64), usize>,
    abandoned: HashSet<(usize, u64)>,
    bracketed: HashSet<(usize, u64)>,
    needs_close: bool,
    repairing: bool,
    reply_in_progress: bool,
    closed: bool,
}

/// Take the paste gate only when the arbiter is ready to write the first
/// chunk. It may first wait for an earlier paste, whose terminal reply must
/// remain able to pass that wait.
struct GroupWriter<'a> {
    panes: &'a PaneTable,
    gate: &'a PasteGate,
    group: &'a QueuedGroup,
}

impl PaneInputWriter for GroupWriter<'_> {
    fn write(&self, key: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
        if self.group.group.start && !bytes.is_empty() {
            let mut state = self
                .gate
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while (state.reply_in_progress || state.repairing) && !state.closed {
                state = self
                    .gate
                    .changed
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
            let id = (self.group.epoch, self.group.group.id);
            if !state.closed && state.pending.contains_key(&id) {
                state.active = Some(id);
                if bytes.starts_with(b"\x1b[200~") {
                    state.bracketed.insert(id);
                }
            }
        }
        self.panes.write(key, bytes)
    }
}

struct InputRoute {
    sender: mpsc::SyncSender<InputJob>,
    /// The terminal's replies, to a worker of their own ([`InputWork::Reply`]).
    replies: mpsc::SyncSender<InputJob>,
    pending_bytes: Arc<AtomicUsize>,
    paste_gate: Arc<PasteGate>,
}

/// Every open pane's input, in order: one worker and one bounded queue per
/// pane, from its `pane.open` until it leaves the table.
pub struct InputQueue {
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    senders: Mutex<HashMap<PaneKey, InputRoute>>,
    workers: Mutex<HashMap<PaneKey, Vec<JoinHandle<()>>>>,
    page: Mutex<PageInputState>,
    page_epoch: Arc<AtomicUsize>,
    accepting: AtomicBool,
}

impl InputQueue {
    pub fn new(panes: Arc<PaneTable>, arbiter: Arc<InputArbiter>) -> Self {
        Self {
            panes,
            arbiter,
            senders: Mutex::new(HashMap::new()),
            workers: Mutex::new(HashMap::new()),
            page: Mutex::new(PageInputState {
                current: None,
                retired: HashSet::new(),
                pending_pages: HashSet::new(),
                page_ordinals: HashMap::new(),
                last_sequences: HashMap::new(),
                last_tickets: HashMap::new(),
                completions: HashMap::new(),
                next_ticket: 1,
            }),
            page_epoch: Arc::new(AtomicUsize::new(0)),
            accepting: AtomicBool::new(true),
        }
    }

    /// Admit one numbered chunk from the active page. Only successful admissions
    /// advance the count, so a count query can distinguish an unseen request.
    pub fn enqueue_page(
        &self,
        page_id: &str,
        key: PaneKey,
        sequence: u64,
        work: InputWork,
        human: bool,
        group: Option<PageInputGroup>,
    ) -> Result<String, String> {
        let mut page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        if page.current.as_deref() != Some(page_id) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
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
        match &work {
            InputWork::Typed(bytes)
            | InputWork::Write(bytes)
            | InputWork::Reply(bytes)
            | InputWork::Paste { body: bytes, .. } => validate_input(bytes)?,
            InputWork::Claim => {}
        }
        if group.is_some() && !human {
            return Err("only human input can start a paste group".to_string());
        }
        if page.completions.len() >= MAX_PENDING_INPUT_TICKETS {
            return Err("pane-input-ticket-capacity".to_string());
        }
        let next_ticket = page
            .next_ticket
            .checked_add(1)
            .ok_or_else(|| "pane-input-ticket-overflow".to_string())?;
        let group = group.map(|group| QueuedGroup {
            epoch: self.page_epoch.load(Ordering::Acquire),
            group,
        });
        let receiver = self
            .submit_group(key.clone(), work, group)
            .map_err(|error| error.to_string())?;
        let ticket = format!("pane-input-{}", page.next_ticket);
        page.next_ticket = next_ticket;
        page.last_sequences.insert(key.clone(), sequence);
        page.last_tickets.insert(key.clone(), ticket.clone());
        page.completions.insert(
            ticket.clone(),
            PageInputCompletion {
                receiver: async move { wait_for_input(receiver).await }
                    .boxed()
                    .shared(),
                human,
                key,
                notified: Arc::new(AtomicBool::new(false)),
            },
        );
        Ok(ticket)
    }

    /// The active page's next count and the last admitted ticket. The ticket
    /// holds a completion, never the bytes: it recovers an answer lost in transit.
    pub fn expected_page_sequence(
        &self,
        page_id: &str,
        key: &PaneKey,
    ) -> Result<(u64, Option<String>), String> {
        let page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        if page.current.as_deref() != Some(page_id) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        let next = page
            .last_sequences
            .get(key)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| "pane-input-sequence-overflow".to_string())?;
        Ok((next, page.last_tickets.get(key).cloned()))
    }

    fn check_page(page: &PageInputState, page_id: &str) -> Result<Option<(String, u64)>, String> {
        if page_id.is_empty() || page_id.len() > 128 {
            return Err("invalid pane input page identity".to_string());
        }
        if page.retired.contains(page_id) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        let ordinal = page_id
            .rsplit_once(':')
            .and_then(|(session, number)| {
                number
                    .parse::<u64>()
                    .ok()
                    .map(|number| (session.to_owned(), number))
            })
            .filter(|(session, number)| !session.is_empty() && *number > 0);
        if let Some((session, number)) = &ordinal {
            if page
                .page_ordinals
                .get(session)
                .is_some_and(|seen| *seen >= *number)
            {
                return Err(INPUT_STALE_PAGE.to_string());
            }
        }
        Ok(ordinal)
    }

    /// A page first announces itself; it cannot write until it receives the
    /// answer and activates. Navigation retires even pending announcements.
    pub fn prepare_page(&self, page_id: &str) -> Result<(), String> {
        let mut page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        if page.current.as_deref() == Some(page_id) || page.pending_pages.contains(page_id) {
            return Ok(());
        }
        Self::check_page(&page, page_id)?;
        page.pending_pages.insert(page_id.to_owned());
        Ok(())
    }

    pub fn activate_page(&self, page_id: &str) -> Result<(), String> {
        let mut page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        if page.current.as_deref() == Some(page_id) {
            return Ok(());
        }
        if !page.pending_pages.remove(page_id) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        self.start_page(&mut page, page_id)
    }

    /// The pane host's own tests can start a page at the subscription boundary.
    pub fn begin_page(&self, page_id: &str) -> Result<(), String> {
        let mut page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        if page.current.as_deref() == Some(page_id) {
            return Ok(());
        }
        self.start_page(&mut page, page_id)
    }

    fn start_page(&self, page: &mut PageInputState, page_id: &str) -> Result<(), String> {
        let ordinal = Self::check_page(page, page_id)?;
        self.retire_page(page)?;
        if let Some((session, number)) = ordinal {
            page.page_ordinals.insert(session, number);
        }
        page.current = Some(page_id.to_owned());
        Ok(())
    }

    pub fn page_is_current(&self, page_id: &str) -> Result<(), String> {
        self.with_current_page(page_id, || ())
    }

    /// Keep a page replacement from overtaking an output subscription after
    /// that subscription checked its identity.
    pub fn with_current_page<T>(
        &self,
        page_id: &str,
        action: impl FnOnce() -> T,
    ) -> Result<T, String> {
        let page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        if page.current.as_deref() != Some(page_id) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        let result = action();
        drop(page);
        Ok(result)
    }

    /// Navigation has begun, before a replacement page subscribes. Delayed
    /// calls from the departed page may no longer enter a pane.
    pub fn end_page(&self) -> Result<(), String> {
        let mut page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        if page.current.is_some() || !page.pending_pages.is_empty() {
            self.retire_page(&mut page)?;
        }
        Ok(())
    }

    fn retire_page(&self, page: &mut PageInputState) -> Result<(), String> {
        if let Some(previous) = page.current.take() {
            page.retired.insert(previous);
        }
        page.retired.extend(std::mem::take(&mut page.pending_pages));
        page.last_sequences.clear();
        page.last_tickets.clear();
        page.completions.clear();
        let epoch = self
            .page_epoch
            .load(Ordering::Acquire)
            .checked_add(1)
            .ok_or_else(|| "pane-input-page-overflow".to_string())?;
        self.page_epoch.store(epoch, Ordering::Release);
        let senders = self
            .senders
            .lock()
            .map_err(|_| "pane input queue lock is poisoned".to_string())?;
        for route in senders.values() {
            let mut state = route
                .paste_gate
                .state
                .lock()
                .map_err(|_| "pane paste lock is poisoned".to_string())?;
            let old_groups: Vec<_> = state.pending.keys().copied().collect();
            state.abandoned.extend(old_groups);
            if state
                .active
                .is_some_and(|id| !state.pending.contains_key(&id))
            {
                let id = state.active.take().expect("active paste");
                if state.bracketed.remove(&id) {
                    state.needs_close = true;
                }
            }
            route.paste_gate.changed.notify_all();
        }
        Ok(())
    }

    /// A wait may be repeated until the page has actually received its answer.
    pub fn page_completion(
        &self,
        page_id: &str,
        ticket: &str,
    ) -> Result<PageInputCompletion, String> {
        let page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        if page.current.as_deref() != Some(page_id) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        page.completions
            .get(ticket)
            .cloned()
            .ok_or_else(|| "pane-input-ticket-not-found".to_string())
    }

    /// The page got the answer and no longer needs to retry the wait.
    pub fn forget_page_completion(&self, page_id: &str, ticket: &str) -> Result<(), String> {
        let mut page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        if page.current.as_deref() != Some(page_id) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        page.completions.remove(ticket);
        Ok(())
    }

    pub(crate) fn write(
        &self,
        key: PaneKey,
        bytes: Vec<u8>,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        self.submit(key, InputWork::Write(bytes))
    }

    /// The terminal's answer to what the program asked of it, from a peer that answers as
    /// the page's terminal does (`pane.reply`): it goes past whatever the pane's input waits on.
    pub(crate) fn reply(
        &self,
        key: PaneKey,
        bytes: Vec<u8>,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        self.submit(key, InputWork::Reply(bytes))
    }

    /// The human's own keys, from a peer that types as the page does (the
    /// headless host's: `pane.typed`).
    pub(crate) fn typed(
        &self,
        key: PaneKey,
        bytes: Vec<u8>,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        self.submit(key, InputWork::Typed(bytes))
    }

    pub(crate) fn paste(
        &self,
        key: PaneKey,
        body: Vec<u8>,
        read_empty: Option<u64>,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        self.submit(key, InputWork::Paste { body, read_empty })
    }

    /// Admit a native-channel send: the pane is current and its input works,
    /// and no paste is going in, since the pane's worker runs one job at a
    /// time.
    pub(crate) fn claim(
        &self,
        key: PaneKey,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        self.submit(key, InputWork::Claim)
    }

    /// A pane just opened gets its worker and queue.
    pub fn open(&self, key: &PaneKey) -> Result<(), String> {
        let mut senders = self
            .senders
            .lock()
            .map_err(|_| "pane input queue lock is poisoned".to_string())?;
        if !self.accepting.load(Ordering::Acquire) {
            return Err("pane input admission is closed".to_string());
        }
        let (sender, jobs) = mpsc::sync_channel(INPUT_QUEUE_CAPACITY);
        let (replies, replied) = mpsc::sync_channel(INPUT_QUEUE_CAPACITY);
        let paste_gate = Arc::new(PasteGate::default());
        let mut workers = Vec::with_capacity(2);
        for (name, jobs) in [("input", jobs), ("replies", replied)] {
            let panes = Arc::clone(&self.panes);
            let arbiter = Arc::clone(&self.arbiter);
            let worker_key = key.clone();
            let gate = Arc::clone(&paste_gate);
            let is_reply = name == "replies";
            let worker = thread::Builder::new()
                .name(format!("consensflow-{name}-{}", worker_key.id))
                .spawn(move || input_worker(jobs, panes, arbiter, worker_key, gate, is_reply))
                .map_err(|error| format!("could not start pane input queue: {error}"))?;
            workers.push(worker);
        }
        self.workers
            .lock()
            .map_err(|_| "pane input worker lock is poisoned".to_string())?
            .insert(key.clone(), workers);
        senders.insert(
            key.clone(),
            InputRoute {
                sender,
                replies,
                pending_bytes: Arc::new(AtomicUsize::new(0)),
                paste_gate,
            },
        );
        Ok(())
    }

    /// A pane gone from the table, killed or retired when its program ended,
    /// takes its input with it: its queue closes (the worker ends once what
    /// was queued has gone in or failed), and its page count and arbiter
    /// state go. They used to stay until the app quit, a parked thread per
    /// window ever opened.
    pub(crate) fn retire(&self, key: &PaneKey) {
        self.senders
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(key);
        self.workers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(key);
        let mut page = self.page.lock().unwrap_or_else(|error| error.into_inner());
        page.last_sequences.remove(key);
        page.last_tickets.remove(key);
        page.completions
            .retain(|_, completion| &completion.key != key);
        drop(page);
        self.arbiter.retire(key);
    }

    fn submit(
        &self,
        key: PaneKey,
        work: InputWork,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        self.submit_group(key, work, None)
    }

    fn submit_group(
        &self,
        key: PaneKey,
        work: InputWork,
        group: Option<QueuedGroup>,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        let closed = || InputError::refused(INPUT_CLOSED, "pane input admission is closed");
        if !self.accepting.load(Ordering::Acquire) {
            return Err(closed());
        }
        let senders = self
            .senders
            .lock()
            .map_err(|_| InputError::refused(LOCK_POISONED, "pane input queue lock is poisoned"))?;
        if !self.accepting.load(Ordering::Acquire) {
            return Err(closed());
        }
        // A pane the table does not hold has no queue: it was never opened,
        // or it is gone.
        let route = senders.get(&key).ok_or(ArbiterError::Stale)?;
        let reserved_bytes = work.byte_count();
        route
            .pending_bytes
            .try_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                pending
                    .checked_add(reserved_bytes)
                    .filter(|next| *next <= MAX_PENDING_INPUT_BYTES_PER_PANE)
            })
            .map_err(|_| InputError::queue_full())?;
        let (response, receiver) = oneshot::channel();
        let group_id = group.as_ref().map(|group| (group.epoch, group.group.id));
        let job = InputJob {
            work,
            group,
            response,
            reserved_bytes,
            pending_bytes: Arc::clone(&route.pending_bytes),
        };
        let sender = match job.work {
            InputWork::Reply(_) => &route.replies,
            _ => &route.sender,
        };
        let mut group_state = group_id.map(|_| {
            route
                .paste_gate
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
        });
        match sender.try_send(job) {
            Ok(()) => {
                if let (Some(id), Some(state)) = (group_id, group_state.as_mut()) {
                    *state.pending.entry(id).or_default() += 1;
                }
                Ok(receiver)
            }
            Err(mpsc::TrySendError::Full(job)) => {
                job.pending_bytes
                    .fetch_sub(job.reserved_bytes, Ordering::AcqRel);
                Err(InputError::queue_full())
            }
            Err(mpsc::TrySendError::Disconnected(job)) => {
                job.pending_bytes
                    .fetch_sub(job.reserved_bytes, Ordering::AcqRel);
                Err(InputError::refused(
                    INPUT_CLOSED,
                    format!("pane input queue for {} is closed", key.id),
                ))
            }
        }
    }

    pub fn close_and_drain(&self) {
        if !self.accepting.swap(false, Ordering::AcqRel) {
            return;
        }
        let mut senders = self
            .senders
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        senders.clear();
        drop(senders);
        let workers = std::mem::take(
            &mut *self
                .workers
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        for worker in workers.into_values().flatten() {
            let _ = worker.join();
        }
    }
}

/// A reloaded page cannot send the remainder of its paste. Close a bracketed
/// paste after its admitted chunks have drained, before any new key or reply.
fn close_interrupted_paste(
    panes: &PaneTable,
    arbiter: &InputArbiter,
    key: &PaneKey,
    gate: &PasteGate,
) -> Result<(), ArbiterError> {
    let mut state = gate.state.lock().unwrap_or_else(|error| error.into_inner());
    while state.repairing || (state.needs_close && state.reply_in_progress) {
        state = gate
            .changed
            .wait(state)
            .unwrap_or_else(|error| error.into_inner());
    }
    if !state.needs_close {
        return Ok(());
    }
    state.needs_close = false;
    state.repairing = true;
    drop(state);
    let result = arbiter.write_typed(panes, key, b"\x1b[201~");
    let mut state = gate.state.lock().unwrap_or_else(|error| error.into_inner());
    state.repairing = false;
    gate.changed.notify_all();
    result
}

fn write_reply(
    panes: &PaneTable,
    arbiter: &InputArbiter,
    key: &PaneKey,
    gate: &PasteGate,
    bytes: &[u8],
) -> Result<(), ArbiterError> {
    loop {
        close_interrupted_paste(panes, arbiter, key, gate)?;
        let mut state = gate.state.lock().unwrap_or_else(|error| error.into_inner());
        while (state.active.is_some() || state.repairing) && !state.closed {
            state = gate
                .changed
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
        if state.needs_close {
            continue;
        }
        state.reply_in_progress = true;
        drop(state);
        let result = arbiter.write(panes, key, bytes);
        let mut state = gate.state.lock().unwrap_or_else(|error| error.into_inner());
        state.reply_in_progress = false;
        gate.changed.notify_all();
        return result;
    }
}

fn input_worker(
    jobs: mpsc::Receiver<InputJob>,
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    key: PaneKey,
    gate: Arc<PasteGate>,
    is_reply: bool,
) {
    for job in jobs {
        let result = close_interrupted_paste(&panes, &arbiter, &key, &gate)
            .and_then(|_| match job.work {
                InputWork::Typed(bytes) => match &job.group {
                    Some(group) => arbiter.write_typed_via(
                        &GroupWriter {
                            panes: &panes,
                            gate: &gate,
                            group,
                        },
                        &key,
                        &bytes,
                    ),
                    None => arbiter.write_typed(&panes, &key, &bytes),
                },
                InputWork::Write(bytes) => arbiter.write(&panes, &key, &bytes),
                InputWork::Reply(bytes) => write_reply(&panes, &arbiter, &key, &gate, &bytes),
                InputWork::Paste { body, read_empty } => {
                    arbiter.write_paste(&panes, &key, &body, read_empty)
                }
                InputWork::Claim => arbiter.claim(&key),
            })
            .map_err(InputError::from);
        if let Some(group) = &job.group {
            let id = (group.epoch, group.group.id);
            let mut state = gate.state.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(pending) = state.pending.get_mut(&id) {
                *pending -= 1;
                if *pending == 0 {
                    state.pending.remove(&id);
                }
            }
            let abandoned_and_drained =
                state.abandoned.contains(&id) && !state.pending.contains_key(&id);
            if group.group.end || result.is_err() || abandoned_and_drained {
                let bracketed = state.bracketed.remove(&id);
                if abandoned_and_drained && !group.group.end && result.is_ok() && bracketed {
                    state.needs_close = true;
                }
                if state.active == Some(id) {
                    state.active = None;
                }
                if !state.pending.contains_key(&id) {
                    state.abandoned.remove(&id);
                }
                gate.changed.notify_all();
            }
        }
        job.pending_bytes
            .fetch_sub(job.reserved_bytes, Ordering::AcqRel);
        let _ = job.response.send(result);
    }
    if !is_reply {
        let mut state = gate.state.lock().unwrap_or_else(|error| error.into_inner());
        state.active = None;
        state.closed = true;
        gate.changed.notify_all();
    }
}

pub async fn wait_for_input(receiver: oneshot::Receiver<InputResponse>) -> InputResponse {
    // The worker answers every job it takes: one left unanswered went down
    // with the worker, perhaps in the middle of its write.
    receiver
        .await
        .map_err(|_| InputError::Uncertain("pane input queue ended before answering".to_string()))?
}

pub(crate) fn wait_for_input_blocking(receiver: oneshot::Receiver<InputResponse>) -> InputResponse {
    futures_executor::block_on(wait_for_input(receiver))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arbiter::EnterTiming;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    use crate::arbiter::SanitizeError;
    #[cfg(unix)]
    use crate::output_hub::OutputHub;
    #[cfg(unix)]
    use crate::pane_handlers::register_pane_handlers;
    use crate::pty::PaneError;
    #[cfg(unix)]
    use cf_bridge::BridgeBuilder;
    #[cfg(unix)]
    use cf_proto::bridge::Role;

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
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        inputs.begin_page("test-page").unwrap();
        let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            Arc::clone(&arbiter),
            Arc::new(OutputHub::new()),
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
                .enqueue_page(
                    "test-page",
                    key.clone(),
                    1,
                    InputWork::Write(b"x".to_vec()),
                    true,
                    None,
                )
                .expect("the page types into the pane");
            assert!(!gone(key), "the pane has its input");
        }

        assert_eq!(
            ask("pane.kill", json!({"id":killed.id,"generation":1})),
            json!({"ok":true})
        );
        assert!(gone(&killed), "a killed pane kept its input");

        assert_eq!(
            ask(
                "pane.input",
                json!({"id":ended.id,"generation":1,"bytes":[13]})
            ),
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

    /// The terminal's answer to what the program asked goes past the pane's input held
    /// behind a human's paste (T-1048): Codex on Windows asks where the cursor is as a paste
    /// comes and reads none of it until it has the answer, and the answer waited behind the
    /// human's Enter, held until the window drew the paste, which it never could. Here the
    /// program reads the paste and says so by a file (the Enter is held behind it), reads the
    /// answer, and draws only then: the answer sent once the program has the paste is read
    /// before their held Enter, or the program never draws and the Enter waits for its cap.
    /// No time of the machine's is in it: the events are the program's, and the cap is two
    /// minutes, which only an answer held behind the Enter runs into.
    #[cfg(unix)]
    #[test]
    fn a_terminals_reply_goes_past_the_humans_keys_held_after_their_paste() {
        use std::collections::HashMap;
        use std::io::Read;
        use std::path::Path;

        use cf_proto::panes::SilentPaste;
        use portable_pty::PtySize;

        use crate::testing::{answered_within_liveness, hex, SilentReader, HELD_FOR_MS, LIVENESS};

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter));
        let paste: &[u8] = b"\x1b[200~ab\x1b[201~";
        let program = SilentReader::new("reply-queue", paste.len());
        let opened = panes
            .open(
                Path::new("/tmp"),
                &["/bin/sh".to_owned(), "-c".to_owned(), program.script()],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open a recorder");
        let key = opened.key.clone();
        let mut reader = opened.reader;
        let mut ready = [0_u8; 5];
        reader
            .read_exact(&mut ready)
            .expect("the recorder is ready");
        assert_eq!(&ready, b"ready");
        // Held until the window draws the paste, which the program does once it has read the
        // answer: the cap is two minutes, which only a stuck answer waits for.
        let silent = SilentPaste {
            pace_us_per_character: 0,
            drawn_after_ms: 0,
            quiet_ms: 100,
            most_ms: HELD_FOR_MS,
            most_us_per_character: 0,
        };
        let output = arbiter
            .register_with(&key, Some(silent))
            .expect("register the pane");
        inputs.open(&key).expect("open the pane's input");
        // What the pane prints is noted as the pane host's output thread notes it: the
        // program's "drawn" is the window's draw of the paste.
        let recorded = Arc::new(Mutex::new(String::new()));
        let printing = {
            let recorded = Arc::clone(&recorded);
            thread::spawn(move || {
                let mut chunk = [0_u8; 256];
                while let Ok(read) = reader.read(&mut chunk) {
                    if read == 0 {
                        break;
                    }
                    output.note(true);
                    recorded
                        .lock()
                        .expect("what the program printed")
                        .push_str(&String::from_utf8_lossy(&chunk[..read]));
                }
            })
        };

        let typed = inputs
            .typed(key.clone(), [paste, b"\r"].concat())
            .expect("the human's paste and Enter");
        // The paste is in the program: the human's Enter is held behind it.
        program.wait_pasted();
        let replied = inputs
            .reply(key.clone(), b"R".to_vec())
            .expect("the terminal's answer");
        answered_within_liveness(replied, "the answer waited behind the held Enter")
            .expect("the answer went in");
        answered_within_liveness(typed, "the Enter was never written after the window drew")
            .expect("the paste and Enter went in");

        // The program says what it read, in the order it read it.
        let record = format!("drawn{}520d", hex(paste));
        let deadline = Instant::now() + LIVENESS;
        loop {
            let said: String = recorded
                .lock()
                .expect("what the program printed")
                .split_whitespace()
                .collect();
            if said.contains(&record) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the program's record is not the paste, the answer and then the Enter: {said}"
            );
            thread::sleep(Duration::from_millis(5));
        }
        panes.kill(&key).expect("kill the pane");
        printing.join().expect("the pane's output thread");
        inputs.close_and_drain();
    }

    /// A second paste may be waiting for the first to draw. Its reply must
    /// pass that wait, before the second paste has started writing.
    #[cfg(unix)]
    #[test]
    fn a_reply_passes_a_later_paste_waiting_for_an_earlier_paste() {
        use crate::testing::SilentReader;
        use cf_proto::panes::SilentPaste;
        use portable_pty::PtySize;
        use std::collections::HashMap;
        use std::io::Read;
        use std::path::Path;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter));
        inputs.begin_page("page").unwrap();
        let paste: &[u8] = b"\x1b[200~ab\x1b[201~";
        let program = SilentReader::new("paste-gate", paste.len());
        let opened = panes
            .open(
                Path::new("/tmp"),
                &["/bin/sh".to_owned(), "-c".to_owned(), program.script()],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .unwrap();
        let key = opened.key;
        let mut reader = opened.reader;
        let mut ready = [0_u8; 5];
        reader.read_exact(&mut ready).unwrap();
        assert_eq!(&ready, b"ready");
        let silent = SilentPaste {
            pace_us_per_character: 0,
            drawn_after_ms: 0,
            quiet_ms: 100,
            most_ms: 2_000,
            most_us_per_character: 0,
        };
        let output = arbiter.register_with(&key, Some(silent)).unwrap();
        inputs.open(&key).unwrap();
        let printing = thread::spawn(move || {
            let mut chunk = [0_u8; 256];
            while let Ok(read) = reader.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                output.note(true);
            }
        });
        let first = inputs
            .enqueue_page(
                "page",
                key.clone(),
                1,
                InputWork::Typed(paste.to_vec()),
                true,
                None,
            )
            .unwrap();
        futures_executor::block_on(inputs.page_completion("page", &first).unwrap().receiver)
            .unwrap();
        program.wait_pasted();
        let second = inputs
            .enqueue_page(
                "page",
                key.clone(),
                2,
                InputWork::Typed(b"x".to_vec()),
                true,
                Some(PageInputGroup {
                    id: 1,
                    start: true,
                    end: true,
                }),
            )
            .unwrap();
        thread::sleep(Duration::from_millis(100));
        let reply = inputs
            .enqueue_page(
                "page",
                key.clone(),
                3,
                InputWork::Reply(b"R".to_vec()),
                false,
                None,
            )
            .unwrap();
        let completion = inputs.page_completion("page", &reply).unwrap();
        let (sent, done) = mpsc::channel();
        let waiter = thread::spawn(move || {
            sent.send(futures_executor::block_on(completion.receiver))
                .unwrap()
        });
        let prompt = done.recv_timeout(Duration::from_millis(1_000));
        if prompt.is_err() {
            done.recv_timeout(Duration::from_secs(4)).unwrap().unwrap();
        }
        futures_executor::block_on(inputs.page_completion("page", &second).unwrap().receiver)
            .unwrap();
        waiter.join().unwrap();
        panes.kill(&key).unwrap();
        printing.join().unwrap();
        inputs.close_and_drain();
        assert!(
            prompt.is_ok(),
            "the reply was held behind a paste that had not started"
        );
    }

    /// A reset may happen with already-admitted paste chunks still in the
    /// worker. A new-page reply must not cut between those chunks.
    #[cfg(unix)]
    #[test]
    fn a_page_reset_keeps_already_admitted_paste_chunks_together() {
        use portable_pty::PtySize;
        use std::collections::HashMap;
        use std::io::Read;
        use std::path::Path;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter));
        inputs.begin_page("old").unwrap();
        let opened = panes
            .open(
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_owned(),
                    "-c".to_owned(),
                    "stty raw -echo; printf READY; sleep 1; dd bs=1 count=65538 2>/dev/null"
                        .to_owned(),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .unwrap();
        let key = opened.key;
        arbiter.register(&key).unwrap();
        inputs.open(&key).unwrap();
        let mut reader = opened.reader;
        let mut ready = [0; 5];
        reader.read_exact(&mut ready).unwrap();
        assert_eq!(&ready, b"READY");
        let (sent, received) = mpsc::channel();
        let reading = thread::spawn(move || {
            let mut bytes = vec![0; 65538];
            reader.read_exact(&mut bytes).unwrap();
            sent.send(bytes).unwrap();
        });
        let first = PageInputGroup {
            id: 1,
            start: true,
            end: false,
        };
        let last = PageInputGroup {
            id: 1,
            start: false,
            end: true,
        };
        inputs
            .enqueue_page(
                "old",
                key.clone(),
                1,
                InputWork::Typed(vec![b'p'; 65536]),
                true,
                Some(first),
            )
            .unwrap();
        inputs
            .enqueue_page(
                "old",
                key.clone(),
                2,
                InputWork::Typed(b"B".to_vec()),
                true,
                Some(last),
            )
            .unwrap();
        let gate = Arc::clone(&inputs.senders.lock().unwrap().get(&key).unwrap().paste_gate);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let state = gate.state.lock().unwrap();
            if state.active.is_some() && state.pending.values().sum::<usize>() >= 2 {
                break;
            }
            drop(state);
            assert!(
                Instant::now() < deadline,
                "the first paste write did not block"
            );
            thread::sleep(Duration::from_millis(2));
        }
        inputs.begin_page("new").unwrap();
        let reply = inputs
            .enqueue_page(
                "new",
                key.clone(),
                1,
                InputWork::Reply(b"R".to_vec()),
                false,
                None,
            )
            .unwrap();
        futures_executor::block_on(inputs.page_completion("new", &reply).unwrap().receiver)
            .unwrap();
        let bytes = received.recv_timeout(Duration::from_secs(5)).unwrap();
        reading.join().unwrap();
        inputs.close_and_drain();
        panes.kill(&key).unwrap();
        let mut expected = vec![b'p'; 65536];
        expected.extend_from_slice(b"BR");
        assert_eq!(bytes, expected);
    }

    #[cfg(unix)]
    #[test]
    fn an_interrupted_paste_closes_after_its_already_admitted_chunks() {
        use portable_pty::PtySize;
        use std::collections::HashMap;
        use std::io::Read;
        use std::path::Path;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter));
        inputs.begin_page("old").unwrap();
        let opened = panes
            .open(
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_owned(),
                    "-c".to_owned(),
                    "stty raw -echo; printf READY; sleep 1; dd bs=1 count=65544 2>/dev/null"
                        .to_owned(),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .unwrap();
        let key = opened.key;
        arbiter.register(&key).unwrap();
        inputs.open(&key).unwrap();
        let mut reader = opened.reader;
        let mut ready = [0; 5];
        reader.read_exact(&mut ready).unwrap();
        assert_eq!(&ready, b"READY");
        let (sent, received) = mpsc::channel();
        let reading = thread::spawn(move || {
            let mut bytes = vec![0; 65544];
            reader.read_exact(&mut bytes).unwrap();
            sent.send(bytes).unwrap();
        });
        let first = PageInputGroup {
            id: 1,
            start: true,
            end: false,
        };
        let last = PageInputGroup {
            id: 1,
            start: false,
            end: false,
        };
        inputs
            .enqueue_page(
                "old",
                key.clone(),
                1,
                InputWork::Typed([b"\x1b[200~".as_slice(), &vec![b'p'; 65530]].concat()),
                true,
                Some(first),
            )
            .unwrap();
        inputs
            .enqueue_page(
                "old",
                key.clone(),
                2,
                InputWork::Typed(b"B".to_vec()),
                true,
                Some(last),
            )
            .unwrap();
        let gate = Arc::clone(&inputs.senders.lock().unwrap().get(&key).unwrap().paste_gate);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let state = gate.state.lock().unwrap();
            if state.active.is_some() && state.pending.values().sum::<usize>() >= 2 {
                break;
            }
            drop(state);
            assert!(
                Instant::now() < deadline,
                "the first paste write did not block"
            );
            thread::sleep(Duration::from_millis(2));
        }
        inputs.begin_page("new").unwrap();
        let reply = inputs
            .enqueue_page(
                "new",
                key.clone(),
                1,
                InputWork::Reply(b"R".to_vec()),
                false,
                None,
            )
            .unwrap();
        futures_executor::block_on(inputs.page_completion("new", &reply).unwrap().receiver)
            .unwrap();
        let bytes = received.recv_timeout(Duration::from_secs(5));
        inputs.close_and_drain();
        panes.kill(&key).unwrap();
        let _ = reading.join();
        let mut expected = b"\x1b[200~".to_vec();
        expected.extend(vec![b'p'; 65530]);
        expected.extend_from_slice(b"B\x1b[201~R");
        assert_eq!(bytes.unwrap(), expected);
    }

    /// Input for a pane the table never held is refused, and starts no
    /// worker: every key used to get a thread and a queue of its own.
    #[test]
    fn input_for_a_pane_never_opened_is_refused() {
        let panes = Arc::new(PaneTable::new());
        let inputs = InputQueue::new(
            Arc::clone(&panes),
            Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
        );
        let refused = inputs.write(PaneKey::new("never-opened", 1), b"x".to_vec());
        assert!(matches!(
            refused,
            Err(InputError::Refused { code: "stale-pane", ref cause }) if cause == "stale pane generation"
        ));
        assert!(inputs.senders.lock().unwrap().is_empty());
        assert!(inputs.workers.lock().unwrap().is_empty());
    }

    /// A host that is stopping takes no more input: what is sent is refused
    /// as closed, nothing of it written, and no pane gets a queue.
    #[test]
    fn a_stopping_host_refuses_input_as_closed() {
        let panes = Arc::new(PaneTable::new());
        let inputs = InputQueue::new(
            Arc::clone(&panes),
            Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
        );
        let key = PaneKey::new("open-pane", 1);
        inputs.open(&key).expect("open the pane's input");
        inputs.close_and_drain();

        let refused = inputs.write(key, b"x".to_vec());
        assert!(matches!(
            refused,
            Err(InputError::Refused { code: "input-closed", ref cause }) if cause == "pane input admission is closed"
        ));
        assert_eq!(
            inputs.open(&PaneKey::new("late-pane", 1)),
            Err("pane input admission is closed".to_string())
        );
        assert!(inputs.workers.lock().unwrap().is_empty());
    }

    /// A paste refused by the arbiter answers that nothing of it went in,
    /// with the code and the cause, so the daemon may send it again; a write
    /// that failed once begun answers that it may have.
    #[test]
    fn a_refused_paste_says_nothing_went_in_and_a_failed_one_is_uncertain() {
        for (error, code) in [
            (ArbiterError::Stale, "stale-pane"),
            (ArbiterError::Busy, "paste-in-flight"),
            (ArbiterError::InputFailed, "input-failed"),
            (ArbiterError::Unsent, "unsent-text"),
            (
                ArbiterError::InvalidBody(SanitizeError::ControlByte(7)),
                "invalid-body",
            ),
            (ArbiterError::LockPoisoned, "lock-poisoned"),
        ] {
            let cause = error.to_string();
            assert_eq!(
                InputError::from(error).paste_answer(),
                json!({"ok":false,"admitted":false,"bytesWritten":0,"error":code,"cause":cause})
            );
        }
        assert_eq!(
            InputError::from(ArbiterError::Pane(PaneError::LockPoisoned)).paste_answer(),
            json!({"ok":false,"admitted":null,"error":"uncertain","cause":"pane table lock is poisoned"})
        );
    }

    /// A count query recovers a lost answer without storing or resending the bytes.
    #[test]
    fn a_lost_page_admission_answer_is_reconciled_by_count_and_ticket() {
        let panes = Arc::new(PaneTable::new());
        let inputs = InputQueue::new(panes, Arc::new(InputArbiter::new(EnterTiming::fixed(0))));
        inputs.begin_page("page-one").unwrap();
        let key = PaneKey::new("a-pane", 1);
        let (sender, received) = mpsc::sync_channel(4);
        let (replies, _received_replies) = mpsc::sync_channel(4);
        inputs.senders.lock().unwrap().insert(
            key.clone(),
            InputRoute {
                sender,
                replies,
                pending_bytes: Arc::new(AtomicUsize::new(0)),
                paste_gate: Arc::new(PasteGate::default()),
            },
        );
        let first = inputs
            .enqueue_page(
                "page-one",
                key.clone(),
                1,
                InputWork::Typed(b"A".to_vec()),
                true,
                None,
            )
            .unwrap();
        assert_eq!(
            inputs.expected_page_sequence("page-one", &key),
            Ok((2, Some(first.clone())))
        );
        assert_eq!(
            inputs.enqueue_page(
                "page-one",
                key.clone(),
                1,
                InputWork::Typed(b"A".to_vec()),
                true,
                None
            ),
            Err(INPUT_SEQUENCE_REGRESSION.to_owned())
        );
        assert_eq!(
            inputs.enqueue_page(
                "page-one",
                key.clone(),
                1,
                InputWork::Typed(b"B".to_vec()),
                true,
                None
            ),
            Err(INPUT_SEQUENCE_REGRESSION.to_owned())
        );
        let second = inputs
            .enqueue_page(
                "page-one",
                key.clone(),
                2,
                InputWork::Typed(b"B".to_vec()),
                true,
                None,
            )
            .unwrap();
        assert_ne!(second, first);
        assert!(
            matches!(received.try_recv().unwrap().work, InputWork::Typed(bytes) if bytes == b"A")
        );
        assert!(
            matches!(received.try_recv().unwrap().work, InputWork::Typed(bytes) if bytes == b"B")
        );
        assert!(matches!(
            received.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        inputs.begin_page("page-two").unwrap();
        assert_eq!(
            inputs.expected_page_sequence("page-two", &key),
            Ok((1, None))
        );
        assert_eq!(
            inputs.enqueue_page(
                "page-one",
                key.clone(),
                1,
                InputWork::Typed(b"A".to_vec()),
                true,
                None,
            ),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        assert_eq!(
            inputs.expected_page_sequence("page-one", &key),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        assert_eq!(
            inputs.begin_page("page-one"),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        inputs
            .enqueue_page(
                "page-two",
                key,
                1,
                InputWork::Typed(b"C".to_vec()),
                true,
                None,
            )
            .unwrap();
        assert!(
            matches!(received.try_recv().unwrap().work, InputWork::Typed(bytes) if bytes == b"C")
        );
        inputs.close_and_drain();
    }

    #[test]
    fn navigation_fences_a_page_before_the_next_subscription() {
        let inputs = InputQueue::new(
            Arc::new(PaneTable::new()),
            Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
        );
        let key = PaneKey::new("navigation-pane", 1);
        inputs.begin_page("session:2").unwrap();
        assert_eq!(
            inputs.begin_page("session:1"),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        inputs.end_page().unwrap();
        assert_eq!(
            inputs.enqueue_page(
                "session:2",
                key.clone(),
                1,
                InputWork::Typed(b"A".to_vec()),
                true,
                None
            ),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        assert_eq!(
            inputs.begin_page("session:1"),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        inputs.begin_page("session:3").unwrap();
        assert_eq!(
            inputs.expected_page_sequence("session:3", &key),
            Ok((1, None))
        );
        inputs.close_and_drain();
    }

    #[test]
    fn navigation_retires_pending_page_registration_and_late_activation() {
        let inputs = InputQueue::new(
            Arc::new(PaneTable::new()),
            Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
        );
        inputs.prepare_page("session:1").unwrap();
        inputs.end_page().unwrap();
        assert_eq!(
            inputs.activate_page("session:1"),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        // An old prepare that arrived only after navigation has no activation
        // from its departed page. A new activation retires it too.
        inputs.prepare_page("session:2").unwrap();
        inputs.prepare_page("session:3").unwrap();
        inputs.activate_page("session:3").unwrap();
        assert_eq!(
            inputs.activate_page("session:2"),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        assert_eq!(
            inputs.prepare_page("session:2"),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        inputs.prepare_page("session:3").unwrap();
        inputs.activate_page("session:3").unwrap();
        assert!(inputs.page_is_current("session:3").is_ok());
        inputs.close_and_drain();
    }

    #[test]
    fn navigation_cannot_overtake_a_page_output_subscription() {
        use std::time::Duration;

        let inputs = Arc::new(InputQueue::new(
            Arc::new(PaneTable::new()),
            Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
        ));
        inputs.begin_page("old").unwrap();
        let (entered, has_entered) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let subscribing = {
            let inputs = Arc::clone(&inputs);
            thread::spawn(move || {
                inputs.with_current_page("old", || {
                    entered.send(()).unwrap();
                    released.recv().unwrap();
                })
            })
        };
        has_entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let (ended, ending) = mpsc::channel();
        let navigating = {
            let inputs = Arc::clone(&inputs);
            thread::spawn(move || ended.send(inputs.end_page()).unwrap())
        };
        let early = ending.recv_timeout(Duration::from_millis(100));
        release.send(()).unwrap();
        subscribing.join().unwrap().unwrap();
        if early.is_err() {
            ending
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap();
        }
        navigating.join().unwrap();
        assert!(early.is_err(), "navigation overtook an active subscription");
        assert_eq!(
            inputs.page_is_current("old"),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        inputs.close_and_drain();
    }

    #[test]
    fn a_refused_admission_does_not_consume_a_number() {
        let inputs = InputQueue::new(
            Arc::new(PaneTable::new()),
            Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
        );
        inputs.begin_page("page").unwrap();
        let key = PaneKey::new("full-pane", 1);
        let (sender, _received) = mpsc::sync_channel(0);
        let (replies, _received_replies) = mpsc::sync_channel(0);
        inputs.senders.lock().unwrap().insert(
            key.clone(),
            InputRoute {
                sender,
                replies,
                pending_bytes: Arc::new(AtomicUsize::new(0)),
                paste_gate: Arc::new(PasteGate::default()),
            },
        );
        let typed = || InputWork::Typed(b"A".to_vec());
        assert_eq!(
            inputs.enqueue_page("page", key.clone(), 1, typed(), true, None),
            Err("the pane's input queue is full".to_string())
        );
        assert_eq!(inputs.expected_page_sequence("page", &key), Ok((1, None)));
        assert_eq!(
            inputs.enqueue_page("page", key.clone(), 1, typed(), true, None),
            Err("the pane's input queue is full".to_string())
        );
        assert_eq!(inputs.page.lock().unwrap().completions.len(), 0);
        inputs.close_and_drain();
    }

    #[test]
    fn a_lost_completion_answer_can_be_waited_for_again() {
        let inputs = InputQueue::new(
            Arc::new(PaneTable::new()),
            Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
        );
        inputs.begin_page("page").unwrap();
        let key = PaneKey::new("completion-pane", 1);
        let (sender, received) = mpsc::sync_channel(1);
        let (replies, _received_replies) = mpsc::sync_channel(1);
        inputs.senders.lock().unwrap().insert(
            key.clone(),
            InputRoute {
                sender,
                replies,
                pending_bytes: Arc::new(AtomicUsize::new(0)),
                paste_gate: Arc::new(PasteGate::default()),
            },
        );
        let ticket = inputs
            .enqueue_page("page", key, 1, InputWork::Typed(b"A".to_vec()), true, None)
            .unwrap();
        received.try_recv().unwrap().response.send(Ok(())).unwrap();
        let first = inputs.page_completion("page", &ticket).unwrap();
        assert!(futures_executor::block_on(first.receiver.clone()).is_ok());
        assert!(first.notify_human_once());
        let again = inputs.page_completion("page", &ticket).unwrap();
        assert!(futures_executor::block_on(again.receiver.clone()).is_ok());
        assert!(!again.notify_human_once());
        inputs.forget_page_completion("page", &ticket).unwrap();
        inputs.forget_page_completion("page", &ticket).unwrap();
        assert!(inputs.page_completion("page", &ticket).is_err());
        inputs.close_and_drain();
    }

    #[test]
    fn unanswered_page_input_is_bounded_until_a_new_page() {
        let inputs = InputQueue::new(
            Arc::new(PaneTable::new()),
            Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
        );
        inputs.begin_page("page-one").unwrap();
        let key = PaneKey::new("unanswered-pane", 1);
        let (sender, _received) = mpsc::sync_channel(MAX_PENDING_INPUT_TICKETS + 1);
        let (replies, _received_replies) = mpsc::sync_channel(1);
        inputs.senders.lock().unwrap().insert(
            key.clone(),
            InputRoute {
                sender,
                replies,
                pending_bytes: Arc::new(AtomicUsize::new(0)),
                paste_gate: Arc::new(PasteGate::default()),
            },
        );
        for sequence in 1..=MAX_PENDING_INPUT_TICKETS as u64 {
            inputs
                .enqueue_page(
                    "page-one",
                    key.clone(),
                    sequence,
                    InputWork::Write(b"x".to_vec()),
                    true,
                    None,
                )
                .unwrap();
        }
        assert_eq!(
            inputs.enqueue_page(
                "page-one",
                key.clone(),
                MAX_PENDING_INPUT_TICKETS as u64 + 1,
                InputWork::Write(b"x".to_vec()),
                true,
                None
            ),
            Err("pane-input-ticket-capacity".to_string())
        );
        assert_eq!(
            inputs.expected_page_sequence("page-one", &key).unwrap().0,
            MAX_PENDING_INPUT_TICKETS as u64 + 1
        );
        inputs.begin_page("page-two").unwrap();
        assert!(inputs
            .enqueue_page(
                "page-two",
                key,
                1,
                InputWork::Write(b"x".to_vec()),
                true,
                None
            )
            .is_ok());
        inputs.close_and_drain();
    }
}
