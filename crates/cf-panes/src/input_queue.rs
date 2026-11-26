//! Every open pane's input, in order: one worker and one bounded queue per
//! pane, and a second for the terminal's replies, the page's numbered input
//! admitted before it is answered, and why an input did not go in.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use futures_channel::oneshot;
use futures_util::future::{BoxFuture, FutureExt, Shared};
use serde_json::{json, Value};

use cf_proto::panes::ReadEmpty;

use crate::arbiter::{ArbiterError, At, InputArbiter, Paste};
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
/// How many notices stay outstanding as rows of their own: past it the oldest are folded into
/// one, "Input was also lost in N more places".
const MAX_OUTSTANDING_NOTICES: usize = 64;
const MAX_PENDING_INPUT_TICKETS: usize = 4096;
/// How many dropped files' places the host holds at once.
const MAX_PLACES: usize = 4096;
/// How many names the host keeps.
const MAX_NAMED_WINDOWS: usize = 1024;
/// How many page sessions' latest numbers the host keeps.
const MAX_PAGE_SESSIONS: usize = 64;
const INPUT_QUEUE_FULL: &str = "pane-input-queue-full";
/// The host is stopping, and takes no more input.
const INPUT_CLOSED: &str = "input-closed";
const LOCK_POISONED: &str = "lock-poisoned";
/// A call numbered at or below what the pane's count has passed: a late
/// original after its skip, or a repeat. Nothing of it is written.
const INPUT_SEQUENCE_STALE: &str = "pane-input-sequence-stale";
pub(crate) const INPUT_STALE_PAGE: &str = "pane-input-stale-page";
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
        at: Option<At>,
        read_empty: Option<ReadEmpty>,
        clear_first: Option<Vec<u8>>,
    },
    /// An Enter of a delivery that is not a paste's first ([`InputArbiter::enter`]).
    Enter(At),
    Claim,
}

impl InputWork {
    fn byte_count(&self) -> usize {
        match self {
            Self::Typed(bytes) | Self::Write(bytes) | Self::Reply(bytes) => bytes.len(),
            Self::Paste { body, .. } => body.len().saturating_add(13),
            Self::Enter(_) => 1,
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
            ArbiterError::LookAgain(why) => {
                return Self::refused("look-again", why.cause().to_string())
            }
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

#[cfg(test)]
thread_local! {
    /// A test makes the thread that starts the bounds clock, or the outcome's helper, fail to.
    static FAIL_THREAD_SPAWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A call's identity: the window and its generation, the page that made it, its epoch and the
/// call's number. The window's generation does not change at a page cut and the epoch does, and
/// two documents that share an epoch (a page that takes over from another with no navigation
/// between) still have their own page ids: the old document's number 1 and the new one's are
/// different calls whatever the web view does.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CallKey {
    key: PaneKey,
    page: String,
    epoch: u64,
    sequence: u64,
}

/// What is known of one call's loss, shared by its outcome and whoever speaks for it (a skip):
/// the call has at most one loss, and whoever takes the number first (a compare and swap from 0)
/// is the one that makes the notice.
#[derive(Default)]
struct CallCell {
    loss: AtomicU64,
}

impl CallCell {
    fn get(&self) -> Option<u64> {
        match self.loss.load(Ordering::Acquire) {
            0 => None,
            loss => Some(loss),
        }
    }

    /// Take loss number `loss` for the call, if it has none: `Err` holds the one it has.
    fn take(&self, loss: u64) -> Result<(), u64> {
        self.loss
            .compare_exchange(0, loss, Ordering::AcqRel, Ordering::Acquire)
            .map(drop)
    }
}

/// A page job's one host-owned outcome. The host settles it itself, under its lock, whatever
/// becomes of the page's wait, ticket or window: written (nothing to say), refused, or
/// uncertain. A job dropped with none (its worker vanished, its queue was dropped whole) is
/// uncertain, and the drop records that, without taking the host's lock: it takes its loss
/// number from the host's counter at once, and the host settles it the next time it looks.
struct Outcome {
    shared: Weak<PageShared>,
    call: CallKey,
    /// Whether the window is still there: false once it has retired.
    alive: Arc<AtomicBool>,
    /// The call's loss, for its wait to answer.
    cell: Arc<CallCell>,
    settled: bool,
}

impl Outcome {
    fn new(
        shared: &Arc<PageShared>,
        call: CallKey,
        alive: Arc<AtomicBool>,
        cell: Arc<CallCell>,
    ) -> Self {
        Self {
            shared: Arc::downgrade(shared),
            call,
            alive,
            cell,
            settled: false,
        }
    }

    /// Recorded under the host's lock: the call did not go in whole, or may not have. A call
    /// that already has its loss (the page skipped it) has that one, and makes no second.
    fn lose(
        &mut self,
        state: &mut PageInputState,
        closed: bool,
        failed: bool,
        events: &mut Vec<Event>,
    ) -> u64 {
        self.settled = true;
        let flags = Flags {
            closed: closed || !self.alive.load(Ordering::Acquire),
            failed,
            dropped: false,
        };
        state.lose_call(&self.cell, &self.call.key, flags, events)
    }

    /// The worker's answer. A failed or uncertain write is recorded before it is answered,
    /// so that the page's wait, a page that has gone and a ticket cleared all leave the
    /// same notice.
    fn finish(mut self, result: InputResponse, response: oneshot::Sender<InputResponse>) {
        let shared = self.shared.upgrade();
        match (shared, result.as_ref().err()) {
            (Some(shared), Some(error)) => {
                let failed = error.failed_until_restart();
                let mut events = Vec::new();
                {
                    let mut state = shared.lock();
                    self.lose(&mut state, false, failed, &mut events);
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
        // The number first, so that the wait this drop answers finds it; the record is the
        // host's to make, from its own list, whatever thread has the lock (this one may). A
        // call that has a loss already (a skip took it) makes none.
        let loss = shared.next_loss.fetch_add(1, Ordering::AcqRel) + 1;
        if self.cell.take(loss).is_err() {
            return;
        }
        shared
            .orphans
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((
                self.call.key.clone(),
                loss,
                !self.alive.load(Ordering::Acquire),
            ));
        shared.wake.notify_all();
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

    /// Nothing of it was written, and the host has recorded its loss (the call's own).
    fn refuse_told(mut self, error: InputError) {
        if let Some(outcome) = self.outcome.as_mut() {
            outcome.settled = true;
        }
        let _ = self.response.send(Err(error));
    }

    /// The call's cell, if it is a page's.
    fn cell(&self) -> Option<Arc<CallCell>> {
        self.outcome
            .as_ref()
            .map(|outcome| Arc::clone(&outcome.cell))
    }
}

/// What the host says of a page's input beyond answering it. It never carries a
/// byte of what was typed.
pub trait InputObserver: Send + Sync {
    /// Typing may not have reached this window (loss number `loss`, the window's
    /// latest): a hint to the page that is current, which pulls the host's list.
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
    /// Whether the refused number is spent: false where the host could not keep it (its slots
    /// are full), and the page then skips it until the host has.
    pub spent: bool,
}

impl From<String> for Refusal {
    fn from(error: String) -> Self {
        Self {
            error,
            loss: None,
            spent: true,
        }
    }
}

#[derive(Clone)]
pub struct PageInputCompletion {
    pub receiver: Shared<BoxFuture<'static, InputResponse>>,
    pub human: bool,
    key: PaneKey,
    sequence: u64,
    notified: Arc<AtomicBool>,
    cell: Arc<CallCell>,
}

impl PageInputCompletion {
    /// A retried wait may finish in more than one caller; tell the page's
    /// state about a successful human write only once.
    pub fn notify_human_once(&self) -> bool {
        self.human && !self.notified.swap(true, Ordering::AcqRel)
    }

    /// The number of the loss the host recorded for this call, once it is told as lost.
    pub fn loss(&self) -> Option<u64> {
        self.cell.get()
    }
}

/// What a notice says. It is fixed when the notice is made: the window's name and generation
/// as the host knew them then, what was lost, and the advice. A change of any of it is a new
/// notice under a new id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoticeBody {
    /// Typing (or, `dropped`, a dropped file's path) may not have reached a window.
    Window {
        key: PaneKey,
        /// What the page called the window, if it had said.
        name: Option<String>,
        /// The window has closed for good.
        closed: bool,
        /// The window takes no more input until its project restarts.
        failed: bool,
        dropped: bool,
    },
    /// The oldest notices past the cap, folded into one: the places input was also lost in.
    Folded { count: u64 },
}

/// One thing the host still owes the person. Its id is never used again while the host runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub id: u64,
    pub body: NoticeBody,
}

/// Everything the host still owes the person, oldest id first.
pub type PendingNotices = Vec<Notice>;

/// What a loss says beyond its window.
#[derive(Clone, Copy, Default)]
struct Flags {
    closed: bool,
    failed: bool,
    dropped: bool,
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
    /// Pages that prepared, with the nonce and epoch the host answered them.
    pending_pages: HashMap<String, (String, u64)>,
    /// The last activated document's number of each page session, for at most
    /// `MAX_PAGE_SESSIONS` sessions (the oldest forgotten). Nothing depends on it: a page is
    /// judged by its epoch and nonce; this only refuses an out-of-order preparation early.
    page_ordinals: HashMap<String, u64>,
    ordinal_order: VecDeque<String>,
    lanes: HashMap<PaneKey, Lane>,
    completions: HashMap<String, PageInputCompletion>,
    next_ticket: u64,
    /// The calls that hold bytes in the host (parked, or handed to a writer), by identity.
    calls: HashMap<CallKey, Weak<CallCell>>,
    calls_swept_at: usize,
    /// Dropped files' places held by a page of this epoch: a number taken at the gesture and
    /// not yet used or let go.
    places: std::collections::HashSet<(PaneKey, u64)>,
    /// Outstanding notices by id; at most `notice_cap` (`MAX_OUTSTANDING_NOTICES`).
    notices: BTreeMap<u64, Notice>,
    notice_cap: usize,
    /// The one row that stands for the oldest notices past the cap: its id and the places folded.
    folded: Option<(u64, u64)>,
    /// Windows whose input fails until their project restarts (their notices say so).
    failed_windows: std::collections::HashSet<PaneKey>,
    /// What the page said of each window's name.
    names: HashMap<PaneKey, String>,
    name_order: VecDeque<PaneKey>,
    counter: Arc<AtomicU64>,
    reaper: bool,
    /// For tests: each loss number's notice, and each notice's successor (a replacement or a fold).
    #[cfg(test)]
    covers: HashMap<u64, u64>,
    #[cfg(test)]
    lineage: HashMap<u64, u64>,
}

struct PageShared {
    state: Mutex<PageInputState>,
    wake: Condvar,
    observer: Mutex<Option<Arc<dyn InputObserver>>>,
    /// The host's loss counter: a number is taken from it with or without the state's lock.
    next_loss: Arc<AtomicU64>,
    /// Outcomes of calls whose worker went without answering: `(window, loss, closed)`,
    /// settled into the state the next time anything looks at it.
    orphans: Mutex<Vec<(PaneKey, u64, bool)>>,
}

impl PageShared {
    /// The host's state. A lock a panic poisoned is taken back: the state is what it was when
    /// the panic came, and the host goes on with it rather than refuse every call after.
    fn lock(&self) -> MutexGuard<'_, PageInputState> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                self.state.clear_poison();
                poisoned.into_inner()
            }
        };
        let orphans = std::mem::take(
            &mut *self
                .orphans
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        for (key, loss, closed) in orphans {
            state.record(
                &key,
                loss,
                Flags {
                    closed,
                    ..Flags::default()
                },
                &mut Vec::new(),
            );
        }
        state
    }

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
    /// The next loss number: from the host's counter, which a job's drop also takes from. A
    /// notice's id is the loss number that made it, so an id is never used twice.
    fn fresh_version(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Record that typing may be missing from this window, as a loss of its own: the window's
    /// notice is replaced by a new one under a new id (so that a Dismiss of the old one cannot
    /// clear what came after it), and the page that is current is told to ask.
    fn lost(&mut self, key: &PaneKey, closed: bool, events: &mut Vec<Event>) -> u64 {
        self.lost_with(
            key,
            Flags {
                closed,
                ..Flags::default()
            },
            events,
        )
    }

    fn lost_with(&mut self, key: &PaneKey, flags: Flags, events: &mut Vec<Event>) -> u64 {
        let loss = self.fresh_version();
        self.record(key, loss, flags, events);
        loss
    }

    /// A call's one loss: the number it already has, or a new one that takes it.
    fn lose_call(
        &mut self,
        cell: &CallCell,
        key: &PaneKey,
        flags: Flags,
        events: &mut Vec<Event>,
    ) -> u64 {
        let loss = self.fresh_version();
        match cell.take(loss) {
            Ok(()) => {
                self.record(key, loss, flags, events);
                loss
            }
            Err(held) => {
                // A skip took the call's one loss first. If the write then failed for good,
                // the window is failed all the same and its notice says to restart.
                if flags.failed {
                    self.mark_failed(key);
                }
                held
            }
        }
    }

    /// The window takes no more input until its project restarts: it is marked, and its
    /// outstanding typing notice, if it does not say so, is replaced by one that does.
    fn mark_failed(&mut self, key: &PaneKey) {
        self.failed_windows.insert(key.clone());
        let unsaid: Vec<u64> = self
            .notices
            .iter()
            .filter(|(_, notice)| {
                matches!(&notice.body, NoticeBody::Window { key: of, failed: false, dropped: false, .. } if of == key)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in unsaid {
            self.replace(id, |body| {
                if let NoticeBody::Window { failed, .. } = body {
                    *failed = true;
                }
            });
        }
    }

    /// The cell of a call that still holds bytes in the host, by its whole identity.
    fn call_cell(&self, call: &CallKey) -> Option<Arc<CallCell>> {
        self.calls.get(call).and_then(Weak::upgrade)
    }

    fn remember_call(&mut self, call: CallKey, cell: &Arc<CallCell>) {
        if self.calls.len() >= self.calls_swept_at.max(1024) {
            self.calls.retain(|_, held| held.strong_count() > 0);
            self.calls_swept_at = self.calls.len() * 2;
        }
        self.calls.insert(call, Arc::downgrade(cell));
    }

    /// Make the notice for loss `loss`. The window's outstanding notice of the same kind, if
    /// any, goes, and what it said is kept (a closed window stays closed; a failed one stays
    /// failed): the new one stands in its place under the new id. Past the cap the oldest is
    /// folded.
    fn record(&mut self, key: &PaneKey, loss: u64, flags: Flags, events: &mut Vec<Event>) {
        if flags.failed {
            self.failed_windows.insert(key.clone());
        }
        let mut body = NoticeBody::Window {
            key: key.clone(),
            name: self.names.get(key).cloned(),
            closed: flags.closed,
            failed: flags.failed || self.failed_windows.contains(key),
            dropped: flags.dropped,
        };
        let before = self
            .notices
            .iter()
            .find(|(_, notice)| match &notice.body {
                NoticeBody::Window {
                    key: of, dropped, ..
                } => of == key && *dropped == flags.dropped,
                NoticeBody::Folded { .. } => false,
            })
            .map(|(id, _)| *id);
        if let Some(id) = before {
            if let Some(Notice {
                body: NoticeBody::Window { closed: was, .. },
                ..
            }) = self.notices.remove(&id)
            {
                if let NoticeBody::Window { closed, .. } = &mut body {
                    *closed |= was;
                }
            }
            #[cfg(test)]
            self.lineage.insert(id, loss);
        }
        #[cfg(test)]
        self.covers.insert(loss, loss);
        self.notices.insert(loss, Notice { id: loss, body });
        self.fold_past_the_cap();
        if self.current.is_some() {
            events.push(Event::Lost(key.clone(), loss));
        }
    }

    /// Past the cap the oldest notices are one row, "also lost in N more places"; a fold that
    /// changes is a new row under a new id.
    fn fold_past_the_cap(&mut self) {
        while self.notices.len() > self.notice_cap {
            let Some(oldest) = self.notices.keys().next().copied() else {
                return;
            };
            self.notices.remove(&oldest);
            let id = self.fresh_version();
            let count = self.folded.map_or(0, |(_, count)| count) + 1;
            #[cfg(test)]
            {
                self.lineage.insert(oldest, id);
                if let Some((old, _)) = self.folded {
                    self.lineage.insert(old, id);
                }
            }
            self.folded = Some((id, count));
        }
    }

    /// The person dismissed the notice with this id: it goes, if it is still outstanding under
    /// that id. A replaced notice's id is nothing any more.
    fn dismiss(&mut self, id: u64) -> bool {
        if self.notices.remove(&id).is_some() {
            return true;
        }
        if self.folded.is_some_and(|(held, _)| held == id) {
            self.folded = None;
            return true;
        }
        false
    }

    fn notices(&self) -> PendingNotices {
        let mut all: Vec<Notice> = self.notices.values().cloned().collect();
        if let Some((id, count)) = self.folded {
            all.push(Notice {
                id,
                body: NoticeBody::Folded { count },
            });
        }
        all.sort_by_key(|notice| notice.id);
        all
    }

    /// The name the page gave a window. A notice of that window which says another name (or
    /// none) is replaced by one that says this.
    fn name_window(&mut self, key: &PaneKey, name: &str) {
        if self.names.get(key).map(String::as_str) == Some(name) {
            return;
        }
        if self.names.insert(key.clone(), name.to_owned()).is_none() {
            self.name_order.push_back(key.clone());
        }
        while self.name_order.len() > MAX_NAMED_WINDOWS {
            if let Some(old) = self.name_order.pop_front() {
                self.names.remove(&old);
            }
        }
        let stale: Vec<u64> = self
            .notices
            .iter()
            .filter(|(_, notice)| match &notice.body {
                NoticeBody::Window {
                    key: of,
                    name: said,
                    ..
                } => of == key && said.as_deref() != Some(name),
                NoticeBody::Folded { .. } => false,
            })
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            self.replace(id, |body| {
                if let NoticeBody::Window { name: said, .. } = body {
                    *said = Some(name.to_owned());
                }
            });
        }
    }

    /// The notice `id` says something else now: it is replaced by one under a new id.
    fn replace(&mut self, id: u64, change: impl FnOnce(&mut NoticeBody)) -> Option<u64> {
        let mut notice = self.notices.remove(&id)?;
        change(&mut notice.body);
        let fresh = self.fresh_version();
        notice.id = fresh;
        #[cfg(test)]
        self.lineage.insert(id, fresh);
        self.notices.insert(fresh, notice);
        Some(fresh)
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
        let mut gave_up: Vec<(u64, u64)> = Vec::new();
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
                        gave_up.push((lane.passed + 1, first - 1));
                        lane.give_up(first - 1, now, waited, key, &mut said);
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
            let flags = Flags {
                closed,
                failed: error.failed_until_restart(),
                dropped: false,
            };
            let loss = match job.cell() {
                Some(cell) => self.lose_call(&cell, key, flags, events),
                None => self.lost_with(key, flags, events),
            };
            job.refuse_told(error.clone());
            refused.push((number, error, loss));
        }
        // What was given up: a dropped file's place is a loss of the file, any other number
        // a loss of typing.
        for (from, upto) in gave_up {
            let places: Vec<u64> = self
                .places
                .iter()
                .filter(|(of, number)| of == key && (from..=upto).contains(number))
                .map(|(_, number)| *number)
                .collect();
            for number in &places {
                self.places.remove(&(key.clone(), *number));
                self.lost_with(
                    key,
                    Flags {
                        dropped: true,
                        ..Flags::default()
                    },
                    events,
                );
            }
            if (upto - from + 1) > places.len() as u64 {
                self.lost(key, false, events);
            }
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
    /// call is cancelled as a loss, and so is a known missing number below the numbers seen,
    /// and each dropped file's place that was never used. What was handed to a window's
    /// writer stays written, in order.
    fn cut(&mut self, events: &mut Vec<Event>) {
        let lanes = std::mem::take(&mut self.lanes);
        for (key, lane) in lanes {
            if lane.slots.is_empty() {
                continue;
            }
            let loss = self.lost(&key, false, events);
            for slot in lane.slots.into_values() {
                if let Slot::Parked { job, .. } = slot {
                    if let Some(cell) = job.cell() {
                        let _ = cell.take(loss);
                    }
                    job.refuse_told(InputError::refused(
                        INPUT_STALE_PAGE,
                        "the page reloaded before this was sent",
                    ));
                }
            }
        }
        // A file dropped whose path the old page had not typed: the host says so itself.
        let mut places: Vec<(PaneKey, u64)> =
            std::mem::take(&mut self.places).into_iter().collect();
        places.sort();
        for (key, _) in places {
            self.lost_with(
                &key,
                Flags {
                    dropped: true,
                    ..Flags::default()
                },
                events,
            );
        }
        // No departed identity is kept: a page is judged by the epoch and the nonce it echoes.
        self.current = None;
        self.current_nonce.clear();
        self.pending_pages.clear();
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
    /// Whether the window is still there; false once it has retired.
    alive: Arc<AtomicBool>,
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
    /// Counts the changes of the current page, each made under the page lock before the page
    /// changes: a call that checked its page and needs to know, later and without the lock, that
    /// the page has not changed since asks [`InputQueue::page_unchanged`].
    page_epoch: AtomicU64,
    accepting: AtomicBool,
}

impl InputQueue {
    pub fn new(panes: Arc<PaneTable>, arbiter: Arc<InputArbiter>) -> Self {
        let counter = Arc::new(AtomicU64::new(0));
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
                    pending_pages: HashMap::new(),
                    page_ordinals: HashMap::new(),
                    ordinal_order: VecDeque::new(),
                    lanes: HashMap::new(),
                    completions: HashMap::new(),
                    next_ticket: 1,
                    calls: HashMap::new(),
                    calls_swept_at: 0,
                    places: std::collections::HashSet::new(),
                    notices: BTreeMap::new(),
                    notice_cap: MAX_OUTSTANDING_NOTICES,
                    folded: None,
                    failed_windows: std::collections::HashSet::new(),
                    names: HashMap::new(),
                    name_order: VecDeque::new(),
                    counter: Arc::clone(&counter),
                    reaper: false,
                    #[cfg(test)]
                    covers: HashMap::new(),
                    #[cfg(test)]
                    lineage: HashMap::new(),
                }),
                wake: Condvar::new(),
                observer: Mutex::new(None),
                next_loss: counter,
                orphans: Mutex::new(Vec::new()),
            }),
            page_epoch: AtomicU64::new(0),
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
        Ok(self.page.lock())
    }

    /// How many page identities the host holds, for a test that counts what a cut leaves.
    #[cfg(test)]
    fn held_identities(&self) -> usize {
        let state = self.page.lock();
        state.pending_pages.len() + state.page_ordinals.len()
    }

    /// The host's epoch: how many main documents have loaded.
    pub fn epoch(&self) -> u64 {
        self.page.lock().epoch
    }

    /// Start the clock that settles what a bound passes over, if something
    /// waits on one.
    fn watch_bounds(&self, state: &mut PageInputState) {
        if state.reaper || state.next_deadline().is_none() {
            return;
        }
        state.reaper = true;
        let shared = Arc::downgrade(&self.page);
        #[cfg(test)]
        let refused_by_test = FAIL_THREAD_SPAWN.with(|fail| fail.get());
        #[cfg(not(test))]
        let refused_by_test = false;
        let spawned = if refused_by_test {
            Err(std::io::Error::other("a test refused the thread"))
        } else {
            thread::Builder::new()
                .name("consensflow-input-bounds".to_owned())
                .spawn(move || bounds_clock(shared))
        };
        if spawned.is_err() {
            state.reaper = false;
            // No clock will come for what waits: settle it all now, as if its bound had passed,
            // so that nothing stays parked, unanswered or reserved. The gaps are numbered losses.
            let mut events = Vec::new();
            state.pump_all(Instant::now() + ORDER_BOUND * 2, &mut events);
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
    /// recorded for it (a number for the host's own record; the page draws nothing from it).
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
        // A refusal's number can leave a gap the clock must watch, whatever else is parked.
        if let Ok(mut page) = self.state() {
            self.watch_bounds(&mut page);
        }
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
            let call = CallKey {
                key: key.clone(),
                page: page_id.to_owned(),
                epoch,
                sequence,
            };
            // A repeat of a call that still holds bytes in the host is that call's: it has its
            // own outcome (and its loss, if it has one), and the repeat makes no second. Any
            // other call of a departed page is a loss of its window.
            let loss = match page.call_cell(&call) {
                Some(cell) => cell.get(),
                None => Some(page.lost(&key, closed, events)),
            };
            return Err(Refusal {
                error: INPUT_STALE_PAGE.to_string(),
                loss,
                spent: true,
            });
        }
        // The call takes the number a dropped file may have held for it.
        page.places.remove(&(key.clone(), sequence));
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
            // Final for its number: the order goes on past it. A spent number is cheap, so the
            // lane keeps twice what it parks; past that the page is told it is not spent yet.
            let lane = page.lane(&key);
            let spent = lane.slots.len() < 2 * MAX_LANE_SLOTS;
            if spent {
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
                spent,
            })
        };
        if page.lane(&key).slots.len() >= MAX_LANE_SLOTS {
            return refuse(
                &mut page,
                events,
                InputError::queue_full().to_string(),
                false,
            );
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
            InputWork::Claim | InputWork::Enter(_) => {}
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
        let (reserved, sender, alive) = {
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
                Ok(reserved) => (
                    reserved,
                    route.sender_for(&work).clone(),
                    Arc::clone(&route.alive),
                ),
                Err(error) => {
                    drop(senders);
                    return refuse(&mut page, events, error.to_string(), false);
                }
            }
        };
        let cell = Arc::new(CallCell::default());
        let call = CallKey {
            key: key.clone(),
            page: page_id.to_owned(),
            epoch,
            sequence,
        };
        page.remember_call(call.clone(), &cell);
        let job = InputJob {
            work,
            response,
            _reserved: reserved,
            outcome: Some(Outcome::new(&self.page, call, alive, Arc::clone(&cell))),
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
                cell,
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
                spent: true,
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
        self.spend_sequence(page_id, epoch, key, sequence, true)
    }

    /// A number the page took for a gesture and never used (a drop that was refused): spent
    /// like a skip's, with nothing told, for nothing was typed.
    pub fn release_page_sequence(
        &self,
        page_id: &str,
        epoch: u64,
        key: &PaneKey,
        sequence: u64,
    ) -> Result<(), String> {
        self.spend_sequence(page_id, epoch, key, sequence, false)
            .map(drop)
    }

    fn spend_sequence(
        &self,
        page_id: &str,
        epoch: u64,
        key: &PaneKey,
        sequence: u64,
        record: bool,
    ) -> Result<u64, String> {
        let mut events = Vec::new();
        let result = {
            let mut page = self.state()?;
            let now = Instant::now();
            let call = CallKey {
                key: key.clone(),
                page: page_id.to_owned(),
                epoch,
                sequence,
            };
            if !page.admits(page_id, epoch) {
                // A departed page's skip says a call of its own may be missing: that call, if
                // it still holds bytes in the host, has its own loss (or will). A let-go place
                // from a departed page says nothing is missing: it records nothing.
                if record {
                    let closed = !self.has_route(key);
                    let flags = Flags {
                        closed,
                        ..Flags::default()
                    };
                    match page.call_cell(&call) {
                        Some(cell) => {
                            page.lose_call(&cell, key, flags, &mut events);
                        }
                        None => {
                            page.lost_with(key, flags, &mut events);
                        }
                    }
                }
                drop(page);
                self.page.flush(events);
                return Err(INPUT_STALE_PAGE.to_string());
            }
            if sequence == u64::MAX {
                return Err("pane-input-sequence-overflow".to_string());
            }
            page.places.remove(&(key.clone(), sequence));
            page.pump(key, now, &mut events);
            let held = page.call_cell(&call);
            let lane = page.lane(key);
            if let Some((_, loss)) = lane.skipped.iter().find(|(number, _)| *number == sequence) {
                Ok(*loss)
            } else if let Some(loss) = held.as_ref().and_then(|cell| cell.get()).filter(|_| record)
            {
                // The call has its loss already (its writer failed first): this is that one.
                Ok(loss)
            } else if !record && sequence <= lane.passed {
                Ok(0)
            } else if !record && matches!(lane.slots.get(&sequence), Some(Slot::Parked { .. })) {
                // A number that holds a call is not a place nobody used: the call stands, and
                // is written or lost on its own.
                Ok(0)
            } else if sequence > lane.passed
                && !lane.slots.contains_key(&sequence)
                && lane.slots.len() >= 2 * MAX_LANE_SLOTS
            {
                // Its number cannot be spent yet: the skip stays pending, and the page asks again.
                Err(SKIP_PENDING.to_string())
            } else {
                // A skip that is final leaves nothing of the call: a parked original is taken
                // out, so that it never writes when the numbers before it come.
                let mut cancelled = None;
                if sequence > lane.passed {
                    let void = Slot::Void {
                        deadline: now + ORDER_BOUND,
                    };
                    match lane.slots.insert(sequence, void) {
                        Some(Slot::Parked { job, .. }) => cancelled = Some(job),
                        Some(Slot::Void { deadline }) => {
                            lane.slots.insert(sequence, Slot::Void { deadline });
                        }
                        None => {}
                    }
                }
                // The page's own ticket only: the number is its own.
                page.completions.retain(|_, completion| {
                    !(&completion.key == key && completion.sequence == sequence)
                });
                page.pump(key, now, &mut events);
                let closed = !self.has_route(key);
                let loss = if !record {
                    0
                } else {
                    let flags = Flags {
                        closed,
                        ..Flags::default()
                    };
                    match &held {
                        Some(cell) => page.lose_call(cell, key, flags, &mut events),
                        None => page.lost_with(key, flags, &mut events),
                    }
                };
                if let Some(job) = cancelled {
                    job.refuse_told(InputError::refused(
                        INPUT_SEQUENCE_STALE,
                        "the page skipped this call",
                    ));
                }
                if record {
                    let lane = page.lane(key);
                    lane.skipped.push_back((sequence, loss));
                    while lane.skipped.len() > 64 {
                        lane.skipped.pop_front();
                    }
                }
                Ok(loss)
            }
            .inspect(|_| self.watch_bounds(&mut page))
        };
        self.page.flush(events);
        self.page.wake.notify_all();
        result
    }

    /// A drop took number `sequence` of `key` at its gesture, before it looked up its path:
    /// the host holds the place. If the page reloads before the call comes, the host says
    /// the file was not typed; a call with the number, a skip or a release ends the place.
    pub fn place_page_sequence(
        &self,
        page_id: &str,
        epoch: u64,
        key: &PaneKey,
        sequence: u64,
    ) -> Result<(), String> {
        let mut page = self.state()?;
        if !page.admits(page_id, epoch) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        let lane = page.lane(key);
        if lane.slots.contains_key(&sequence) || sequence <= lane.passed {
            // The number was used before this was heard.
            return Ok(());
        }
        if !self.has_route(key) {
            return Ok(());
        }
        if page.places.len() >= MAX_PLACES {
            return Err("pane-input-place-capacity".to_string());
        }
        page.places.insert((key.clone(), sequence));
        Ok(())
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

    /// The page says what a window is called, for the notice of a window that has closed.
    pub fn name_window(
        &self,
        page_id: &str,
        epoch: u64,
        key: &PaneKey,
        name: &str,
    ) -> Result<(), String> {
        let mut page = self.state()?;
        if !page.admits(page_id, epoch) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        page.name_window(key, name);
        Ok(())
    }

    /// The person dismissed the notice with this id, on the current page of the current
    /// epoch: it goes, if it is still outstanding under that id. A notice the host replaced
    /// (its window closed, a name came, a loss came after it) has a new id, and a dismissal of
    /// the old one clears nothing of it. Whether anything went comes back.
    pub fn dismiss_notice(&self, page_id: &str, epoch: u64, id: u64) -> Result<bool, String> {
        let mut page = self.state()?;
        if !page.admits(page_id, epoch) {
            return Err(INPUT_STALE_PAGE.to_string());
        }
        Ok(page.dismiss(id))
    }

    /// What the host still owes the person, for the page that is current to show: it asks at
    /// its activation, after the board loads, on a timer and when it comes into view. Every
    /// outstanding notice, until it is dismissed by its id.
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
    /// is kept until the person dismisses it ([`Self::dismiss_notice`]): an activation whose
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
            if page.page_ordinals.insert(session.clone(), number).is_none() {
                page.ordinal_order.push_back(session);
            }
            // At most `MAX_PAGE_SESSIONS` sessions are remembered, the oldest forgotten. A
            // forgotten session's page is refused by its epoch and nonce all the same.
            while page.ordinal_order.len() > MAX_PAGE_SESSIONS {
                if let Some(old) = page.ordinal_order.pop_front() {
                    page.page_ordinals.remove(&old);
                }
            }
        }
        self.page_epoch.fetch_add(1, Ordering::SeqCst);
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

    /// The page `page_id` as of now, if it is the current page: a count that stays the same until
    /// the current page changes ([`InputQueue::page_unchanged`]). The page's lock is held only
    /// for this.
    pub fn current_page_epoch(&self, page_id: &str) -> Result<u64, String> {
        self.with_current_page(page_id, || self.page_epoch.load(Ordering::SeqCst))
    }

    /// Whether the current page has not changed since `epoch` was read. No lock is taken.
    pub fn page_unchanged(&self, epoch: u64) -> bool {
        self.page_epoch.load(Ordering::SeqCst) == epoch
    }

    /// A new main document has begun loading, whatever page is current or answers: the host's
    /// epoch moves, under the input lock, and nothing of the old page's that is still in
    /// order goes in (see [`PageInputState::cut`]). The first call of the new document finds
    /// its place, and no window waits for a reload.
    pub fn end_page(&self) -> Result<(), String> {
        let mut events = Vec::new();
        {
            let mut page = self.state()?;
            self.page_epoch.fetch_add(1, Ordering::SeqCst);
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
        at: Option<At>,
        read_empty: Option<ReadEmpty>,
        clear_first: Option<Vec<u8>>,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        self.submit(
            key,
            InputWork::Paste {
                body,
                at,
                read_empty,
                clear_first,
            },
        )
    }

    /// An Enter of a delivery, on the pane's worker, in order with its keys and pastes.
    pub(crate) fn enter(
        &self,
        key: PaneKey,
        at: At,
    ) -> Result<oneshot::Receiver<InputResponse>, InputError> {
        self.submit(key, InputWork::Enter(at))
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
                alive: Arc::new(AtomicBool::new(true)),
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
        if let Some(route) = self
            .senders
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(key)
        {
            route.alive.store(false, Ordering::Release);
        }
        self.workers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(key);
        let mut page = self.page.lock();
        let mut events = Vec::new();
        if let Some(lane) = page.lanes.remove(key) {
            if !lane.slots.is_empty() {
                let loss = page.lost(key, true, &mut events);
                for slot in lane.slots.into_values() {
                    if let Slot::Parked { job, .. } = slot {
                        if let Some(cell) = job.cell() {
                            let _ = cell.take(loss);
                        }
                        job.refuse_told(InputError::refused(
                            INPUT_CLOSED,
                            "pane input queue is closed",
                        ));
                    }
                }
            }
        }
        // A notice of a window that has closed says so: it is a new notice under a new id, so
        // that a Dismiss of the old one clears nothing of it.
        let open: Vec<u64> = page
            .notices
            .iter()
            .filter(|(_, notice)| {
                matches!(&notice.body, NoticeBody::Window { key: of, closed: false, .. } if of == key)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in open {
            if let Some(fresh) = page.replace(id, |body| {
                if let NoticeBody::Window { closed, .. } = body {
                    *closed = true;
                }
            }) {
                if page.current.is_some() {
                    events.push(Event::Lost(key.clone(), fresh));
                }
            }
        }
        // A file dropped into a window that closes before the page reloads is told, as a
        // file, in a notice of the closed window.
        let held: Vec<(PaneKey, u64)> = page
            .places
            .iter()
            .filter(|(of, _)| of == key)
            .cloned()
            .collect();
        for place in held {
            page.places.remove(&place);
            page.lost_with(
                key,
                Flags {
                    closed: true,
                    dropped: true,
                    ..Flags::default()
                },
                &mut events,
            );
        }
        page.failed_windows.remove(key);
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
        let lanes = std::mem::take(&mut self.page.lock().lanes);
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
            let mut state = shared.lock();
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
        let result = match job.work {
            InputWork::Typed(bytes) => arbiter.write_typed(&panes, &key, &bytes),
            InputWork::Write(bytes) | InputWork::Reply(bytes) => {
                arbiter.write(&panes, &key, &bytes)
            }
            InputWork::Paste {
                body,
                at,
                read_empty,
                clear_first,
            } => arbiter.write_paste(
                &panes,
                &key,
                Paste {
                    body: &body,
                    at,
                    read_empty,
                    clear_first: clear_first.as_deref(),
                },
            ),
            InputWork::Enter(at) => arbiter.enter(&*panes, &key, at),
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
        fn dismiss(&self, id: u64) -> Result<bool, String>;
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
            Ok(notices
                .into_iter()
                .filter_map(|notice| match notice.body {
                    NoticeBody::Window { key, .. } => Some((key, notice.id)),
                    NoticeBody::Folded { .. } => None,
                })
                .collect())
        }
        fn dismiss(&self, id: u64) -> Result<bool, String> {
            let page = self
                .page
                .state
                .lock()
                .unwrap()
                .current
                .clone()
                .expect("a page is current");
            self.dismiss_notice(&page, self.epoch(), id)
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
                alive: Arc::new(AtomicBool::new(true)),
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
    fn a_refused_number_lets_the_next_go_in_at_once() {
        let (inputs, key, received, _) = bare();
        inputs.begin_page("page").unwrap();
        let over = InputWork::Typed(vec![b'x'; crate::validation::MAX_PAGE_INPUT_BYTES + 1]);
        assert!(inputs.enqueue("page", key.clone(), 1, over, true).is_err());
        inputs
            .enqueue("page", key.clone(), 2, typed(b"B"), true)
            .unwrap();
        assert_eq!(
            handed(&received),
            b"B",
            "the order goes on past the refused number"
        );
        inputs.close_and_drain();
    }

    /// A refusal is final for its number: nothing was written, and the next goes on.
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
                alive: Arc::new(AtomicBool::new(true)),
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
                alive: Arc::new(AtomicBool::new(true)),
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
                alive: Arc::new(AtomicBool::new(true)),
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
                    && line
                        .split("had not come after ")
                        .nth(1)
                        .and_then(|rest| rest.split(' ').next())
                        .and_then(|ms| ms.parse::<u64>().ok())
                        .is_some_and(|ms| (1400..2500).contains(&ms))),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("late call numbered 1")
                    && line
                        .split("given up ")
                        .nth(1)
                        .and_then(|rest| rest.split(' ').next())
                        .and_then(|ms| ms.parse::<u64>().ok())
                        .is_some_and(|ms| ms >= 200)),
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
        assert_eq!(
            handed(&received),
            b"",
            "B waited behind a missing number: it is cancelled"
        );
        inputs.prepare("s:2").unwrap();
        let lost = inputs.activate("s:2").unwrap();
        assert_eq!(lost.len(), 1);
        assert_eq!(lost[0].0, key);
        // The answer was lost at the page, which asks again, and gets the same notice.
        assert_eq!(inputs.activate("s:2").unwrap(), lost);
        assert_eq!(inputs.dismiss(lost[0].1), Ok(true));
        assert!(inputs.activate("s:2").unwrap().is_empty());
        inputs.close_and_drain();
    }

    /// A notice dismissed by its id clears only that notice: a later loss of the window is a
    /// new notice under a new id.
    #[test]
    fn a_dismissal_clears_only_the_notice_of_its_id() {
        let (inputs, key, _received, _) = bare();
        inputs.begin_page("s:1").unwrap();
        inputs.skip("s:1", &key, 1).unwrap();
        let first = inputs.activate("s:1").unwrap();
        inputs.skip("s:1", &key, 2).unwrap();
        let second = inputs.activate("s:1").unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert!(second[0].1 > first[0].1);
        assert_eq!(
            inputs.dismiss(first[0].1),
            Ok(false),
            "the replaced id is nothing"
        );
        assert_eq!(inputs.activate("s:1").unwrap(), second);
        assert_eq!(inputs.dismiss(second[0].1), Ok(true));
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
        // What it parks, and the spent numbers of what it was refused, twice as many at most.
        assert!(
            held > MAX_LANE_SLOTS && held <= 2 * MAX_LANE_SLOTS,
            "{held}"
        );
        inputs.close_and_drain();
    }

    /// The host keeps no more notices than its bound.
    #[test]
    fn the_notices_a_host_keeps_are_bounded() {
        let (inputs, _key, _received, _) = bare();
        inputs.begin_page("page").unwrap();
        for window in 0..(MAX_OUTSTANDING_NOTICES as u64 + 20) {
            let key = PaneKey::new(format!("window-{window}"), 1);
            inputs.skip("page", &key, 1).unwrap();
        }
        assert_eq!(
            inputs.activate("page").unwrap().len(),
            MAX_OUTSTANDING_NOTICES
        );
        inputs.close_and_drain();
    }
}
