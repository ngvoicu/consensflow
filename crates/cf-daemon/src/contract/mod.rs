//! The executor's contract as the daemon keeps it. The engine's work runs on
//! one executor that runs what is woken, in the order it was, until nothing
//! is, and the daemon says where Node's event loop went on to its next
//! callback by draining it there (`cf_engine::runtime::Executor`). These tests
//! set the daemon's real pieces against that, each in the arrangement where
//! only the daemon's checkpoint, and not luck in tokio's scheduling, keeps
//! Node's order:
//!
//! - [`bridge`]: the real reader of the pane host's frames, an answer and an
//!   exit in one read and in two reads that are ready at once, reads of
//!   answers only, frames in pieces;
//! - [`requests`]: real hyper requests, a body at once and in pieces, two
//!   requests on one connection, a client that leaves after the work began;
//! - [`callbacks`]: timers and the records' answers, which come from outside
//!   the executor, two at once, a timer with an exit or a request, an answer
//!   in the middle of a drain, a chain of continuations whole between them;
//!   [`looks`] hangs the engine's own chains on them;
//! - [`liveness`]: the driver, for what comes from another thread with
//!   nothing after it;
//! - [`kicks`]: the pass loop, a kick that is `setImmediate` and never a pass
//!   inside a drain.
//!
//! What the engine does is read at its seams: a task's state in the ledger,
//! the events it logged in order, the calls its adapter was given (a delivery
//! is one). What Node did is what the engine kit's run of the same
//! arrangement does ([`reference`]), which the 183 dispatcher tests hold to
//! Node's traces: an arrangement of callbacks stands for the kit's calls and
//! drains, so a result is held to the kit's, never to a number written here.
//! There is no Node recording of this surface: these are the checks of
//! Astraeus's second look, not a trace player.
//!
//! What is real is the executor and its driver, the bridge's reader, hyper on
//! sockets, the timers, the pass loop, the dispatcher on a ledger, and the
//! daemon's own trace, credentials and role texts ([`rig`]). What is not is
//! the harness: the engine kit's fake adapter stands in for it, and the
//! records' worker is a stand-in that answers from a thread of its own
//! ([`standing`]); the pane host is the test, as bytes ([`host`]).

mod bridge;
mod callbacks;
mod client;
mod host;
mod kicks;
mod liveness;
mod looking;
mod looks;
mod reference;
mod requests;
mod rig;
mod standing;

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use cf_engine::runtime::next_turn;
use tokio::task::LocalSet;

/// Lets what is ready run, without the clock moving.
async fn settle() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

/// The turns of a chain of the engine's work: more than the few its own take.
const TURNS: usize = 5;

/// What the chains said as they ran, in order.
type Order = Rc<RefCell<Vec<String>>>;

/// A chain of `turns` turns of the engine's work, each said as `name n`.
async fn chain(order: Order, name: &'static str, turns: usize) {
    for turn in 0..turns {
        order.borrow_mut().push(format!("{name} {turn}"));
        next_turn().await;
    }
}

/// Each chain of `chains` ran whole, the one and then the other, in whichever
/// order the callbacks came: never a turn of one among the turns of the other.
fn assert_whole(order: &Order, chains: [(&str, usize); 2]) {
    let said = |(name, turns): (&str, usize)| -> Vec<String> {
        (0..turns).map(|turn| format!("{name} {turn}")).collect()
    };
    let (first, second) = (said(chains[0]), said(chains[1]));
    let (forwards, backwards) = (
        [first.clone(), second.clone()].concat(),
        [second, first].concat(),
    );
    let ran = order.borrow().clone();
    assert!(
        ran == forwards || ran == backwards,
        "the chains took turns: {ran:?}"
    );
}

/// Runs a scenario on the local set the daemon runs on, in real time.
async fn scene<F: Future>(scenario: F) -> F::Output {
    LocalSet::new().run_until(scenario).await
}
