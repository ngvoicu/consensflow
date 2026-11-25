use std::collections::{HashMap, VecDeque};
#[cfg(any(test, feature = "test-support"))]
use std::io::Read;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use portable_pty::{Child, MasterPty, PtySize};

use crate::background::Baseline;
use crate::ended::EndedSlot;
use crate::journal::Journal;
use crate::screen::{Drawn, PaneScreen};

mod close;
#[cfg(windows)]
mod conpty;
#[cfg(all(test, windows))]
pub(crate) mod conpty_test;
mod error;
mod flow;
mod holding;
#[cfg(test)]
mod journal_tests;
#[cfg(test)]
mod keys;
mod output_end;
mod owed;
#[cfg(all(test, windows))]
mod probe;
#[cfg(all(any(test, feature = "test-support"), unix))]
mod sentinel;
mod spawn;
mod teardown;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
mod turn;

pub use close::CLOSE_GRACE;
pub use error::PaneError;
#[cfg(all(test, unix))]
use flow::terminal_of;
use flow::OutputFlow;
pub(crate) use output_end::Handed;
pub use owed::ExitOwed;
#[cfg(all(any(test, feature = "test-support"), unix))]
pub use sentinel::Sentinel;
pub(crate) use spawn::validate_drop_env;
use teardown::{has_exited, wait_for_exit, Ending, Teardown};
#[cfg(all(any(test, feature = "test-support"), unix))]
pub use test_support::process_exists;
#[cfg(any(test, feature = "test-support"))]
use test_support::HoldState;
#[cfg(test)]
use test_support::TERMINAL_FAILS;
#[cfg(all(test, windows))]
use test_support::WATCH_FAILS;
#[cfg(any(test, feature = "test-support"))]
pub use test_support::{serial_pty_test, OpenHold, SerialPtyTest};

/// How long a pane whose output has ended waits for its program's exit to be
/// readable: a program closes its terminal before it has finished exiting,
/// and a large one takes a while to let go of its memory.
const EXIT_GRACE: Duration = Duration::from_secs(5);
/// How long ConPTY is given, once a pane's program has exited, to pass on
/// what the program printed last.
#[cfg(windows)]
const EXIT_FLUSH: Duration = Duration::from_secs(1);

/// How many windows killed before they opened a table keeps ([`PaneTable::kill`]): an
/// open the daemon gave up on reaches the host a moment after its kill, and the kills
/// of windows long gone, which find nothing too, make room for the latest.
const CANCELLED_KEPT: usize = 256;

/// How many journals of windows that have ended a table keeps for their page to pull.
const ENDED_JOURNALS_KEPT: usize = 16;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PaneKey {
    pub id: String,
    pub generation: u64,
}

impl PaneKey {
    pub fn new(id: impl Into<String>, generation: u64) -> Self {
        Self {
            id: id.into(),
            generation,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneInfo {
    pub id: String,
    pub generation: u64,
    pub alive: bool,
    pub idle_ms: u64,
}

#[cfg(any(test, feature = "test-support"))]
pub struct OpenedPane {
    pub key: PaneKey,
    pub reader: Box<dyn Read + Send>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneOutput {
    pub key: PaneKey,
    pub seq: u64,
    pub bytes: Vec<u8>,
}

pub struct StreamedPane {
    pub key: PaneKey,
    /// The window's program, when its process id is known.
    pub pid: Option<u32>,
    pub output: mpsc::Receiver<PaneOutput>,
    /// What the window's screen shows: whoever reads `output` feeds it.
    pub screen: Arc<PaneScreen>,
    /// The window's journal, where the table keeps them ([`PaneTable::keep_journals`]).
    pub journal: Option<Arc<Journal>>,
    /// How the window's program ended, once anyone read its status.
    pub ended: Arc<EndedSlot>,
    /// The window's exit, owed to the daemon until whoever reads `output` is done: the table
    /// is asked whether every one has been told ([`PaneTable::exits_told_within`]).
    pub owed: ExitOwed,
    /// The window's process tree, which its exit asks what may be the window's and was not
    /// ended (`ProcessTree::possibly_left`): the pane may have left the table by then. On
    /// macOS only: on Windows the tree is the pane's job, ended as its last handle closes,
    /// and a handle the exit held would end it only after the exit (Calliope's review T-421).
    #[cfg(target_os = "macos")]
    pub(crate) tree: crate::process_tree::ProcessTree,
}

/// The child environment carried by `pane.open`: inherit the parent, apply
/// `overlay`, then remove every validated name in `drop`.
pub struct PaneEnvironment<'a> {
    overlay: &'a HashMap<String, String>,
    drop: &'a [String],
}

impl<'a> PaneEnvironment<'a> {
    pub fn new(overlay: &'a HashMap<String, String>, drop: &'a [String]) -> Self {
        Self { overlay, drop }
    }
}

pub(crate) trait PaneInputWriter {
    fn write(&self, key: &PaneKey, bytes: &[u8]) -> Result<(), PaneError>;
}

struct Pane {
    /// Before the master, so that the terminal's input is let go before the master closes
    /// it: on Windows a console that waits on its input is then told it has ended (`conpty`).
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    process_group_id: Option<i32>,
    #[cfg(target_os = "macos")]
    process_tree: crate::process_tree::ProcessTree,
    #[cfg(windows)]
    job: Arc<crate::job_object::JobObject>,
    /// The processes the pane's tree had when its turn began (`pane.mark`).
    baseline: Baseline,
    /// What the host saw of the wall clock since the pane opened (`background::clock`).
    #[cfg(target_os = "macos")]
    clock: Arc<Mutex<crate::background::PaneClock>>,
    output_flow: Option<Arc<OutputFlow>>,
    journal: Option<Arc<Journal>>,
    active_writes: Arc<AtomicU64>,
    alive: bool,
    last_activity: Arc<Mutex<Instant>>,
    screen: Arc<PaneScreen>,
    ended: Arc<EndedSlot>,
}

pub struct PaneTable {
    panes: Mutex<HashMap<PaneKey, Pane>>,
    /// The windows killed before their open reached the table, the latest
    /// [`CANCELLED_KEPT`]: one of them never opens. Taken under `panes`' lock,
    /// after it, by a kill and an open alike.
    cancelled: Mutex<VecDeque<PaneKey>>,
    #[cfg(any(test, feature = "test-support"))]
    next_id: AtomicU64,
    updating: AtomicBool,
    teardown: Arc<Teardown>,
    ending: Arc<Ending>,
    /// The exits of the windows whose output is read, which are yet to be told.
    exits: Arc<owed::ExitsOwed>,
    /// How long a closed window's programs are given to end once asked to
    /// ([`CLOSE_GRACE`]): a test gives less.
    close_grace: Duration,
    /// Whether windows opened from now on keep a journal.
    journals: AtomicBool,
    /// The journals of windows that have ended, the last [`ENDED_JOURNALS_KEPT`], until their
    /// page forgets them ([`PaneTable::forget_ended_journal`]).
    ended_journals: Mutex<VecDeque<(PaneKey, Arc<Journal>)>>,
    /// A test's hold on the opens that reach the table ([`PaneTable::hold_opens`]).
    #[cfg(any(test, feature = "test-support"))]
    open_hold: Mutex<Option<Arc<HoldState>>>,
}

/// What is left of the windows a table has held: the panes in it, the closes that took a pane
/// out and have not read its program's end, and the programs whose end could not be proven.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pending {
    /// Panes in the table.
    pub open: usize,
    /// Closes that took a pane out of the table and are still ending its program (a kill that
    /// is waiting out its grace, a detached close).
    pub finishing: u64,
    /// Programs whose end no close could prove: they may still run, and nothing clears this
    /// but the app's quit.
    pub unconfirmed: u64,
}

impl Pending {
    /// Whether no program of the table's windows can still run.
    pub fn is_clean(&self) -> bool {
        self.open == 0 && self.finishing == 0 && self.unconfirmed == 0
    }
}

/// The installer owns admission until restart, or until failure drops this guard.
pub struct UpdatePermit(Arc<PaneTable>);

impl Drop for UpdatePermit {
    fn drop(&mut self) {
        self.0.updating.store(false, Ordering::Release);
    }
}
impl PaneTable {
    pub fn new() -> Self {
        Self::closing_within(CLOSE_GRACE)
    }

    /// A table whose closed windows' programs are given `grace` to end once
    /// asked to, where [`PaneTable::new`]'s are given [`CLOSE_GRACE`]: a test of
    /// the app's quit gives less than the grace it takes. On Windows nothing is
    /// asked and no grace is given, whichever it is.
    pub fn closing_within(grace: Duration) -> Self {
        Self {
            panes: Mutex::new(HashMap::new()),
            cancelled: Mutex::new(VecDeque::new()),
            #[cfg(any(test, feature = "test-support"))]
            next_id: AtomicU64::new(1),
            updating: AtomicBool::new(false),
            teardown: Arc::new(Teardown::default()),
            ending: Arc::new(Ending::default()),
            exits: Arc::new(owed::ExitsOwed::default()),
            close_grace: grace,
            journals: AtomicBool::new(false),
            ended_journals: Mutex::new(VecDeque::new()),
            #[cfg(any(test, feature = "test-support"))]
            open_hold: Mutex::new(None),
        }
    }

    pub fn begin_update(self: &Arc<Self>) -> Result<UpdatePermit, PaneError> {
        let panes = self.lock_panes()?;
        let (finishing, unconfirmed) = self.teardown.counts();
        if !panes.is_empty() || finishing != 0 || unconfirmed != 0 {
            return Err(PaneError::UpdateBlocked);
        }
        if self.updating.swap(true, Ordering::AcqRel) {
            return Err(PaneError::Updating);
        }
        Ok(UpdatePermit(Arc::clone(self)))
    }

    /// What is left of the windows the table has held, for whoever must know that no
    /// program of theirs can still run ([`Pending::is_clean`]).
    pub fn pending(&self) -> Result<Pending, PaneError> {
        let panes = self.lock_panes()?;
        let (finishing, unconfirmed) = self.teardown.counts();
        Ok(Pending {
            open: panes.len(),
            finishing,
            unconfirmed,
        })
    }

    pub fn update_blockers(&self) -> Result<Vec<PaneKey>, PaneError> {
        let panes = self.lock_panes()?;
        let mut keys: Vec<_> = panes.keys().cloned().collect();
        let (finishing, unconfirmed) = self.teardown.counts();
        for i in 0..finishing {
            keys.push(PaneKey::new(format!("finishing-pane-cleanup-{i}"), 0));
        }
        // Named apart from the transient case: this one never clears on its own.
        for i in 0..unconfirmed {
            keys.push(PaneKey::new(format!("unconfirmed-pane-cleanup-{i}"), 0));
        }
        keys.sort();
        Ok(keys)
    }

    pub(crate) fn write(&self, key: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
        let (writer, last_activity, active_writes) = {
            let panes = self.lock_panes()?;
            let pane = panes
                .get(key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?;
            pane.active_writes.fetch_add(1, Ordering::AcqRel);
            (
                Arc::clone(&pane.writer),
                Arc::clone(&pane.last_activity),
                Arc::clone(&pane.active_writes),
            )
        };
        let _active_write = ActiveWrite(active_writes);
        let mut writer = writer.lock().map_err(|_| PaneError::LockPoisoned)?;
        writer.write_all(bytes)?;
        writer.flush()?;
        *last_activity.lock().map_err(|_| PaneError::LockPoisoned)? = Instant::now();
        Ok(())
    }

    /// How many writes into the pane are in flight, counted from before the write waits for the
    /// pane's writer: a test that must have a write in hand before it does the next thing waits
    /// for this and not for a time (T-1332).
    #[cfg(all(test, unix))]
    pub(crate) fn writes_in_flight(&self, key: &PaneKey) -> Option<u64> {
        let panes = self.lock_panes().ok()?;
        panes
            .get(key)
            .map(|pane| pane.active_writes.load(Ordering::Acquire))
    }

    pub fn resize(&self, key: &PaneKey, rows: u16, cols: u16) -> Result<(), PaneError> {
        self.resize_at(key, rows, cols, None)
    }

    /// [`PaneTable::resize`] of a window whose page's emulator took the size once it had
    /// parsed `at` bytes of the window's output (`None`: the size is not the current page's,
    /// and takes the journal's end).
    pub fn resize_at(
        &self,
        key: &PaneKey,
        rows: u16,
        cols: u16,
        at: Option<u64>,
    ) -> Result<(), PaneError> {
        let mut panes = self.lock_panes()?;
        let pane = panes
            .get_mut(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?;
        pane.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| PaneError::Pty(error.to_string()))?;
        pane.screen.resize(rows, cols);
        if let Some(journal) = &pane.journal {
            journal.resize(at, (rows, cols));
        }
        *pane
            .last_activity
            .lock()
            .map_err(|_| PaneError::LockPoisoned)? = Instant::now();
        Ok(())
    }

    /// The last `max` lines a pane's screen shows (see
    /// [`PaneScreen::tail`]); the pane must be in the table.
    pub fn tail(&self, key: &PaneKey, max: usize) -> Result<Vec<String>, PaneError> {
        let screen = Arc::clone(
            &self
                .lock_panes()?
                .get(key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?
                .screen,
        );
        Ok(screen.tail(max))
    }

    /// The last `max` lines of a pane's screen as it is drawn ([`PaneScreen::bottom`]).
    pub fn bottom(&self, key: &PaneKey, max: usize) -> Result<Drawn, PaneError> {
        let screen = Arc::clone(
            &self
                .lock_panes()?
                .get(key)
                .ok_or_else(|| PaneError::NotFound(key.clone()))?
                .screen,
        );
        Ok(screen.bottom(max))
    }

    pub fn list(&self) -> Result<Vec<PaneInfo>, PaneError> {
        let mut panes = self.lock_panes()?;
        let now = Instant::now();
        let mut listed = Vec::with_capacity(panes.len());
        for (key, pane) in panes.iter_mut() {
            let status = pane.child.try_wait()?;
            if let Some(status) = &status {
                pane.ended.record(status);
            }
            let alive = status.is_none();
            if pane.alive && !alive {
                *pane
                    .last_activity
                    .lock()
                    .map_err(|_| PaneError::LockPoisoned)? = now;
            }
            pane.alive = alive;
            let last_activity = *pane
                .last_activity
                .lock()
                .map_err(|_| PaneError::LockPoisoned)?;
            let idle_ms = now.duration_since(last_activity).as_millis();
            listed.push(PaneInfo {
                id: key.id.clone(),
                generation: key.generation,
                alive,
                idle_ms: idle_ms.min(u128::from(u64::MAX)) as u64,
            });
        }
        listed
            .sort_by(|left, right| (&left.id, left.generation).cmp(&(&right.id, right.generation)));
        Ok(listed)
    }

    pub(crate) fn process_group_id(&self, key: &PaneKey) -> Result<i32, PaneError> {
        self.lock_panes()?
            .get(key)
            .ok_or_else(|| PaneError::NotFound(key.clone()))?
            .process_group_id
            .ok_or_else(|| PaneError::Pty("the PTY has no process group".to_string()))
    }

    fn lock_panes(&self) -> Result<MutexGuard<'_, HashMap<PaneKey, Pane>>, PaneError> {
        self.panes.lock().map_err(|_| PaneError::LockPoisoned)
    }
}

impl PaneInputWriter for PaneTable {
    fn write(&self, key: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
        PaneTable::write(self, key, bytes)
    }
}

/// A paste in brackets, then its Enter as a write of its own, once
/// `before_enter` returns: a harness takes an Enter that arrives with the
/// paste into it.
pub(crate) fn write_paste_via<W: PaneInputWriter + ?Sized>(
    writer: &W,
    key: &PaneKey,
    body: &[u8],
    before_enter: impl FnOnce(),
) -> Result<(), PaneError> {
    let mut bracketed = Vec::with_capacity(12 + body.len());
    bracketed.extend_from_slice(b"\x1b[200~");
    bracketed.extend_from_slice(body);
    bracketed.extend_from_slice(b"\x1b[201~");
    writer.write(key, &bracketed)?;
    before_enter();
    writer.write(key, b"\r")
}

struct ActiveWrite(Arc<AtomicU64>);

impl Drop for ActiveWrite {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Default for PaneTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
