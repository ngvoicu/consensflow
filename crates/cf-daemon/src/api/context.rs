//! What a handler of the agents' API is given. **Frozen**: one type, the
//! options Node's API was started with and the daemon's log and trace beside
//! them, and the engine's one answer (`finish`), a move of a task that Node's
//! API had no twin of, which only the engine can make: it reads the window.
//!
//! Everything in it is shared, and the daemon runs on one thread: a handler
//! borrows the ledger for one call at a time and never across a wait, copying
//! out what it still needs of what it read (`await_holding_refcell_ref` is
//! denied). A view read before a body is awaited is the view the handler goes
//! on with, as Node's was.

use std::cell::RefCell;
use std::rc::Rc;

use cf_base::refusal::Refusal;
use cf_engine::Finished;
use cf_harness::contract::Work;
use cf_ledger::Ledger;
use tokio::sync::watch;

use super::credentials::Credentials;
use crate::files::{Log, Trace};

/// Whether the daemon is stopping: a door still waiting for an answer is
/// answered at once, and no new wait begins.
#[derive(Clone)]
pub struct Closing {
    state: Rc<watch::Sender<bool>>,
}

impl Closing {
    /// A daemon that is not stopping.
    pub fn new() -> Self {
        Self {
            state: Rc::new(watch::Sender::new(false)),
        }
    }

    /// The daemon is stopping, which wakes whoever waits on it.
    pub fn set(&self) {
        self.state.send_replace(true);
    }

    /// Whether the daemon is stopping now.
    pub fn is_set(&self) -> bool {
        *self.state.borrow()
    }

    /// Ends once the daemon is stopping: at once if it is.
    pub async fn wait(&self) {
        let mut state = self.state.subscribe();
        // The sender is held by this: it is never dropped while it waits.
        let _ = state.wait_for(|stopping| *stopping).await;
    }
}

impl Default for Closing {
    fn default() -> Self {
        Self::new()
    }
}

/// What the API asks of the engine for the one move of a task that is its own to make:
/// `finish(project, number, by)`, which `by` (the chief's handle) asks so that what the
/// worker of a working task said last is its result, ending nothing. The engine reads the
/// window and answers with the task done and the jobs and agents its harness still
/// counted ([`Finished`]), or with why it is refused.
pub type Finishing = dyn Fn(i64, i64, String) -> Work<'static, Result<Finished, Refusal>>;

/// What the API asks of the engine of a participant's window: the task it holds in the
/// background, and the jobs and agents its turn left running (`held(participant)`), while
/// it holds one; none for any other, and before the engine exists.
pub type Holding = dyn Fn(i64) -> Option<(i64, usize, usize)>;

/// What every handler of the agents' API is given.
pub struct Context {
    /// The board's record: `borrow()` to read, `borrow_mut()` to write, for
    /// the one call.
    pub ledger: Rc<RefCell<Ledger>>,
    /// The tokens of the windows that are open, which a caller is told apart
    /// by ([`Credentials::resolve`]).
    pub credentials: Rc<Credentials>,
    /// Wakes the dispatcher (`changed`): every write calls it once it is
    /// done. It only schedules a pass; none runs inside it.
    pub kick: Rc<dyn Fn()>,
    /// What the engine makes of a task the chief asks to finish ([`Finishing`]).
    pub finish: Rc<Finishing>,
    /// What the engine holds a window's task in the background for ([`Holding`]).
    pub held: Rc<Holding>,
    pub closing: Closing,
    pub log: Rc<Log>,
    pub trace: Rc<Trace>,
    /// Whether each request a window's token answered is written to the trace for the
    /// tooling ([`super::calls`]): only a test's daemon, started with
    /// [`super::calls::TOOLING_API_CALLS`], has it.
    pub tooling_calls: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn closing_wakes_what_waits_on_it_at_once_and_stays_set() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let closing = Closing::new();
                assert!(!closing.is_set());
                let waiting = closing.clone();
                let woken = tokio::task::spawn_local(async move { waiting.wait().await });
                tokio::time::sleep(Duration::from_secs(1)).await;
                assert!(!woken.is_finished(), "it waits while nothing stops");
                closing.set();
                woken.await.unwrap();
                assert!(closing.is_set());
                // Asked after it was set, it does not wait.
                closing.wait().await;
                closing.set();
            })
            .await;
    }
}
