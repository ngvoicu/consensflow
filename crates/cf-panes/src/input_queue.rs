//! Every open pane's input, in order: one worker and one bounded queue per
//! pane, and a second for the terminal's replies, the page's numbered input
//! admitted before it is answered, and why an input did not go in.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};

use futures_channel::oneshot;
use futures_util::future::{BoxFuture, FutureExt, Shared};
use serde_json::{json, Value};

use crate::arbiter::{ArbiterError, InputArbiter};
use crate::pty::{PaneKey, PaneTable};
use crate::validation::{validate_page_input, INVALID_BODY};

const INPUT_QUEUE_CAPACITY: usize = 1024;
const MAX_PENDING_INPUT_BYTES_PER_PANE: usize = 4 * 1024 * 1024;
const MAX_PENDING_INPUT_TICKETS: usize = 4096;
const INPUT_QUEUE_FULL: &str = "pane-input-queue-full";
/// The host is stopping, and takes no more input.
const INPUT_CLOSED: &str = "input-closed";
const LOCK_POISONED: &str = "lock-poisoned";
/// A call numbered at or below what the pane's count has passed: a late
/// original after its skip, or a repeat. Nothing of it is written.
const INPUT_SEQUENCE_STALE: &str = "pane-input-sequence-stale";
const INPUT_STALE_PAGE: &str = "pane-input-stale-page";
const INPUT_FAILED: &str = "input-failed";

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
/// it may be sent again. Uncertain: some of it may have reached the window.
/// A failed write is uncertain and also holds this generation until restart.
#[derive(Clone, Debug)]
pub enum InputError {
    Refused { code: &'static str, cause: String },
    Uncertain(String),
    FailedWrite(String),
}

impl InputError {
    /// Whether some of this input may already have reached the pane.
    pub fn is_uncertain(&self) -> bool {
        matches!(self, Self::Uncertain(_) | Self::FailedWrite(_))
    }

    /// Whether the pane's generation refuses later input until replaced.
    pub fn failed_until_restart(&self) -> bool {
        matches!(
            self,
            Self::FailedWrite(_)
                | Self::Refused {
                    code: INPUT_FAILED,
                    ..
                }
        )
    }

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
            Self::Uncertain(cause) | Self::FailedWrite(cause) => json!({
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
            Self::Refused { cause, .. } | Self::Uncertain(cause) | Self::FailedWrite(cause) => {
                formatter.write_str(cause)
            }
        }
    }
}

impl From<ArbiterError> for InputError {
    fn from(error: ArbiterError) -> Self {
        let code = match &error {
            ArbiterError::Pane(_) => return Self::FailedWrite(error.to_string()),
            ArbiterError::Stale => "stale-pane",
            ArbiterError::Busy => "paste-in-flight",
            ArbiterError::InputFailed => INPUT_FAILED,
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
    response: oneshot::Sender<InputResponse>,
    reserved_bytes: usize,
    pending_bytes: Arc<AtomicUsize>,
}

#[derive(Clone)]
pub struct PageInputCompletion {
    pub receiver: Shared<BoxFuture<'static, InputResponse>>,
    pub human: bool,
    key: PaneKey,
    sequence: u64,
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
    completions: HashMap<String, PageInputCompletion>,
    next_ticket: u64,
}

struct InputRoute {
    sender: mpsc::SyncSender<InputJob>,
    /// The terminal's replies, to a worker of their own ([`InputWork::Reply`]).
    replies: mpsc::SyncSender<InputJob>,
    pending_bytes: Arc<AtomicUsize>,
}

/// Every open pane's input, in order: one worker and one bounded queue per
/// pane, from its `pane.open` until it leaves the table.
pub struct InputQueue {
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    senders: Mutex<HashMap<PaneKey, InputRoute>>,
    workers: Mutex<HashMap<PaneKey, Vec<JoinHandle<()>>>>,
    page: Mutex<PageInputState>,
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
                completions: HashMap::new(),
                next_ticket: 1,
            }),
            accepting: AtomicBool::new(true),
        }
    }

    /// Admit one numbered call from the active page: a key, a paste whole, or
    /// a terminal reply. Only a successful admission advances the count.
    pub fn enqueue_page(
        &self,
        page_id: &str,
        key: PaneKey,
        sequence: u64,
        work: InputWork,
        human: bool,
    ) -> Result<String, String> {
        let mut page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        if page.current.as_deref() != Some(page_id) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        // The count only goes up. A call at or below it is one the host has
        // passed, by admitting it or by a skip: it writes nothing, so a late
        // original can never type a key the page has already given up on.
        // Anything above it is admitted (a skip that never came would
        // otherwise leave every later key refused).
        let passed = page.last_sequences.get(&key).copied().unwrap_or(0);
        if sequence <= passed {
            return Err(INPUT_SEQUENCE_STALE.to_string());
        }
        match &work {
            InputWork::Typed(bytes)
            | InputWork::Write(bytes)
            | InputWork::Reply(bytes)
            | InputWork::Paste { body: bytes, .. } => validate_page_input(bytes)?,
            InputWork::Claim => {}
        }
        if page.completions.len() >= MAX_PENDING_INPUT_TICKETS {
            return Err("pane-input-ticket-capacity".to_string());
        }
        let next_ticket = page
            .next_ticket
            .checked_add(1)
            .ok_or_else(|| "pane-input-ticket-overflow".to_string())?;
        let receiver = self
            .submit(key.clone(), work)
            .map_err(|error| error.to_string())?;
        let ticket = format!("pane-input-{}", page.next_ticket);
        page.next_ticket = next_ticket;
        page.last_sequences.insert(key.clone(), sequence);
        page.completions.insert(
            ticket.clone(),
            PageInputCompletion {
                receiver: async move { wait_for_input(receiver).await }
                    .boxed()
                    .shared(),
                human,
                key,
                sequence,
                notified: Arc::new(AtomicBool::new(false)),
            },
        );
        Ok(ticket)
    }

    /// A call of the active page ended with its fate unknown (a timeout, a
    /// lost answer), and the page will not send it again. Move the pane's
    /// count past it: the host keeps the higher of its count and `sequence`,
    /// so the original either went in before this or is refused as stale when
    /// it comes late. A completion the page never learned of is dropped.
    pub fn skip_page_sequence(
        &self,
        page_id: &str,
        key: &PaneKey,
        sequence: u64,
    ) -> Result<(), String> {
        let mut page = self
            .page
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())?;
        if page.current.as_deref() != Some(page_id) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        if sequence == u64::MAX {
            return Err("pane-input-sequence-overflow".to_string());
        }
        let passed = page.last_sequences.entry(key.clone()).or_insert(0);
        *passed = (*passed).max(sequence);
        page.completions
            .retain(|_, completion| !(&completion.key == key && completion.sequence == sequence));
        Ok(())
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
        page.completions.clear();
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
        let mut workers = Vec::with_capacity(2);
        for (name, jobs) in [("input", jobs), ("replies", replied)] {
            let panes = Arc::clone(&self.panes);
            let arbiter = Arc::clone(&self.arbiter);
            let worker_key = key.clone();
            let worker = thread::Builder::new()
                .name(format!("consensflow-{name}-{}", worker_key.id))
                .spawn(move || input_worker(jobs, panes, arbiter, worker_key))
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
        let job = InputJob {
            work,
            response,
            reserved_bytes,
            pending_bytes: Arc::clone(&route.pending_bytes),
        };
        let sender = match job.work {
            InputWork::Reply(_) => &route.replies,
            _ => &route.sender,
        };
        match sender.try_send(job) {
            Ok(()) => Ok(receiver),
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

fn input_worker(
    jobs: mpsc::Receiver<InputJob>,
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    key: PaneKey,
) {
    for job in jobs {
        let result = match job.work {
            InputWork::Typed(bytes) => arbiter.write_typed(&panes, &key, &bytes),
            InputWork::Write(bytes) | InputWork::Reply(bytes) => {
                arbiter.write(&panes, &key, &bytes)
            }
            InputWork::Paste { body, read_empty } => {
                arbiter.write_paste(&panes, &key, &body, read_empty)
            }
            InputWork::Claim => arbiter.claim(&key),
        }
        .map_err(InputError::from);
        job.pending_bytes
            .fetch_sub(job.reserved_bytes, Ordering::AcqRel);
        let _ = job.response.send(result);
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

    /// The pane's count: the last number the host has passed.
    fn passed(inputs: &InputQueue, key: &PaneKey) -> u64 {
        inputs
            .page
            .lock()
            .unwrap()
            .last_sequences
            .get(key)
            .copied()
            .unwrap_or(0)
    }
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

    #[test]
    fn uncertainty_and_a_failed_pane_are_distinct() {
        let (sender, receiver) = oneshot::channel();
        drop(sender);
        let unanswered = futures_executor::block_on(wait_for_input(receiver)).unwrap_err();
        assert!(unanswered.is_uncertain());
        assert!(!unanswered.failed_until_restart());

        let failed_write = InputError::from(ArbiterError::Pane(PaneError::Io(
            std::io::Error::other("test write failure"),
        )));
        assert!(failed_write.is_uncertain());
        assert!(failed_write.failed_until_restart());
        let refused = InputError::from(ArbiterError::InputFailed);
        assert!(!refused.is_uncertain());
        assert!(refused.failed_until_restart());
        let ordinary = InputError::from(ArbiterError::Stale);
        assert!(!ordinary.is_uncertain());
        assert!(!ordinary.failed_until_restart());
    }

    /// A call whose fate is unknown is skipped, never sent again: the count
    /// moves past it, a late original is refused and writes nothing, and the
    /// next number goes on.
    #[test]
    fn a_skipped_call_is_never_written_when_its_original_comes_late() {
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
            },
        );
        let typed = |text: &[u8]| InputWork::Typed(text.to_vec());
        inputs
            .enqueue_page("page-one", key.clone(), 1, typed(b"A"), true)
            .unwrap();
        // An admitted number is passed at once: a repeat of it is stale.
        assert_eq!(
            inputs.enqueue_page("page-one", key.clone(), 1, typed(b"A"), true),
            Err(INPUT_SEQUENCE_STALE.to_owned())
        );
        // Call 2 timed out at the page: skipped. Its original comes late.
        inputs.skip_page_sequence("page-one", &key, 2).unwrap();
        assert_eq!(passed(&inputs, &key), 2);
        assert_eq!(
            inputs.enqueue_page("page-one", key.clone(), 2, typed(b"B"), true),
            Err(INPUT_SEQUENCE_STALE.to_owned())
        );
        // A repeat of an admitted number is refused the same way.
        assert_eq!(
            inputs.enqueue_page("page-one", key.clone(), 1, typed(b"A"), true),
            Err(INPUT_SEQUENCE_STALE.to_owned())
        );
        // A skip below the count moves nothing: the host keeps the higher.
        inputs.skip_page_sequence("page-one", &key, 2).unwrap();
        inputs.skip_page_sequence("page-one", &key, 0).unwrap();
        assert_eq!(passed(&inputs, &key), 2);
        // A skip for an original that was admitted: no completion is left for
        // the answer the page never read.
        inputs
            .enqueue_page("page-one", key.clone(), 3, typed(b"C"), true)
            .unwrap();
        assert_eq!(inputs.page.lock().unwrap().completions.len(), 2);
        inputs.skip_page_sequence("page-one", &key, 3).unwrap();
        assert_eq!(inputs.page.lock().unwrap().completions.len(), 1);
        inputs
            .enqueue_page("page-one", key.clone(), 4, typed(b"D"), true)
            .unwrap();
        let written: Vec<_> = std::iter::from_fn(|| received.try_recv().ok())
            .map(|job| match job.work {
                InputWork::Typed(bytes) => bytes,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            written,
            vec![b"A".to_vec(), b"C".to_vec(), b"D".to_vec()],
            "B, skipped before it came, writes nothing"
        );
        inputs.begin_page("page-two").unwrap();
        assert_eq!(
            inputs.skip_page_sequence("page-one", &key, 9),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        assert_eq!(passed(&inputs, &key), 0);
        assert_eq!(
            inputs.enqueue_page("page-one", key.clone(), 1, typed(b"A"), true),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        assert_eq!(
            inputs.begin_page("page-one"),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        inputs
            .enqueue_page("page-two", key, 1, typed(b"E"), true)
            .unwrap();
        assert!(
            matches!(received.try_recv().unwrap().work, InputWork::Typed(bytes) if bytes == b"E")
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
                true
            ),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        assert_eq!(
            inputs.begin_page("session:1"),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        inputs.begin_page("session:3").unwrap();
        assert_eq!(passed(&inputs, &key), 0);
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
            },
        );
        let typed = || InputWork::Typed(b"A".to_vec());
        assert_eq!(
            inputs.enqueue_page("page", key.clone(), 1, typed(), true),
            Err("the pane's input queue is full".to_string())
        );
        assert_eq!(passed(&inputs, &key), 0);
        assert_eq!(
            inputs.enqueue_page("page", key.clone(), 1, typed(), true),
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
            },
        );
        let ticket = inputs
            .enqueue_page("page", key, 1, InputWork::Typed(b"A".to_vec()), true)
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
                )
                .unwrap();
        }
        assert_eq!(
            inputs.enqueue_page(
                "page-one",
                key.clone(),
                MAX_PENDING_INPUT_TICKETS as u64 + 1,
                InputWork::Write(b"x".to_vec()),
                true
            ),
            Err("pane-input-ticket-capacity".to_string())
        );
        assert_eq!(passed(&inputs, &key), MAX_PENDING_INPUT_TICKETS as u64);
        inputs.begin_page("page-two").unwrap();
        assert!(inputs
            .enqueue_page("page-two", key, 1, InputWork::Write(b"x".to_vec()), true)
            .is_ok());
        inputs.close_and_drain();
    }
}
