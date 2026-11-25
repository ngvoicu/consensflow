//! The stop: one latch for every way the daemon is asked to end, one deadline
//! for the waiting, and a tail that waits for nothing.
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
//! - **The tail** waits for the thread that writes the trace, a quarter of a second at most,
//!   and for the log's, last: what is left of the stop's deadline, a quarter of a second at
//!   least, so that the log holds what it was given, then the count of what it dropped, then
//!   the stop's line and the exit's (T-2263). It lets the home go (the ledger is closed)
//!   only once the trace is written: past that it exits with the ledger still held, so that the writer ends with the
//!   process and no daemon that takes the home meanwhile has lines written behind it.
//!   The children are forced to end without
//!   waiting (each group looked at once more, at once, for what it started as
//!   the signal went round: `Ender::force`), `exit 0` is written, and the process exits. It
//!   never closes the bridge (a close refuses what waits on it, and the engine
//!   would record a launch failed that Node left for the next start to
//!   settle), and it never drops the runtime, whose read of its input cannot
//!   be cancelled.

use std::cell::{OnceCell, RefCell};
use std::future::Future;
use std::rc::Rc;
use std::time::{Duration, Instant};

use cf_bridge::local::Ended;
use cf_ledger::Ledger;
use cf_process::{megabytes, rss};
use tokio::sync::watch;

use crate::api::Api;
use crate::errors::{contain, contain_now, Errors};
use crate::files::SETTLE;
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
    /// is the system processes' `end_all`, an `Ender::force` a child, which looks
    /// once at what its group started and never waits (Calliope's P2 on T-581).
    pub ends_children: Rc<dyn Fn()>,
    pub ledger: Rc<RefCell<Ledger>>,
    /// How the process ends: the daemon's own exits; a test's is told it.
    pub exit: Rc<dyn Fn(i32)>,
}

impl Stopping {
    /// The stop, for `why`: ends with the process's exit.
    pub async fn run(&self, why: &str) {
        let began = Instant::now();
        let size = rss().map_or(0, megabytes);
        self.errors
            .log()
            .keep(&format!("stop: {why}; rss {size} MB"));
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
        self.tail_within(DEADLINE.saturating_sub(began.elapsed()).max(SETTLE));
    }

    /// What is left to do, with no wait in it. Each step is its own: one that
    /// fails or panics leaves the rest, and the process ends.
    pub(crate) fn tail(&self) {
        self.tail_within(SETTLE);
    }

    /// [`Stopping::tail`], waiting `log_within` at most for the log.
    fn tail_within(&self, log_within: Duration) {
        if let Err(panicked) = contain_now(|| (self.ends_children)()) {
            self.errors.caught("the children did not end", &panicked);
        }
        // What the trace was told is written first, if its file lets it within the
        // tail's wait, and only then is the home let go: a daemon that took the home
        // after this one must find no writer of this one's at the trace, or it would write
        // lines of a project the next one deleted. A file that is held loses the last lines,
        // never the exit, and the ledger stays held then: the writer ends with the process.
        if self.errors.trace().end(SETTLE) {
            let closed = contain_now(|| self.ledger.borrow_mut().close_in_place());
            match closed {
                Ok(Ok(())) => {}
                Ok(Err(failed)) => self
                    .errors
                    .log()
                    .error("the ledger did not close", Some(&failed.to_string())),
                Err(panicked) => self.errors.caught("the ledger did not close", &panicked),
            }
        } else {
            self.errors.log().warn(
                "the trace was not written in time; its last lines may be missing",
                None,
            );
        }
        self.errors.log().keep("exit 0");
        // What was said is written before the process goes, if the log's file lets it in time.
        let _ = self.errors.log().written(log_within);
        (self.exit)(0);
    }
}

/// Arms the stop: every way the daemon is asked to end trips `latch`, and
/// what trips it runs `stopping`, once.
pub(crate) fn arm(
    latch: &Rc<Latch>,
    ended: impl Future<Output = Ended> + 'static,
    signals: bool,
    errors: &Rc<Errors>,
    stopping: Rc<Stopping>,
    done: watch::Sender<bool>,
) {
    let watching = Rc::clone(latch);
    errors.spawn("the stop's watch failed", async move {
        // Why the input ended is told by the bridge; a failure is told to the
        // fatal handler too, which has said it first.
        match ended.await {
            Ended::Input => watching.trip("stdin ended"),
            Ended::Closed => watching.trip("stdin closed"),
            Ended::Failed(_) => watching.trip("the bridge failed"),
        }
    });
    if signals {
        listen_for_signals(latch, errors);
    }
    let (waiting, stop_errors) = (Rc::clone(latch), Rc::clone(errors));
    errors.spawn("the stop failed", async move {
        let why = waiting.tripped().await;
        // The tail must be reached whatever goes wrong before it.
        if let Err(panicked) = contain(stopping.run(&why)).await {
            stop_errors.caught("the stop failed", &panicked);
            stopping.tail();
        }
        done.send_replace(true);
    });
}

/// SIGTERM and SIGINT, which stop the daemon; on Windows Ctrl-C, and the end
/// of the input, are all there is.
fn listen_for_signals(latch: &Rc<Latch>, errors: &Rc<Errors>) {
    #[cfg(unix)]
    for (kind, name) in [
        (tokio::signal::unix::SignalKind::terminate(), "SIGTERM"),
        (tokio::signal::unix::SignalKind::interrupt(), "SIGINT"),
    ] {
        if let Ok(mut signal) = tokio::signal::unix::signal(kind) {
            let latch = Rc::clone(latch);
            errors.spawn("a signal's watch failed", async move {
                signal.recv().await;
                latch.trip(name);
            });
        }
    }
    #[cfg(not(unix))]
    {
        let latch = Rc::clone(latch);
        errors.spawn("a signal's watch failed", async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                latch.trip("SIGINT");
            }
        });
    }
}

#[cfg(test)]
mod tests;
