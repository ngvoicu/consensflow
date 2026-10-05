//! A panic is what an exception was: caught where work runs, written down,
//! and gone past. Node went on after a throw nobody caught
//! (`uncaughtException`, `unhandledRejection`, `src/core/daemon.js:60-65`),
//! logging it and tracing it as `daemon.error`; the daemon goes on after a
//! panic the same way, at each place that runs work for someone: a pass, a
//! page operation, a request, a task that goes on apart, an event of the host.
//!
//! The hook ([`install_hook`]) only notes where the panic was; it writes
//! nothing and touches no ledger, which may be borrowed while the panic
//! unwinds. What is written, and how it is answered, is the catcher's.

use std::any::Any;
use std::cell::RefCell;
use std::future::Future;
use std::panic::{self, AssertUnwindSafe};
use std::rc::Rc;

use futures_util::FutureExt;

use crate::files::{Log, Trace};

thread_local! {
    /// Where the last panic of this thread was, for whoever catches it.
    static WHERE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// A panic that was caught: what it said, and where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Panicked {
    pub message: String,
    /// `file:line:column`, when the hook noted it.
    pub location: Option<String>,
}

impl Panicked {
    fn caught(payload: Box<dyn Any + Send>) -> Self {
        let message = payload
            .downcast_ref::<&str>()
            .map(|words| (*words).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic with no words".to_owned());
        Self {
            message,
            location: WHERE.with(|noted| noted.borrow_mut().take()),
        }
    }

    /// What the log says under the line: the words, and where they came from.
    pub fn cause(&self) -> String {
        match &self.location {
            Some(location) => format!("panic: {}\n    at {location}", self.message),
            None => format!("panic: {}", self.message),
        }
    }
}

/// Notes where each panic is, in place of the system's hook, which would
/// write to the error output of a process whose error output the app keeps
/// for its own. Called once, by the process that is the daemon.
pub fn install_hook() {
    panic::set_hook(Box::new(|info| {
        let place = info
            .location()
            .map(|at| format!("{}:{}:{}", at.file(), at.line(), at.column()));
        WHERE.with(|noted| *noted.borrow_mut() = place);
    }));
}

/// `work`, or the panic it made, wherever in its running (its first poll
/// too). What it held (a borrow, a hold) is let go as it unwinds.
pub async fn contain<F: Future>(work: F) -> Result<F::Output, Panicked> {
    AssertUnwindSafe(work)
        .catch_unwind()
        .await
        .map_err(Panicked::caught)
}

/// `work`, run now, or the panic it made.
pub fn contain_now<T>(work: impl FnOnce() -> T) -> Result<T, Panicked> {
    panic::catch_unwind(AssertUnwindSafe(work)).map_err(Panicked::caught)
}

/// Where a panic that was caught is written down: the log, and the trace as
/// `daemon.error`. Neither reaches the ledger.
pub struct Errors {
    log: Rc<Log>,
    trace: Rc<Trace>,
}

impl Errors {
    /// Panics caught are written to `log` and `trace`.
    pub fn new(log: Rc<Log>, trace: Rc<Trace>) -> Self {
        Self { log, trace }
    }

    /// The log a failure is written in, which says the rest of what is worth
    /// knowing too.
    pub fn log(&self) -> &Log {
        &self.log
    }

    /// `what` failed with `panicked`: the log says `what` with the panic
    /// underneath, and the trace says `what: <its words>`.
    pub fn caught(&self, what: &str, panicked: &Panicked) {
        self.log.error(what, Some(&panicked.cause()));
        self.trace.error(&format!("{what}: {}", panicked.message));
    }

    /// `work`, run apart on this thread: a panic in it is caught here and
    /// written down as `what`, and nothing else is the worse for it.
    pub fn spawn(self: &Rc<Self>, what: &'static str, work: impl Future<Output = ()> + 'static) {
        let errors = Rc::clone(self);
        drop(tokio::task::spawn_local(async move {
            if let Err(panicked) = contain(work).await {
                errors.caught(what, &panicked);
            }
        }));
    }
}

#[cfg(test)]
mod tests;
