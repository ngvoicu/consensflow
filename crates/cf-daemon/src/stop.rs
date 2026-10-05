//! The stop (`stop`, `src/core/daemon.js:109-125`): one latch for every way the
//! daemon is asked to end, one deadline for the waiting, and a tail that
//! waits for nothing.
//!
//! - **The latch** is tripped by the first of: the bridge's input ended (the
//!   app closed the daemon's input, or the daemon was started on a terminal
//!   and it was closed), the bridge failed (a broken output comes this way),
//!   SIGTERM or SIGINT (on Windows Ctrl-C and the end of the input are all
//!   there is), and a broken pipe on the standard streams. The first reason
//!   wins, and the daemon stops once. It is armed before the handle line, so
//!   nothing the app does after reading it finds the daemon unable to stop.
//! - **The deadline** is one second, for everything that waits at once: the
//!   pass loop starts no pass and waits for the one running, and the API says
//!   its doors are answered, lets go of its listener and asks each connection
//!   to finish. What is open at the deadline is dropped. The app ends the
//!   daemon 2 s after asking it to stop, and its own exit hooks must run
//!   first.
//! - **The tail** has no wait in it: the children are ended without waiting,
//!   the ledger is closed, `exit 0` is written, and the process exits. It
//!   never closes the bridge (a close refuses what waits on it, and the engine
//!   would record a launch failed that Node left for the next start to
//!   settle), and it never drops the runtime, whose read of its input cannot
//!   be cancelled.

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use cf_ledger::Ledger;
use cf_process::{megabytes, rss};
use tokio::sync::watch;

use crate::api::Api;
use crate::errors::{contain_now, Errors};
use crate::pass::PassLoop;

/// How long a stop waits, all of it together.
pub const DEADLINE: Duration = Duration::from_secs(1);

/// The reason the daemon stops, said once: the first to trip it wins.
pub struct Latch {
    reason: watch::Sender<Option<String>>,
}

impl Latch {
    /// A latch nobody has tripped.
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            reason: watch::Sender::new(None),
        })
    }

    /// Asks the daemon to stop, for `why`; a daemon already asked is asked once.
    pub fn trip(&self, why: &str) {
        self.reason.send_if_modified(|reason| {
            if reason.is_some() {
                return false;
            }
            *reason = Some(why.to_owned());
            true
        });
    }

    /// Why it was tripped, or none if it was not.
    pub fn reason(&self) -> Option<String> {
        self.reason.borrow().clone()
    }

    /// Waits until it is tripped: what it was tripped for.
    pub async fn tripped(&self) -> String {
        let mut reason = self.reason.subscribe();
        let tripped = reason
            .wait_for(Option::is_some)
            .await
            .map(|why| why.clone().unwrap_or_default());
        // The latch is held by whoever waits: never dropped meanwhile.
        tripped.unwrap_or_default()
    }
}

/// What a stop ends. The pass loop is made after the stop is armed, so it is
/// told here when it is.
pub struct Stopping {
    pub errors: Rc<Errors>,
    pub passes: Rc<OnceCell<PassLoop>>,
    pub api: Rc<Api>,
    /// Ends the children the daemon started, waiting for none: the daemon's own
    /// is the system processes' `end_all`.
    pub ends_children: Rc<dyn Fn()>,
    pub ledger: Rc<RefCell<Ledger>>,
    /// How the process ends: the daemon's own exits; a test's is told it.
    pub exit: Rc<dyn Fn(i32)>,
}

impl Stopping {
    /// The stop, for `why`: ends with the process's exit.
    pub async fn run(&self, why: &str) {
        let size = rss().map_or(0, megabytes);
        self.errors
            .log()
            .info(&format!("stop: {why}; rss {size} MB"));
        let waiting = async {
            let passes = async {
                if let Some(passes) = self.passes.get() {
                    passes.stop().await;
                }
            };
            tokio::join!(passes, self.api.close());
        };
        if tokio::time::timeout(DEADLINE, waiting).await.is_err() {
            // What did not end in time is dropped: the connections with
            // their requests, and the pass left behind, whose work is
            // settled at the next start.
            self.api.drop_connections();
        }
        self.tail();
    }

    /// What is left to do, with no wait in it. Each step is its own: one that
    /// fails or panics leaves the rest, and the process ends.
    pub(crate) fn tail(&self) {
        if let Err(panicked) = contain_now(|| (self.ends_children)()) {
            self.errors.caught("the children did not end", &panicked);
        }
        let closed = contain_now(|| self.ledger.borrow_mut().close_in_place());
        match closed {
            Ok(Ok(())) => {}
            Ok(Err(failed)) => self
                .errors
                .log()
                .error("the ledger did not close", Some(&failed.to_string())),
            Err(panicked) => self.errors.caught("the ledger did not close", &panicked),
        }
        self.errors.log().info("exit 0");
        (self.exit)(0);
    }
}

#[cfg(test)]
mod tests;
