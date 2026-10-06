//! Where the engine's work is polled: outside tokio's cooperative budget.
//!
//! The daemon polls the engine's work from inside tokio tasks: the bridge's
//! reader, an HTTP connection, the executor's driver, the timer of the pass
//! loop. Each of those tasks is polled under a budget (128 operations in the
//! pinned tokio) that a tokio resource spends whenever it hands out something
//! that was ready: a channel's answer, a timer that has elapsed, a socket's
//! bytes. Once it is spent, the next such resource answers `Pending` though
//! its answer is there, and tokio wakes the task only after the tasks queued
//! ahead of it have run.
//!
//! The engine's work polled under a task's budget would borrow it. A burst of
//! ready answers, or a drain entered by a task with little left, would stop in
//! the middle of a chain with the answer unread and the executor's queue
//! empty, so the executor would look idle; the continuation would be woken
//! behind whatever tokio runs next, an exit read before an answer that was
//! already there. Node's microtasks ran to their end whatever came before
//! them. So every poll of the engine's work is made here, with the budget set
//! aside for the poll and given back as it was when the poll returns: the
//! work spends none of the enclosing task's, and is stopped by none.

use std::future::Future;
use std::pin::{pin, Pin};
use std::task::{Context, Poll};

use tokio::task::coop::unconstrained;

/// One poll of `work`, outside the cooperative budget of the tokio task it is
/// made in, if any.
pub(super) fn poll_unconstrained<F: Future + ?Sized>(
    work: Pin<&mut F>,
    context: &mut Context<'_>,
) -> Poll<F::Output> {
    pin!(unconstrained(work)).poll(context)
}
