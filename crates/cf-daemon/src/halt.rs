//! Whether the daemon is ending, and whether it is ending because its ledger broke:
//! the one thing every outward effect asks right before it acts (a file written, a
//! spec copied or opened, an event appended, a window's line queued, a page's answer
//! that says it worked), and the log asks before it writes (a log is written off the
//! runtime from the first moment of a stop or a break).
//!
//! It is the latch's ([`crate::stop::Latch`]), shared by `Arc` so that a blocking thread
//! can read it too. A stop sets [`Halt::stopping`] and a ledger that cannot save sets
//! [`Halt::broke`] as well, whichever came first and even where a stop is already under
//! way: the process's status is the break's (74) wherever the stop began.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The daemon's ending, as every effect reads it.
#[derive(Debug, Default)]
pub struct Halt {
    stopping: AtomicBool,
    broke: AtomicBool,
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

/// What an operation says when it was admitted before the break and finished after it.
pub const BROKEN_WORDS: &str =
    "ConsensFlow can't save to its ledger any more and is stopping: nothing was changed";

/// What an operation says when the daemon is ending and it was not begun in time.
pub const STOPPING_WORDS: &str = "the daemon is stopping: nothing was changed";

impl Halt {
    /// The refusal an effect makes right before it acts, or none while the daemon runs.
    pub fn refusal(&self) -> Option<&'static str> {
        if self.broke() {
            Some(BROKEN_WORDS)
        } else if self.stopping() {
            Some(STOPPING_WORDS)
        } else {
            None
        }
    }
}
