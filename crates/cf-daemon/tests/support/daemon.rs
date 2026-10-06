//! The daemon's own parts, as every player builds them: the executor the
//! engine's work runs on, with the daemon's log and trace in a folder of their
//! own, and the wake-ups of the dispatcher that a handler asks for, counted.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use cf_daemon::errors::Errors;
use cf_daemon::files::{Log, Trace};
use cf_daemon::seams::DaemonSpawn;

/// The executor, driven, and the daemon's log and trace in `folder`, which
/// what a handler is given holds too.
pub fn executor(folder: &Path) -> (Rc<DaemonSpawn>, Rc<Log>, Rc<Trace>) {
    let log = Rc::new(Log::new(folder));
    let trace = Rc::new(Trace::new(folder));
    let spawn = Rc::new(DaemonSpawn::new(Rc::new(Errors::new(
        Rc::clone(&log),
        Rc::clone(&trace),
    ))));
    spawn.drive();
    (spawn, log, trace)
}

/// The wake-ups of the dispatcher (`changed()`) the handlers asked for.
pub struct Kicks {
    count: Rc<Cell<usize>>,
    counted: Cell<usize>,
}

impl Kicks {
    pub fn new() -> Self {
        Self {
            count: Rc::new(Cell::new(0)),
            counted: Cell::new(0),
        }
    }

    /// What a handler is given to wake the dispatcher with.
    pub fn waker(&self) -> Rc<dyn Fn()> {
        let count = Rc::clone(&self.count);
        Rc::new(move || count.set(count.get() + 1))
    }

    /// How many times the dispatcher was woken since this was last asked.
    pub fn take(&self) -> usize {
        let total = self.count.get();
        total - self.counted.replace(total)
    }
}
