//! Whether the daemon is ending, and whether it is ending because its ledger broke:
//! the one thing every outward effect asks right before it acts (a file written, a
//! spec copied or opened, an event appended, a window's line queued, a page's answer
//! that says it worked), and the log asks before it writes (a log is written off the
//! runtime from the first moment of a stop or a break).
//!
//! An effect made on another thread than the one the break is declared on is made under
//! the halt's one hold ([`Halt::act`]), which the break waits for before the app is told
//! ([`Halt::settled`]).
//!
//! It is the latch's ([`crate::stop::Latch`]), shared by `Arc` so that a blocking thread
//! can read it too. A stop sets [`Halt::stopping`] and a ledger that cannot save sets
//! [`Halt::broke`] as well, whichever came first and even where a stop is already under
//! way: the process's status is the break's (74) wherever the stop began.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

/// The daemon's ending, as every effect reads it.
#[derive(Debug, Default)]
pub struct Halt {
    stopping: AtomicBool,
    broke: AtomicBool,
    /// How many effects are under way off the ledger's thread, and the wait for none.
    under_way: Mutex<usize>,
    idle: Condvar,
}

/// One effect counted as under way, until it is dropped (a panic in it drops it too).
struct Held<'a>(&'a Halt);

impl Drop for Held<'_> {
    fn drop(&mut self) {
        *self.0.count() -= 1;
        self.0.idle.notify_all();
    }
}

impl Halt {
    /// Nothing has ended.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The daemon is ending, for whatever reason.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
    }

    /// The ledger cannot save: the daemon is ending, and its status says why.
    pub fn break_ledger(&self) {
        self.broke.store(true, Ordering::SeqCst);
        self.stop();
    }

    /// Whether the daemon is ending.
    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    /// Whether the ledger cannot save.
    pub fn broke(&self) -> bool {
        self.broke.load(Ordering::SeqCst)
    }
}

/// What an operation that was refused before it did anything says: nothing was changed.
pub const BROKEN_REFUSED: &str =
    "ConsensFlow can't save to its ledger any more and is stopping: nothing was changed";

/// What the page is told beside an answer that was made before the break and is true: its
/// change was made. The break is said after it.
pub const BROKEN_WORDS: &str = "ConsensFlow can't save to its ledger any more and is stopping, so \
     nothing more will be saved. What was asked was done before; check the board after it \
     is open again";

/// What an operation says when the daemon is ending and it was not begun in time.
pub const STOPPING_WORDS: &str = "the daemon is stopping: nothing was changed";

impl Halt {
    /// An outward effect made off the ledger's thread: the hold taken, the refusal asked,
    /// the act made, the hold let go. `Err` is the refusal's words; the act did not run.
    /// The break sets its flags before it waits for the hold, so an act that takes it after
    /// them refuses, and one that took it before is over before the app is told, unless the
    /// system holds it longer than the break waits.
    pub fn act<T>(&self, act: impl FnOnce() -> T) -> Result<T, &'static str> {
        *self.count() += 1;
        let _held = Held(self);
        match self.refusal() {
            Some(words) => Err(words),
            None => Ok(act()),
        }
    }

    /// The break's wait for the acts under way, `within` at most: false where one outlasted it.
    pub fn settled(&self, within: Duration) -> bool {
        let (count, _) = self
            .idle
            .wait_timeout_while(self.count(), within, |count| *count > 0)
            .unwrap_or_else(PoisonError::into_inner);
        *count == 0
    }

    fn count(&self) -> std::sync::MutexGuard<'_, usize> {
        self.under_way
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The refusal an effect makes right before it acts, or none while the daemon runs.
    pub fn refusal(&self) -> Option<&'static str> {
        if self.broke() {
            Some(BROKEN_REFUSED)
        } else if self.stopping() {
            Some(STOPPING_WORDS)
        } else {
            None
        }
    }
}
