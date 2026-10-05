//! What a test does with the engine's executor ([`crate::runtime::Executor`],
//! the daemon's own, made strict so that a panic in any work fails the test
//! where it runs): starting work and reading its answer, and a gate it opens
//! to let a wait end. Only work that is woken runs again, as on the daemon's
//! executor, so a fake that never wakes what waits on it holds that work for
//! good, as it would there.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use crate::runtime::{Executor, Spawn};

impl Executor {
    /// Starts `work` as work of its own, run with everything else by the next
    /// [`Executor::drain`]: where its answer will be.
    pub fn start<T: 'static>(&self, work: impl Future<Output = T> + 'static) -> Answer<T> {
        let answer = Answer::default();
        let slot = answer.clone();
        self.spawn(Box::pin(async move {
            *slot.0.borrow_mut() = Some(work.await);
        }));
        answer
    }

    /// Starts `work` and polls it once where it is called, as JavaScript ran
    /// a call to its first wait; the rest is run, with everything else, by
    /// the next [`Executor::drain`]: where its answer will be.
    pub fn start_now<T: 'static>(&self, work: impl Future<Output = T> + 'static) -> Answer<T> {
        let answer = Answer::default();
        let slot = answer.clone();
        let task = self.add(Box::pin(async move {
            *slot.0.borrow_mut() = Some(work.await);
        }));
        self.poll(task);
        answer
    }

    /// Runs `work` as work of its own, with everything else, until nothing
    /// can move: its answer, if it has one by then.
    pub fn finish<T: 'static>(&self, work: impl Future<Output = T> + 'static) -> Option<T> {
        let answer = self.start(work);
        self.drain();
        answer.take()
    }
}

/// The answer of work started ([`Executor::start`]), once it has one.
pub struct Answer<T>(Rc<RefCell<Option<T>>>);

impl<T> Default for Answer<T> {
    fn default() -> Self {
        Self(Rc::new(RefCell::new(None)))
    }
}

impl<T> Clone for Answer<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T> Answer<T> {
    /// Whether the work has ended.
    pub fn ended(&self) -> bool {
        self.0.borrow().is_some()
    }

    /// The answer, if the work has ended; it is given once.
    pub fn take(&self) -> Option<T> {
        self.0.borrow_mut().take()
    }
}

/// A wait that ends once a test opens it, waking what waits on it.
#[derive(Clone, Default)]
pub struct Gate(Rc<GateState>);

#[derive(Default)]
struct GateState {
    open: Cell<bool>,
    wakers: RefCell<Vec<Waker>>,
}

impl Gate {
    /// Opens the gate: every wait on it ends.
    pub fn open(&self) {
        self.0.open.set(true);
        let wakers = std::mem::take(&mut *self.0.wakers.borrow_mut());
        for waker in wakers {
            waker.wake();
        }
    }

    /// Whether the gate was opened.
    pub fn is_open(&self) -> bool {
        self.0.open.get()
    }

    /// A wait on the gate.
    pub fn wait(&self) -> GateWait {
        GateWait(Rc::clone(&self.0))
    }
}

/// A wait on a [`Gate`].
pub struct GateWait(Rc<GateState>);

impl Future for GateWait {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0.open.get() {
            Poll::Ready(())
        } else {
            self.0.wakers.borrow_mut().push(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::next_turn;

    /// What a test's pieces of work did, in order.
    type Log = Rc<RefCell<Vec<&'static str>>>;

    #[test]
    fn work_started_now_does_what_comes_before_its_first_wait_where_it_is_called() {
        let executor = Executor::strict();
        let log = Log::default();
        let (started, later) = (Rc::clone(&log), Rc::clone(&log));
        let answer = executor.start_now(async move {
            started.borrow_mut().push("start");
            next_turn().await;
            started.borrow_mut().push("end");
            7
        });
        // Before anything is run, as JavaScript ran a call to its first wait.
        later.borrow_mut().push("caller");
        assert_eq!(*log.borrow(), ["start", "caller"]);
        assert!(!answer.ended());
        executor.drain();
        assert_eq!(*log.borrow(), ["start", "caller", "end"]);
        assert_eq!(answer.take(), Some(7));
    }

    #[test]
    fn work_started_waits_for_the_next_drain_to_begin() {
        let executor = Executor::strict();
        let log = Log::default();
        let started = Rc::clone(&log);
        let answer = executor.start(async move { started.borrow_mut().push("start") });
        assert!(log.borrow().is_empty());
        executor.drain();
        assert_eq!(*log.borrow(), ["start"]);
        assert!(answer.ended());
    }
}
