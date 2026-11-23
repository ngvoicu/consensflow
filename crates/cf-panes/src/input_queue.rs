//! Every open pane's input, in order: one worker and one bounded queue per
//! pane, and a second for the terminal's replies, the page's numbered input
//! admitted before it is answered, and why an input did not go in.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use futures_channel::oneshot;
use futures_util::future::{BoxFuture, FutureExt, Shared};
use serde_json::{json, Value};

use crate::arbiter::{ArbiterError, InputArbiter};
use crate::pty::{PaneKey, PaneTable};
use crate::validation::{validate_page_input, INVALID_BODY};

const INPUT_QUEUE_CAPACITY: usize = 1024;
/// What a window holds of the human's pending input: a paste of the largest
/// size being written, and what is typed behind it.
const MAX_PENDING_INPUT_BYTES_PER_PANE: usize = 8 * 1024 * 1024;
/// The terminal's replies have room of their own, so that a pending paste never
/// refuses one.
const MAX_PENDING_REPLY_BYTES_PER_PANE: usize = 64 * 1024;
/// How long a call waits in the host for the numbers before it.
pub const ORDER_BOUND: Duration = Duration::from_millis(1500);
/// What the page may hold for one window while it waits for the numbers before it, whatever
/// the bytes: a number refused or skipped is remembered only until the order passes it.
#[cfg(not(test))]
const MAX_LANE_SLOTS: usize = 8192;
#[cfg(test)]
const MAX_LANE_SLOTS: usize = 64;
/// How many windows' unshown notices the host keeps.
const MAX_PENDING_LOSSES: usize = 512;
const MAX_PENDING_INPUT_TICKETS: usize = 4096;
const INPUT_QUEUE_FULL: &str = "pane-input-queue-full";
/// The host is stopping, and takes no more input.
const INPUT_CLOSED: &str = "input-closed";
const LOCK_POISONED: &str = "lock-poisoned";
/// A call numbered at or below what the pane's count has passed: a late
/// original after its skip, or a repeat. Nothing of it is written.
const INPUT_SEQUENCE_STALE: &str = "pane-input-sequence-stale";
const INPUT_STALE_PAGE: &str = "pane-input-stale-page";
/// A skip whose number cannot be spent yet: the page asks again, and the skip stays pending.
pub const SKIP_PENDING: &str = "pane-input-skip-pending";
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

/// Bytes counted against a pane's pending input until its worker is done with
/// them (or the call is refused): given back when this is dropped.
struct Reserved {
    counter: Arc<AtomicUsize>,
    bytes: usize,
}

impl Drop for Reserved {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

/// A page job's one host-owned outcome. The host settles it itself, under its lock, whatever
/// becomes of the page's wait, ticket or window: written (nothing to say), refused, or
/// uncertain. A job dropped with none (its worker vanished, its queue was dropped whole) is
/// uncertain, and the drop records that.
struct Outcome {
    shared: Weak<PageShared>,
    key: PaneKey,
    /// The number of the loss the host recorded for this call, for its wait to answer.
    loss: Arc<AtomicU64>,
    settled: bool,
}

impl Outcome {
    fn new(shared: &Arc<PageShared>, key: &PaneKey, loss: Arc<AtomicU64>) -> Self {
        Self {
            shared: Arc::downgrade(shared),
            key: key.clone(),
            loss,
            settled: false,
        }
    }

    /// Recorded under the host's lock: the call did not go in whole, or may not have.
    fn lose(&mut self, state: &mut PageInputState, closed: bool, events: &mut Vec<Event>) -> u64 {
        let loss = state.lost(&self.key, closed, events);
        self.loss.store(loss, Ordering::Release);
        self.settled = true;
        loss
    }

    /// The worker's answer. A failed or uncertain write is recorded before it is answered,
    /// so that the page's wait, a page that has gone and a ticket cleared all leave the
    /// same notice.
    fn finish(mut self, result: InputResponse, response: oneshot::Sender<InputResponse>) {
        let shared = self.shared.upgrade();
        match (shared, result.is_err()) {
            (Some(shared), true) => {
                let mut events = Vec::new();
                {
                    let mut state = shared
                        .state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    self.lose(&mut state, false, &mut events);
                    let _ = response.send(result);
                }
                shared.flush(events);
                shared.wake.notify_all();
            }
            _ => {
                self.settled = true;
                let _ = response.send(result);
            }
        }
    }
}

impl Drop for Outcome {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        let key = self.key.clone();
        let cell = Arc::clone(&self.loss);
        let record = move |shared: Arc<PageShared>| {
            let mut events = Vec::new();
            {
                let mut state = shared
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let loss = state.lost(&key, false, &mut events);
                cell.store(loss, Ordering::Release);
            }
            shared.flush(events);
            shared.wake.notify_all();
        };
        // Inline when the lock is free, so that the wait that this drop answers finds the
        // loss; on a thread when a holder (which may be this one) has it.
        let free = shared.state.try_lock().map(drop).is_ok();
        if free {
            record(shared);
        } else {
            let _ = thread::Builder::new()
                .name("consensflow-input-outcome".to_owned())
                .spawn(move || record(shared));
        }
    }
}

struct InputJob {
    work: InputWork,
    response: oneshot::Sender<InputResponse>,
    /// Held until the job is done, or refused before it was written.
    _reserved: Reserved,
    /// A page job's outcome; a native submission has none.
    outcome: Option<Outcome>,
}

impl InputJob {
    /// Nothing of it was written, and nothing is owed to the page (the host is stopping).
    fn abandon(mut self, error: InputError) {
        if let Some(outcome) = self.outcome.as_mut() {
            outcome.settled = true;
        }
        let _ = self.response.send(Err(error));
    }

    /// Nothing of it was written, and the host has recorded loss number `loss` for it.
    fn refuse_told(mut self, loss: u64, error: InputError) {
        if let Some(outcome) = self.outcome.as_mut() {
            outcome.loss.store(loss, Ordering::Release);
            outcome.settled = true;
        }
        let _ = self.response.send(Err(error));
    }
}

/// What the host says of a page's input beyond answering it. It never carries a
/// byte of what was typed.
pub trait InputObserver: Send + Sync {
    /// Typing may not have reached this window (loss number `loss`, the window's
    /// latest): told to the page that is current, which says so and confirms it.
    fn lost(&self, key: &PaneKey, loss: u64);
    /// A line for the host's own log (R01): a number given up at its bound, a
    /// late call refused, and how late.
    fn say(&self, line: &str);
}

enum Event {
    Lost(PaneKey, u64),
    Say(String),
}

/// A call the host refused: why, and for the page that is current the loss it recorded.
#[derive(Debug)]
pub struct Refusal {
    pub error: String,
    pub loss: Option<u64>,
}

impl From<String> for Refusal {
    fn from(error: String) -> Self {
        Self { error, loss: None }
    }
}

#[derive(Clone)]
pub struct PageInputCompletion {
    pub receiver: Shared<BoxFuture<'static, InputResponse>>,
    pub human: bool,
    key: PaneKey,
    sequence: u64,
    notified: Arc<AtomicBool>,
    loss: Arc<AtomicU64>,
}

impl PageInputCompletion {
    /// A retried wait may finish in more than one caller; tell the page's
    /// state about a successful human write only once.
    pub fn notify_human_once(&self) -> bool {
        self.human && !self.notified.swap(true, Ordering::AcqRel)
    }

    /// The number of the loss the host recorded for this call, once it is told as lost.
    pub fn loss(&self) -> Option<u64> {
        match self.loss.load(Ordering::Acquire) {
            0 => None,
            loss => Some(loss),
        }
    }
}

/// A window whose typing may not have arrived: the notice's version, and whether the window
/// has closed for good (its notice then names it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LossNotice {
    pub key: PaneKey,
    pub loss: u64,
    pub closed: bool,
}

/// The losses past the bound, counted: its own version, and how many further records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverflowNotice {
    pub version: u64,
    pub count: u64,
}

/// Everything the host still owes the person.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PendingNotices {
    pub losses: Vec<LossNotice>,
    pub overflow: Option<OverflowNotice>,
}

#[derive(Clone, Copy)]
struct LossRecord {
    version: u64,
    closed: bool,
}

/// A number of the page's, waiting for the numbers before it.
enum Slot {
    /// A call that came early. It holds its bytes, and the worker it goes to.
    Parked {
        job: InputJob,
        sender: mpsc::SyncSender<InputJob>,
        deadline: Instant,
    },
    /// A number given up on (the page skipped it) or refused: nothing is
    /// written for it and the numbers after it go on.
    Void { deadline: Instant },
}

impl Slot {
    fn deadline(&self) -> Instant {
        match self {
            Self::Parked { deadline, .. } | Self::Void { deadline } => *deadline,
        }
    }
}

/// The current page's numbers for one window.
#[derive(Default)]
struct Lane {
    /// The numbers up to this one are settled: handed to the writer, refused,
    /// skipped or given up.
    passed: u64,
    slots: BTreeMap<u64, Slot>,
    /// The last numbers given up at their bound, and when.
    gave_up: VecDeque<(u64, Instant)>,
    /// The last skips, each with the loss recorded for it: a retried skip answers the same.
    skipped: VecDeque<(u64, u64)>,
}

struct PageInputState {
    /// The host's own count of main documents, moved only by navigation.
    epoch: u64,
    current: Option<String>,
    /// The nonce the current page prepared with.
    current_nonce: String,
    retired: HashSet<String>,
    /// Pages that prepared, with the nonce and epoch the host answered them.
    pending_pages: HashMap<String, (String, u64)>,
    page_ordinals: HashMap<String, u64>,
    lanes: HashMap<PaneKey, Lane>,
    completions: HashMap<String, PageInputCompletion>,
    next_ticket: u64,
    /// By window, its latest loss, until a page says it has shown that version.
    pending_lost: HashMap<PaneKey, LossRecord>,
    overflow: Option<OverflowNotice>,
    next_loss: u64,
    reaper: bool,
}

struct PageShared {
    state: Mutex<PageInputState>,
    wake: Condvar,
    observer: Mutex<Option<Arc<dyn InputObserver>>>,
}

impl PageShared {
    fn flush(&self, events: Vec<Event>) {
        if events.is_empty() {
            return;
        }
        let observer = self
            .observer
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let Some(observer) = observer else { return };
        for event in events {
            match event {
                Event::Lost(key, loss) => observer.lost(&key, loss),
                Event::Say(line) => observer.say(&line),
            }
        }
    }
}

impl PageInputState {
    /// Record that typing may be missing from this window. Kept until a page says it has
    /// shown this version; the page that is current is told at once, and one that is not
    /// yet, at its activation.
    fn lost(&mut self, key: &PaneKey, closed: bool, events: &mut Vec<Event>) -> u64 {
        self.next_loss += 1;
        let loss = self.next_loss;
        let closed = closed || self.pending_lost.get(key).is_some_and(|record| record.closed);
        self.pending_lost.insert(
            key.clone(),
            LossRecord {
                version: loss,
                closed,
            },
        );
        if self.pending_lost.len() > MAX_PENDING_LOSSES {
            if let Some(oldest) = self
                .pending_lost
                .iter()
                .min_by_key(|(_, record)| record.version)
                .map(|(key, _)| key.clone())
            {
                self.pending_lost.remove(&oldest);
                // Past the bound a record is counted, with a version of its own.
                self.next_loss += 1;
                let count = self.overflow.map_or(0, |overflow| overflow.count) + 1;
                self.overflow = Some(OverflowNotice {
                    version: self.next_loss,
                    count,
                });
            }
        }
        if self.current.is_some() {
            events.push(Event::Lost(key.clone(), loss));
        }
        loss
    }

    fn notices(&self) -> PendingNotices {
        let mut losses: Vec<LossNotice> = self
            .pending_lost
            .iter()
            .map(|(key, record)| LossNotice {
                key: key.clone(),
                loss: record.version,
                closed: record.closed,
            })
            .collect();
        losses.sort_by_key(|notice| notice.loss);
        PendingNotices {
            losses,
            overflow: self.overflow,
        }
    }

    /// What the page has shown: those windows' versions, and the overflow's count at its
    /// version. A later loss of the same window stays, and so does an eviction since.
    fn told(&mut self, told: &[(PaneKey, u64)], overflow: Option<(u64, u64)>) {
        for (key, loss) in told {
            if self
                .pending_lost
                .get(key)
                .is_some_and(|record| record.version <= *loss)
            {
                self.pending_lost.remove(key);
            }
        }
        if let Some((version, count)) = overflow {
            if let Some(held) = self.overflow {
                if held.version <= version {
                    self.overflow = None;
                } else if held.count > count {
                    self.overflow = Some(OverflowNotice {
                        version: held.version,
                        count: held.count - count,
                    });
                } else {
                    self.overflow = None;
                }
            }
        }
    }

    /// Whether a call is of the current page and the current epoch.
    fn admits(&self, page_id: &str, epoch: u64) -> bool {
        self.epoch == epoch && self.current.as_deref() == Some(page_id)
    }

    fn lane(&mut self, key: &PaneKey) -> &mut Lane {
        self.lanes.entry(key.clone()).or_default()
    }

    /// The soonest moment something must be settled that nothing else will
    /// settle: a call behind a gap reaches its own deadline.
    fn next_deadline(&self) -> Option<Instant> {
        let mut next: Option<Instant> = None;
        let mut soonest = |at: Instant| next = Some(next.map_or(at, |old| old.min(at)));
        for lane in self.lanes.values() {
            let mut expected = lane.passed + 1;
            let mut gap = false;
            for (number, slot) in &lane.slots {
                if !gap && *number == expected {
                    expected += 1;
                } else {
                    gap = true;
                    soonest(slot.deadline());
                }
            }
        }
        next
    }

    /// Hand to the writers what is in order, and give up what a bound has passed over.
    /// Returns the numbers whose worker refused them, each with the loss recorded.
    fn pump(
        &mut self,
        key: &PaneKey,
        now: Instant,
        events: &mut Vec<Event>,
    ) -> Vec<(u64, InputError, u64)> {
        let mut refusals: Vec<(u64, InputJob, InputError, bool)> = Vec::new();
        let mut gave_up = false;
        let mut said = Vec::new();
        {
            let Some(lane) = self.lanes.get_mut(key) else {
                return Vec::new();
            };
            // A call waits for the numbers before it for the bound since it came, whatever
            // came after it: the highest call whose wait is over carries every gap below it.
            let expired_to = lane
                .slots
                .iter()
                .filter(|(_, slot)| slot.deadline() <= now)
                .map(|(number, _)| *number)
                .max();
            let earliest = lane.slots.values().map(Slot::deadline).min();
            while let Some(first) = lane.slots.first_key_value().map(|(number, _)| *number) {
                if first > lane.passed + 1 {
                    if expired_to.is_some_and(|expired| expired >= first) {
                        let waited = earliest.map_or(ORDER_BOUND, |earliest| {
                            now.saturating_duration_since(earliest - ORDER_BOUND)
                        });
                        lane.give_up(first - 1, now, waited, key, &mut said);
                        gave_up = true;
                        continue;
                    }
                    break;
                }
                let Some(slot) = lane.slots.remove(&first) else {
                    break;
                };
                lane.passed = first;
                if let Slot::Parked { job, sender, .. } = slot {
                    match sender.try_send(job) {
                        Ok(()) => {}
                        Err(mpsc::TrySendError::Full(job)) => {
                            refusals.push((first, job, InputError::queue_full(), false));
                        }
                        Err(mpsc::TrySendError::Disconnected(job)) => {
                            let error = InputError::refused(
                                INPUT_CLOSED,
                                format!("pane input queue for {} is closed", key.id),
                            );
                            refusals.push((first, job, error, true));
                        }
                    }
                }
            }
        }
        events.extend(said.into_iter().map(Event::Say));
        let mut refused = Vec::new();
        for (number, job, error, closed) in refusals {
            let loss = self.lost(key, closed, events);
            job.refuse_told(loss, error.clone());
            refused.push((number, error, loss));
        }
        if gave_up {
            self.lost(key, false, events);
        }
        refused
    }

    fn pump_all(&mut self, now: Instant, events: &mut Vec<Event>) {
        let keys: Vec<PaneKey> = self.lanes.keys().cloned().collect();
        for key in keys {
            self.pump(&key, now, events);
        }
    }

    /// A new main document: nothing of the old page's still in order goes in. Each parked
    /// call is cancelled as a loss, and so is a known missing number below the numbers seen.
    /// What was handed to a window's writer stays written, in order.
    fn cut(&mut self, events: &mut Vec<Event>) {
        let lanes = std::mem::take(&mut self.lanes);
        for (key, lane) in lanes {
            if lane.slots.is_empty() {
                continue;
            }
            let loss = self.lost(&key, false, events);
            for slot in lane.slots.into_values() {
                if let Slot::Parked { job, .. } = slot {
                    job.refuse_told(
                        loss,
                        InputError::refused(INPUT_STALE_PAGE, "the page reloaded before this was sent"),
                    );
                }
            }
        }
        if let Some(previous) = self.current.take() {
            self.retired.insert(previous);
        }
        self.current_nonce.clear();
        self.retired.extend(
            std::mem::take(&mut self.pending_pages)
                .into_keys(),
        );
        self.completions.clear();
    }
}

impl Lane {
    /// Give up the numbers after `passed` up to and including `upto`: they
    /// never came.
    fn give_up(
        &mut self,
        upto: u64,
        now: Instant,
        waited: Duration,
        key: &PaneKey,
        said: &mut Vec<String>,
    ) {
        if upto <= self.passed {
            return;
        }
        let from = self.passed + 1;
        for number in (from..=upto).rev().take(64) {
            self.gave_up.push_back((number, now));
        }
        while self.gave_up.len() > 64 {
            self.gave_up.pop_front();
        }
        self.passed = upto;
        said.push(format!(
            "pane input: gave up numbers {from} to {upto} for window {} (generation {}): they had not come after {} ms",
            key.id,
            key.generation,
            waited.as_millis()
        ));
    }
}

struct InputRoute {
    sender: mpsc::SyncSender<InputJob>,
    /// The terminal's replies, to a worker of their own ([`InputWork::Reply`]).
    replies: mpsc::SyncSender<InputJob>,
    /// The keys' pending input, and the replies' own.
    pending_bytes: Arc<AtomicUsize>,
    reply_bytes: Arc<AtomicUsize>,
}

impl InputRoute {
    /// Count a call's bytes against the window's keys, or against its replies'
    /// own room.
    fn reserve(&self, work: &InputWork) -> Result<Reserved, InputError> {
        let (counter, limit) = match work {
            InputWork::Reply(_) => (&self.reply_bytes, MAX_PENDING_REPLY_BYTES_PER_PANE),
            _ => (&self.pending_bytes, MAX_PENDING_INPUT_BYTES_PER_PANE),
        };
        let bytes = work.byte_count();
        counter
            .try_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                pending.checked_add(bytes).filter(|next| *next <= limit)
            })
            .map_err(|_| InputError::queue_full())?;
        Ok(Reserved {
            counter: Arc::clone(counter),
            bytes,
        })
    }

    fn sender_for(&self, work: &InputWork) -> &mpsc::SyncSender<InputJob> {
        match work {
            InputWork::Reply(_) => &self.replies,
            _ => &self.sender,
        }
    }
}

/// Every open pane's input, in order: one worker and one bounded queue per
/// pane, from its `pane.open` until it leaves the table.
pub struct InputQueue {
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    senders: Mutex<HashMap<PaneKey, InputRoute>>,
    workers: Mutex<HashMap<PaneKey, Vec<JoinHandle<()>>>>,
    page: Arc<PageShared>,
    accepting: AtomicBool,
}

impl InputQueue {
    pub fn new(panes: Arc<PaneTable>, arbiter: Arc<InputArbiter>) -> Self {
        Self {
            panes,
            arbiter,
            senders: Mutex::new(HashMap::new()),
            workers: Mutex::new(HashMap::new()),
            page: Arc::new(PageShared {
                state: Mutex::new(PageInputState {
                    epoch: 0,
                    current: None,
                    current_nonce: String::new(),
                    retired: HashSet::new(),
                    pending_pages: HashMap::new(),
                    page_ordinals: HashMap::new(),
                    lanes: HashMap::new(),
                    completions: HashMap::new(),
                    next_ticket: 1,
                    pending_lost: HashMap::new(),
                    overflow: None,
                    next_loss: 0,
                    reaper: false,
                }),
                wake: Condvar::new(),
                observer: Mutex::new(None),
            }),
            accepting: AtomicBool::new(true),
        }
    }

    /// Who is told what the host gives up on or refuses of a page's input.
    pub fn set_observer(&self, observer: Arc<dyn InputObserver>) {
        *self
            .page
            .observer
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(observer);
    }

    fn state(&self) -> Result<MutexGuard<'_, PageInputState>, String> {
        self.page
            .state
            .lock()
            .map_err(|_| "pane input admission lock is poisoned".to_string())
    }

    /// The host's epoch: how many main documents have loaded.
    pub fn epoch(&self) -> u64 {
        self.page
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .epoch
    }

    /// Start the clock that settles what a bound passes over, if something
    /// waits on one.
    fn watch_bounds(&self, state: &mut PageInputState) {
        if state.reaper || state.next_deadline().is_none() {
            return;
        }
        state.reaper = true;
        let shared = Arc::downgrade(&self.page);
        let spawned = thread::Builder::new()
            .name("consensflow-input-bounds".to_owned())
            .spawn(move || bounds_clock(shared));
        if spawned.is_err() {
            state.reaper = false;
        }
    }

    fn has_route(&self, key: &PaneKey) -> bool {
        self.senders
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains_key(key)
    }

    /// Admit one numbered call from the page: a key, a paste whole, or a
    /// terminal reply. The host writes a window's calls in the page's number
    /// order: a call that comes before the ones ahead of it waits, at most the
    /// bound. A refusal is final for its number: nothing was written, and the
    /// numbers after it go on. A call of an epoch that is not the host's current one
    /// is never written.
    pub fn enqueue_page(
        &self,
        page_id: &str,
        epoch: u64,
        key: PaneKey,
        sequence: u64,
        work: InputWork,
        human: bool,
    ) -> Result<String, String> {
        self.enqueue_page_told(page_id, epoch, key, sequence, work, human)
            .map_err(|refusal| refusal.error)
    }

    /// As [`Self::enqueue_page`], and a refusal carries the number of the loss the host
    /// recorded for it: the page confirms that number once it has said so.
    pub fn enqueue_page_told(
        &self,
        page_id: &str,
        epoch: u64,
        key: PaneKey,
        sequence: u64,
        work: InputWork,
        human: bool,
    ) -> Result<String, Refusal> {
        let mut events = Vec::new();
        let result = self.admit(page_id, epoch, key, sequence, work, human, &mut events);
        self.page.flush(events);
        self.page.wake.notify_all();
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn admit(
        &self,
        page_id: &str,
        epoch: u64,
        key: PaneKey,
        sequence: u64,
        work: InputWork,
        human: bool,
        events: &mut Vec<Event>,
    ) -> Result<String, Refusal> {
        let mut page = self.state()?;
        let now = Instant::now();
        if !page.admits(page_id, epoch) {
            events.push(Event::Say(format!(
                "pane input: refused a call numbered {sequence} of page {page_id} (epoch {epoch}, the host's is {}) for window {} (generation {}): it is not the current page's",
                page.epoch, key.id, key.generation
            )));
            let closed = !self.has_route(&key);
            page.lost(&key, closed, events);
            return Err(INPUT_STALE_PAGE.to_string().into());
        }
        let lane = page.lane(&key);
        // A call at or below what the lane has settled is a late original after
        // its skip or its bound, or a repeat: nothing of it is written.
        if sequence <= lane.passed || lane.slots.contains_key(&sequence) {
            let late = lane
                .gave_up
                .iter()
                .find(|(number, _)| *number == sequence)
                .map(|(_, at)| now.saturating_duration_since(*at));
            let line = match late {
                Some(late) => format!(
                    "pane input: refused a late call numbered {sequence} for window {} (generation {}): its number was given up {} ms ago",
                    key.id,
                    key.generation,
                    late.as_millis()
                ),
                None => format!(
                    "pane input: refused a repeat of call {sequence} for window {} (generation {})",
                    key.id, key.generation
                ),
            };
            events.push(Event::Say(line));
            return Err(INPUT_SEQUENCE_STALE.to_string().into());
        }
        let refuse = |page: &mut PageInputState,
                      events: &mut Vec<Event>,
                      error: String,
                      closed: bool|
         -> Result<String, Refusal> {
            // Final for its number: the order goes on past it.
            let lane = page.lane(&key);
            if lane.slots.len() < MAX_LANE_SLOTS {
                lane.slots.insert(
                    sequence,
                    Slot::Void {
                        deadline: now + ORDER_BOUND,
                    },
                );
            }
            page.pump(&key, now, events);
            let loss = page.lost(&key, closed, events);
            Err(Refusal {
                error,
                loss: Some(loss),
            })
        };
        if page.lane(&key).slots.len() >= MAX_LANE_SLOTS {
            return refuse(&mut page, events, InputError::queue_full().to_string(), false);
        }
        match &work {
            InputWork::Typed(bytes)
            | InputWork::Write(bytes)
            | InputWork::Reply(bytes)
            | InputWork::Paste { body: bytes, .. } => {
                if let Err(error) = validate_page_input(bytes) {
                    return refuse(&mut page, events, error, false);
                }
            }
            InputWork::Claim => {}
        }
        if page.completions.len() >= MAX_PENDING_INPUT_TICKETS {
            return refuse(
                &mut page,
                events,
                "pane-input-ticket-capacity".to_string(),
                false,
            );
        }
        let next_ticket = match page.next_ticket.checked_add(1) {
            Some(next) => next,
            None => {
                return refuse(
                    &mut page,
                    events,
                    "pane-input-ticket-overflow".to_string(),
                    false,
                )
            }
        };
        let (response, receiver) = oneshot::channel();
        let (reserved, sender) = {
            let senders = self
                .senders
                .lock()
                .map_err(|_| "pane input queue lock is poisoned".to_string());
            let senders = match senders {
                Ok(senders) => senders,
                Err(error) => return refuse(&mut page, events, error, false),
            };
            if !self.accepting.load(Ordering::Acquire) {
                drop(senders);
                return refuse(
                    &mut page,
                    events,
                    InputError::refused(INPUT_CLOSED, "pane input admission is closed").to_string(),
                    false,
                );
            }
            let Some(route) = senders.get(&key) else {
                drop(senders);
                return refuse(
                    &mut page,
                    events,
                    InputError::from(ArbiterError::Stale).to_string(),
                    true,
                );
            };
            match route.reserve(&work) {
                Ok(reserved) => (reserved, route.sender_for(&work).clone()),
                Err(error) => {
                    drop(senders);
                    return refuse(&mut page, events, error.to_string(), false);
                }
            }
        };
        let loss = Arc::new(AtomicU64::new(0));
        let job = InputJob {
            work,
            response,
            _reserved: reserved,
            outcome: Some(Outcome::new(&self.page, &key, Arc::clone(&loss))),
        };
        let ticket = format!("pane-input-{}", page.next_ticket);
        page.next_ticket = next_ticket;
        page.completions.insert(
            ticket.clone(),
            PageInputCompletion {
                receiver: async move { wait_for_input(receiver).await }
                    .boxed()
                    .shared(),
                human,
                key: key.clone(),
                sequence,
                notified: Arc::new(AtomicBool::new(false)),
                loss,
            },
        );
        page.lane(&key).slots.insert(
            sequence,
            Slot::Parked {
                job,
                sender,
                deadline: now + ORDER_BOUND,
            },
        );
        let refused = page.pump(&key, now, events);
        if let Some((_, error, loss)) = refused.into_iter().find(|(number, ..)| *number == sequence)
        {
            page.completions.remove(&ticket);
            return Err(Refusal {
                error: error.to_string(),
                loss: Some(loss),
            });
        }
        self.watch_bounds(&mut page);
        Ok(ticket)
    }

    /// A call of the page ended with its fate unknown (a timeout, a lost
    /// answer), and the page will not send it again. Its number is spent
    /// without a write, and only then is the skip final: the original either went in
    /// before this or is refused when it comes late, and the numbers after it go on. A
    /// completion the page never learned of is dropped. The loss recorded is the answer,
    /// and a retry of the same skip answers the same loss. When the number cannot be
    /// spent yet (the page's numbers are full), the answer is a retry, and the page
    /// keeps asking.
    pub fn skip_page_sequence(
        &self,
        page_id: &str,
        epoch: u64,
        key: &PaneKey,
        sequence: u64,
    ) -> Result<u64, String> {
        let mut events = Vec::new();
        let result = {
            let mut page = self.state()?;
            let now = Instant::now();
            if !page.admits(page_id, epoch) {
                let closed = !self.has_route(key);
                page.lost(key, closed, &mut events);
                drop(page);
                self.page.flush(events);
                return Err(INPUT_STALE_PAGE.to_string());
            }
            if sequence == u64::MAX {
                return Err("pane-input-sequence-overflow".to_string());
            }
            page.pump(key, now, &mut events);
            let lane = page.lane(key);
            if let Some((_, loss)) = lane.skipped.iter().find(|(number, _)| *number == sequence) {
                Ok(*loss)
            } else if sequence > lane.passed
                && !lane.slots.contains_key(&sequence)
                && lane.slots.len() >= MAX_LANE_SLOTS
            {
                Err(SKIP_PENDING.to_string())
            } else {
                if sequence > lane.passed && !lane.slots.contains_key(&sequence) {
                    lane.slots.insert(
                        sequence,
                        Slot::Void {
                            deadline: now + ORDER_BOUND,
                        },
                    );
                }
                // The page's own ticket only: the number is its own.
                page.completions.retain(|_, completion| {
                    !(&completion.key == key && completion.sequence == sequence)
                });
                page.pump(key, now, &mut events);
                let closed = !self.has_route(key);
                let loss = page.lost(key, closed, &mut events);
                let lane = page.lane(key);
                lane.skipped.push_back((sequence, loss));
                while lane.skipped.len() > 64 {
                    lane.skipped.pop_front();
                }
                Ok(loss)
            }
            .inspect(|_| self.watch_bounds(&mut page))
        };
        self.page.flush(events);
        self.page.wake.notify_all();
        result
    }

    /// A call of the page was refused before the host saw it (its bytes did not decode): the
    /// number is spent, and the loss is recorded as a skip's is.
    pub fn refuse_page_sequence(
        &self,
        page_id: &str,
        epoch: u64,
        key: &PaneKey,
        sequence: u64,
    ) -> Result<u64, String> {
        self.skip_page_sequence(page_id, epoch, key, sequence)
    }

    /// The page has shown these windows' notices (each at its version), and the overflow's
    /// (at its version, with its count): those are done, atomically with the check that it is
    /// the current page of the current epoch. A later loss of the same window stays.
    pub fn lost_told(
        &self,
        page_id: &str,
        epoch: u64,
        told: &[(PaneKey, u64)],
        overflow: Option<(u64, u64)>,
    ) -> Result<(), String> {
        let mut page = self.state()?;
        if !page.admits(page_id, epoch) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        page.told(told, overflow);
        Ok(())
    }

    /// What the host still owes the person, for the page that is current to show: it asks at
    /// its activation, after the board loads, on a timer and when it comes into view.
    pub fn pending_losses(&self, page_id: &str, epoch: u64) -> Result<PendingNotices, String> {
        let page = self.state()?;
        if !page.admits(page_id, epoch) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        Ok(page.notices())
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

    /// A page first announces itself, and is answered with the host's epoch and its own
    /// nonce back; it cannot write until it activates with that exact pair. A retry is
    /// answered the same, and a page that navigation has retired is refused.
    pub fn prepare_page(&self, page_id: &str, nonce: &str) -> Result<u64, String> {
        if nonce.is_empty() || nonce.len() > 128 {
            return Err("invalid pane input page nonce".to_string());
        }
        let mut page = self.state()?;
        if page.current.as_deref() == Some(page_id) {
            return if page.current_nonce == nonce {
                Ok(page.epoch)
            } else {
                Err(INPUT_STALE_PAGE.to_string())
            };
        }
        if let Some((held, epoch)) = page.pending_pages.get(page_id) {
            return if held == nonce {
                Ok(*epoch)
            } else {
                Err(INPUT_STALE_PAGE.to_string())
            };
        }
        Self::check_page(&page, page_id)?;
        let epoch = page.epoch;
        page.pending_pages
            .insert(page_id.to_owned(), (nonce.to_owned(), epoch));
        Ok(epoch)
    }

    /// The page becomes current, if it echoes the exact pair its preparation was answered
    /// and the host's epoch is still that one. What the host owes the person comes back, and
    /// is kept until a page says it has shown it ([`Self::lost_told`]): an activation whose
    /// answer was lost is asked again.
    pub fn activate_page(
        &self,
        page_id: &str,
        nonce: &str,
        epoch: u64,
    ) -> Result<PendingNotices, String> {
        let mut events = Vec::new();
        let notices = {
            let mut page = self.state()?;
            if page.epoch != epoch {
                return Err(INPUT_STALE_PAGE.to_string());
            }
            if page.current.as_deref() == Some(page_id) {
                return if page.current_nonce == nonce {
                    Ok(page.notices())
                } else {
                    Err(INPUT_STALE_PAGE.to_string())
                };
            }
            match page.pending_pages.get(page_id) {
                Some((held, prepared)) if held == nonce && *prepared == epoch => {}
                _ => return Err(INPUT_STALE_PAGE.to_string()),
            }
            page.pending_pages.remove(page_id);
            self.start_page(&mut page, page_id, nonce, &mut events)?;
            self.watch_bounds(&mut page);
            page.notices()
        };
        self.page.flush(events);
        self.page.wake.notify_all();
        Ok(notices)
    }

    /// The pane host's own tests can start a page at the host's current epoch, with no
    /// handshake. Returns that epoch.
    pub fn begin_page(&self, page_id: &str) -> Result<u64, String> {
        let mut events = Vec::new();
        let epoch = {
            let mut page = self.state()?;
            if page.current.as_deref() != Some(page_id) {
                self.start_page(&mut page, page_id, "", &mut events)?;
                self.watch_bounds(&mut page);
            }
            page.epoch
        };
        self.page.flush(events);
        self.page.wake.notify_all();
        Ok(epoch)
    }

    fn start_page(
        &self,
        page: &mut PageInputState,
        page_id: &str,
        nonce: &str,
        events: &mut Vec<Event>,
    ) -> Result<(), String> {
        let ordinal = Self::check_page(page, page_id)?;
        // A page that replaces another with no navigation between is a cut all the same.
        if page.current.is_some() {
            page.cut(events);
        }
        if let Some((session, number)) = ordinal {
            page.page_ordinals.insert(session, number);
        }
        page.current = Some(page_id.to_owned());
        page.current_nonce = nonce.to_owned();
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
        let page = self.state()?;
        if page.current.as_deref() != Some(page_id) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        let result = action();
        drop(page);
        Ok(result)
    }

    /// A new main document has begun loading, whatever page is current or answers: the host's
    /// epoch moves, under the input lock, and nothing of the old page's that is still in
    /// order goes in (see [`PageInputState::cut`]). The first call of the new document finds
    /// its place, and no window waits for a reload.
    pub fn end_page(&self) -> Result<(), String> {
        let mut events = Vec::new();
        {
            let mut page = self.state()?;
            page.cut(&mut events);
            page.epoch += 1;
        }
        self.page.flush(events);
        self.page.wake.notify_all();
        Ok(())
    }

    /// A wait may be repeated until the page has actually received its answer.
    pub fn page_completion(
        &self,
        page_id: &str,
        ticket: &str,
    ) -> Result<PageInputCompletion, String> {
        let page = self.state()?;
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
        let mut page = self.state()?;
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
                reply_bytes: Arc::new(AtomicUsize::new(0)),
            },
        );
        Ok(())
    }


    /// A pane gone from the table, killed or retired when its program ended,
    /// takes its input with it: its queue closes (the worker ends once what
    /// was queued has gone in or failed), what waited in order is refused and
    /// recorded as a loss for a window that has closed (named on the current page),
    /// and its page numbers and arbiter state go.
    pub(crate) fn retire(&self, key: &PaneKey) {
        self.senders
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(key);
        self.workers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(key);
        let mut page = self
            .page
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut events = Vec::new();
        if let Some(lane) = page.lanes.remove(key) {
            if !lane.slots.is_empty() {
                let loss = page.lost(key, true, &mut events);
                for slot in lane.slots.into_values() {
                    if let Slot::Parked { job, .. } = slot {
                        job.refuse_told(
                            loss,
                            InputError::refused(INPUT_CLOSED, "pane input queue is closed"),
                        );
                    }
                }
            }
        }
        // An unshown notice of a window that has closed names it, whatever its card.
        if let Some(record) = page.pending_lost.get_mut(key) {
            record.closed = true;
        }
        page.completions
            .retain(|_, completion| &completion.key != key);
        drop(page);
        self.page.flush(events);
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
        let reserved = route.reserve(&work)?;
        let (response, receiver) = oneshot::channel();
        let sender = route.sender_for(&work);
        let job = InputJob {
            work,
            response,
            _reserved: reserved,
            outcome: None,
        };
        match sender.try_send(job) {
            Ok(()) => Ok(receiver),
            Err(mpsc::TrySendError::Full(_)) => Err(InputError::queue_full()),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(InputError::refused(
                INPUT_CLOSED,
                format!("pane input queue for {} is closed", key.id),
            )),
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
        // What waits in order holds a sender to its worker: refuse it, or the
        // worker never ends.
        let lanes = std::mem::take(
            &mut self
                .page
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .lanes,
        );
        for lane in lanes.into_values() {
            for slot in lane.slots.into_values() {
                if let Slot::Parked { job, .. } = slot {
                    job.abandon(InputError::refused(
                        INPUT_CLOSED,
                        "pane input admission is closed",
                    ));
                }
            }
        }
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

/// Settles what a bound passes over, for as long as something waits on one:
/// a call that came before the ones ahead of it, and a page that has gone.
fn bounds_clock(shared: Weak<PageShared>) {
    loop {
        let Some(shared) = shared.upgrade() else {
            return;
        };
        let mut events = Vec::new();
        {
            let mut state = shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let now = Instant::now();
            state.pump_all(now, &mut events);
            let Some(next) = state.next_deadline() else {
                state.reaper = false;
                drop(state);
                shared.flush(events);
                return;
            };
            let wait = next
                .saturating_duration_since(now)
                .clamp(Duration::from_millis(1), Duration::from_millis(500));
            let (state, _) = shared
                .wake
                .wait_timeout(state, wait)
                .unwrap_or_else(|error| error.into_inner());
            drop(state);
        }
        shared.flush(events);
    }
}

fn input_worker(
    jobs: mpsc::Receiver<InputJob>,
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    key: PaneKey,
) {
    for job in jobs {
        let result = match &job.work {
            InputWork::Typed(bytes) => arbiter.write_typed(&panes, &key, bytes),
            InputWork::Write(bytes) | InputWork::Reply(bytes) => arbiter.write(&panes, &key, bytes),
            InputWork::Paste { body, read_empty } => {
                arbiter.write_paste(&panes, &key, body, *read_empty)
            }
            InputWork::Claim => arbiter.claim(&key),
        }
        .map_err(InputError::from);
        let InputJob {
            response,
            _reserved,
            outcome,
            ..
        } = job;
        drop(_reserved);
        match outcome {
            Some(outcome) => outcome.finish(result, response),
            None => {
                let _ = response.send(result);
            }
        }
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
mod boundary;

#[cfg(test)]
mod tests {
    use super::*;

    /// The pane's count: the last number the host has passed.
    fn passed(inputs: &InputQueue, key: &PaneKey) -> u64 {
        let state = inputs.page.state.lock().unwrap();
        state.lanes.get(key).map_or(0, |lane| lane.passed)
    }

    /// The page API as these older tests make it: at the host's own epoch, with a nonce.
    trait Pages {
        fn enqueue(
            &self,
            page: &str,
            key: PaneKey,
            sequence: u64,
            work: InputWork,
            human: bool,
        ) -> Result<String, String>;
        fn skip(&self, page: &str, key: &PaneKey, sequence: u64) -> Result<u64, String>;
        fn prepare(&self, page: &str) -> Result<(), String>;
        fn activate(&self, page: &str) -> Result<Vec<(PaneKey, u64)>, String>;
        fn told(&self, told: &[(PaneKey, u64)]) -> Result<(), String>;
    }

    impl Pages for InputQueue {
        fn enqueue(
            &self,
            page: &str,
            key: PaneKey,
            sequence: u64,
            work: InputWork,
            human: bool,
        ) -> Result<String, String> {
            self.enqueue_page(page, self.epoch(), key, sequence, work, human)
        }
        fn skip(&self, page: &str, key: &PaneKey, sequence: u64) -> Result<u64, String> {
            self.skip_page_sequence(page, self.epoch(), key, sequence)
        }
        fn prepare(&self, page: &str) -> Result<(), String> {
            self.prepare_page(page, "nonce").map(drop)
        }
        fn activate(&self, page: &str) -> Result<Vec<(PaneKey, u64)>, String> {
            let current = self.page.state.lock().unwrap().current.clone();
            let notices = if current.as_deref() == Some(page) {
                self.pending_losses(page, self.epoch())?
            } else {
                self.activate_page(page, "nonce", self.epoch())?
            };
            Ok(notices.losses.into_iter().map(|n| (n.key, n.loss)).collect())
        }
        fn told(&self, told: &[(PaneKey, u64)]) -> Result<(), String> {
            let page = self
                .page
                .state
                .lock()
                .unwrap()
                .current
                .clone()
                .expect("a page is current");
            self.lost_told(&page, self.epoch(), told, None)
        }
    }
    use crate::arbiter::EnterTiming;

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
                && !inputs.page.state.lock().unwrap().lanes.contains_key(key)
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
                .enqueue(
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
                reply_bytes: Arc::new(AtomicUsize::new(0)),
            },
        );
        let typed = |text: &[u8]| InputWork::Typed(text.to_vec());
        inputs
            .enqueue("page-one", key.clone(), 1, typed(b"A"), true)
            .unwrap();
        // An admitted number is passed at once: a repeat of it is stale.
        assert_eq!(
            inputs.enqueue("page-one", key.clone(), 1, typed(b"A"), true),
            Err(INPUT_SEQUENCE_STALE.to_owned())
        );
        // Call 2 timed out at the page: skipped. Its original comes late.
        inputs.skip("page-one", &key, 2).unwrap();
        assert_eq!(passed(&inputs, &key), 2);
        assert_eq!(
            inputs.enqueue("page-one", key.clone(), 2, typed(b"B"), true),
            Err(INPUT_SEQUENCE_STALE.to_owned())
        );
        // A repeat of an admitted number is refused the same way.
        assert_eq!(
            inputs.enqueue("page-one", key.clone(), 1, typed(b"A"), true),
            Err(INPUT_SEQUENCE_STALE.to_owned())
        );
        // A skip below the count moves nothing: the host keeps the higher.
        inputs.skip("page-one", &key, 2).unwrap();
        inputs.skip("page-one", &key, 0).unwrap();
        assert_eq!(passed(&inputs, &key), 2);
        // A skip for an original that was admitted: no completion is left for
        // the answer the page never read.
        inputs
            .enqueue("page-one", key.clone(), 3, typed(b"C"), true)
            .unwrap();
        assert_eq!(inputs.page.state.lock().unwrap().completions.len(), 2);
        inputs.skip("page-one", &key, 3).unwrap();
        assert_eq!(inputs.page.state.lock().unwrap().completions.len(), 1);
        inputs
            .enqueue("page-one", key.clone(), 4, typed(b"D"), true)
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
        // The page that went is not heard again.
        assert_eq!(
            inputs.enqueue("page-one", key.clone(), 1, typed(b"A"), true),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        assert_eq!(passed(&inputs, &key), 0);
        assert_eq!(
            inputs.begin_page("page-one"),
            Err(INPUT_STALE_PAGE.to_owned())
        );
        inputs
            .enqueue("page-two", key, 1, typed(b"E"), true)
            .unwrap();
        // It goes in once the page that went has been heard for its bound.
        assert!(
            matches!(received.recv_timeout(Duration::from_secs(3)).unwrap().work, InputWork::Typed(bytes) if bytes == b"E")
        );
        inputs.close_and_drain();
    }

    #[test]
    fn a_refused_admission_is_final_for_its_number() {
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
                reply_bytes: Arc::new(AtomicUsize::new(0)),
            },
        );
        let typed = || InputWork::Typed(b"A".to_vec());
        assert_eq!(
            inputs.enqueue("page", key.clone(), 1, typed(), true),
            Err("the pane's input queue is full".to_string())
        );
        // Nothing was written, and the number is spent: the page says so and
        // goes on with the next.
        assert_eq!(passed(&inputs, &key), 1);
        assert_eq!(
            inputs.enqueue("page", key.clone(), 1, typed(), true),
            Err(INPUT_SEQUENCE_STALE.to_string())
        );
        assert_eq!(
            inputs.enqueue("page", key.clone(), 2, typed(), true),
            Err("the pane's input queue is full".to_string())
        );
        assert_eq!(passed(&inputs, &key), 2);
        assert_eq!(inputs.page.state.lock().unwrap().completions.len(), 0);
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
                reply_bytes: Arc::new(AtomicUsize::new(0)),
            },
        );
        let ticket = inputs
            .enqueue("page", key, 1, InputWork::Typed(b"A".to_vec()), true)
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

    /// A window whose writers are the test's own: what the host hands over is
    /// read from the two receivers, and nothing is written anywhere.
    fn bare() -> (
        InputQueue,
        PaneKey,
        mpsc::Receiver<InputJob>,
        mpsc::Receiver<InputJob>,
    ) {
        let inputs = InputQueue::new(
            Arc::new(PaneTable::new()),
            Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
        );
        let key = PaneKey::new("bare-pane", 1);
        let (sender, received) = mpsc::sync_channel(INPUT_QUEUE_CAPACITY);
        let (replies, received_replies) = mpsc::sync_channel(INPUT_QUEUE_CAPACITY);
        inputs.senders.lock().unwrap().insert(
            key.clone(),
            InputRoute {
                sender,
                replies,
                pending_bytes: Arc::new(AtomicUsize::new(0)),
                reply_bytes: Arc::new(AtomicUsize::new(0)),
            },
        );
        (inputs, key, received, received_replies)
    }

    fn typed(text: &[u8]) -> InputWork {
        InputWork::Typed(text.to_vec())
    }

    fn handed(received: &mpsc::Receiver<InputJob>) -> Vec<u8> {
        std::iter::from_fn(|| received.try_recv().ok())
            .flat_map(|job| match &job.work {
                InputWork::Typed(bytes) | InputWork::Reply(bytes) => bytes.clone(),
                _ => unreachable!(),
            })
            .collect()
    }

    #[derive(Default)]
    struct Observed {
        lines: Mutex<Vec<String>>,
        lost: Mutex<Vec<(PaneKey, u64)>>,
    }

    impl InputObserver for Observed {
        fn lost(&self, key: &PaneKey, loss: u64) {
            self.lost.lock().unwrap().push((key.clone(), loss));
        }
        fn say(&self, line: &str) {
            self.lines.lock().unwrap().push(line.to_owned());
        }
    }

    fn observed(inputs: &InputQueue) -> Arc<Observed> {
        let observed = Arc::new(Observed::default());
        inputs.set_observer(observed.clone());
        observed
    }

    /// Calls that come as 2, 3, 1 go to the writer as 1, 2, 3: a call that comes
    /// early waits for the numbers before it, and none is refused as stale.
    #[test]
    fn calls_that_come_out_of_order_reach_the_writer_in_number_order() {
        let (inputs, key, received, _) = bare();
        inputs.begin_page("page").unwrap();
        inputs
            .enqueue("page", key.clone(), 2, typed(b"B"), true)
            .unwrap();
        inputs
            .enqueue("page", key.clone(), 3, typed(b"C"), true)
            .unwrap();
        assert_eq!(handed(&received), b"", "B and C wait for A");
        inputs
            .enqueue("page", key.clone(), 1, typed(b"A"), true)
            .unwrap();
        assert_eq!(handed(&received), b"ABC");
        inputs.close_and_drain();
    }

    /// A number that never comes holds what is behind it for the bound and no
    /// longer; the host says so, and refuses the number when it comes late,
    /// with how late. No byte of what was typed is in what it says.
    #[test]
    fn a_number_that_never_comes_holds_what_is_behind_it_for_the_bound_only() {
        let (inputs, key, received, _) = bare();
        let observed = observed(&inputs);
        inputs.begin_page("page").unwrap();
        let started = Instant::now();
        inputs
            .enqueue("page", key.clone(), 2, typed(b"SECRET"), true)
            .unwrap();
        assert!(received.try_recv().is_err());
        let job = received
            .recv_timeout(Duration::from_secs(3))
            .expect("handed over at the bound");
        let waited = started.elapsed();
        assert!(matches!(job.work, InputWork::Typed(ref bytes) if bytes == b"SECRET"));
        assert!(
            waited >= Duration::from_millis(1400) && waited < Duration::from_millis(2500),
            "waited {waited:?}"
        );
        thread::sleep(Duration::from_millis(200));
        assert_eq!(
            inputs.enqueue("page", key.clone(), 1, typed(b"A"), true),
            Err(INPUT_SEQUENCE_STALE.to_owned())
        );
        let lines = observed.lines.lock().unwrap().clone();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("gave up numbers 1 to 1")
                    && line.contains("had not come after 15")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("late call numbered 1") && line.contains("given up 2")),
            "{lines:?}"
        );
        assert!(
            lines.iter().all(|line| !line.contains("SECRET")),
            "{lines:?}"
        );
        assert_eq!(
            observed.lost.lock().unwrap().len(),
            1,
            "the gap is recorded: the page that is current may not be able to say so"
        );
        inputs.close_and_drain();
    }

    /// A window holds 8 MiB of pending input: a paste of 4 MiB and another
    /// behind it. The next key is refused, once, with the queue's words, and
    /// the number after it goes in at once; a key typed after the writer has
    /// drained goes in.
    #[test]
    fn a_full_window_refuses_the_next_key_once_and_goes_on() {
        let (inputs, key, received, _) = bare();
        inputs.begin_page("page").unwrap();
        let big = vec![b'p'; 4 * 1024 * 1024];
        inputs
            .enqueue("page", key.clone(), 1, InputWork::Typed(big.clone()), true)
            .unwrap();
        inputs
            .enqueue("page", key.clone(), 2, InputWork::Typed(big), true)
            .unwrap();
        assert_eq!(
            inputs.enqueue("page", key.clone(), 3, typed(b"k"), true),
            Err("the pane's input queue is full".to_owned())
        );
        // The refusal is final for its number, and does not hold the next.
        assert_eq!(
            inputs.enqueue("page", key.clone(), 3, typed(b"k"), true),
            Err(INPUT_SEQUENCE_STALE.to_owned())
        );
        drop(received.try_recv().unwrap());
        drop(received.try_recv().unwrap());
        inputs
            .enqueue("page", key.clone(), 4, typed(b"z"), true)
            .unwrap();
        assert_eq!(handed(&received), b"z");
        inputs.close_and_drain();
    }

    /// A hundred calls of 4 MiB numbered ahead of a missing one: the window
    /// holds two, and the rest are refused. At the bound the two go in, in
    /// order.
    #[test]
    fn a_hundred_large_calls_ahead_of_a_missing_one_are_refused_past_8_mib() {
        let (inputs, key, received, _) = bare();
        inputs.begin_page("page").unwrap();
        let mut admitted = 0;
        for number in 2..=101_u64 {
            let mut body = vec![b'x'; 4 * 1024 * 1024];
            body[0] = u8::try_from(number).unwrap();
            if inputs
                .enqueue("page", key.clone(), number, InputWork::Typed(body), true)
                .is_ok()
            {
                admitted += 1;
            }
        }
        assert_eq!(admitted, 2);
        let first = received.recv_timeout(Duration::from_secs(3)).unwrap();
        let second = received.recv_timeout(Duration::from_secs(1)).unwrap();
        let first_byte = |job: &InputJob| match &job.work {
            InputWork::Typed(bytes) => bytes[0],
            _ => unreachable!(),
        };
        assert_eq!((first_byte(&first), first_byte(&second)), (2, 3));
        thread::sleep(Duration::from_millis(100));
        assert!(
            received.try_recv().is_err(),
            "the refused ones were never handed over"
        );
        inputs.close_and_drain();
    }

    /// A reply has room of its own: a paste of 4 MiB pending, its write held,
    /// does not refuse it. Its room is 64 KiB.
    #[test]
    fn a_reply_is_admitted_while_a_paste_is_pending() {
        let (inputs, key, received, replies) = bare();
        inputs.begin_page("page").unwrap();
        inputs
            .enqueue(
                "page",
                key.clone(),
                1,
                InputWork::Typed(vec![b'p'; 4 * 1024 * 1024]),
                true,
            )
            .unwrap();
        inputs
            .enqueue(
                "page",
                key.clone(),
                2,
                InputWork::Typed(vec![b'p'; 4 * 1024 * 1024]),
                true,
            )
            .unwrap();
        inputs
            .enqueue(
                "page",
                key.clone(),
                3,
                InputWork::Reply(b"\x1b[1;1R".to_vec()),
                false,
            )
            .unwrap();
        let answer = replies.try_recv().unwrap();
        assert!(matches!(&answer.work, InputWork::Reply(bytes) if bytes == b"\x1b[1;1R"));
        assert_eq!(
            inputs.enqueue(
                "page",
                key.clone(),
                4,
                InputWork::Reply(vec![b'r'; 64 * 1024]),
                false
            ),
            Err("the pane's input queue is full".to_owned()),
            "the replies' 64 KiB hold the first answer, and this one does not fit"
        );
        drop(answer);
        drop(received);
        inputs.close_and_drain();
    }

    /// With no page current when typing is lost, the notice waits for the next page's
    /// activation, and stays until the page says it told the person.
    #[test]
    fn typing_lost_with_no_page_current_is_told_at_the_next_activation() {
        let (inputs, key, received, _) = bare();
        inputs.begin_page("s:1").unwrap();
        inputs
            .enqueue("s:1", key.clone(), 2, typed(b"B"), true)
            .unwrap();
        inputs.end_page().unwrap();
        assert_eq!(handed(&received), b"", "B waited behind a missing number: it is cancelled");
        inputs.prepare("s:2").unwrap();
        let lost = inputs.activate("s:2").unwrap();
        assert_eq!(lost.len(), 1);
        assert_eq!(lost[0].0, key);
        // The answer was lost at the page, which asks again, and gets the same notice.
        assert_eq!(inputs.activate("s:2").unwrap(), lost);
        inputs.told(&lost).unwrap();
        assert!(inputs.activate("s:2").unwrap().is_empty());
        inputs.close_and_drain();
    }

    /// A notice told for one loss does not clear a later loss of the window.
    #[test]
    fn a_notice_told_clears_only_the_losses_up_to_its_number() {
        let (inputs, key, _received, _) = bare();
        inputs.begin_page("s:1").unwrap();
        let first = inputs.skip("s:1", &key, 1).unwrap();
        let second = inputs.skip("s:1", &key, 2).unwrap();
        assert!(second > first);
        inputs.told(&[(key.clone(), first)]).unwrap();
        assert_eq!(
            inputs.activate("s:1").unwrap(),
            vec![(key.clone(), second)]
        );
        inputs.told(&[(key, second)]).unwrap();
        assert!(inputs.activate("s:1").unwrap().is_empty());
        inputs.close_and_drain();
    }

    /// A later arrival does not stretch an earlier call's wait. Number 3 comes at 0, number 2
    /// at 1,000 ms; at 1,500 ms number 3 has waited its bound.
    #[test]
    fn a_later_arrival_does_not_stretch_an_earlier_calls_wait() {
        let (inputs, key, received, _) = bare();
        inputs.begin_page("page").unwrap();
        inputs
            .enqueue("page", key.clone(), 3, typed(b"C"), true)
            .unwrap();
        thread::sleep(Duration::from_millis(1000));
        inputs
            .enqueue("page", key.clone(), 2, typed(b"B"), true)
            .unwrap();
        thread::sleep(Duration::from_millis(800));
        assert_eq!(handed(&received), b"BC", "number 3 had waited 1.8 s");
        inputs.close_and_drain();
    }

    /// What one page holds for one window behind a gap is bounded, whatever the bytes.
    #[test]
    fn what_a_page_holds_behind_a_gap_is_bounded() {
        let (inputs, key, _received, _) = bare();
        inputs.begin_page("page").unwrap();
        let mut admitted = 0;
        for number in 2..200_u64 {
            if inputs
                .enqueue("page", key.clone(), number, typed(b"x"), true)
                .is_ok()
            {
                admitted += 1;
            }
        }
        assert_eq!(admitted, MAX_LANE_SLOTS);
        let held: usize = inputs
            .page
            .state
            .lock()
            .unwrap()
            .lanes
            .values()
            .map(|lane| lane.slots.len())
            .sum();
        assert_eq!(held, MAX_LANE_SLOTS);
        inputs.close_and_drain();
    }

    /// The host keeps no more notices than its bound.
    #[test]
    fn the_notices_a_host_keeps_are_bounded() {
        let (inputs, _key, _received, _) = bare();
        inputs.begin_page("page").unwrap();
        for window in 0..(MAX_PENDING_LOSSES as u64 + 20) {
            let key = PaneKey::new(format!("window-{window}"), 1);
            inputs.skip("page", &key, 1).unwrap();
        }
        assert_eq!(
            inputs.activate("page").unwrap().len(),
            MAX_PENDING_LOSSES
        );
        inputs.close_and_drain();
    }
}