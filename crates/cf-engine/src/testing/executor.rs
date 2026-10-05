//! An executor that runs every piece of work spawned, the test's own
//! included, in the order each was woken, until nothing can move; a gate a
//! test opens to let a wait end. Only work that is woken runs again, as on tokio's
//! `LocalSet`, so a fake that never wakes what waits on it holds that work
//! for good, as it would there. A panic in any work fails the test where it
//! runs.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use crate::runtime::{LocalWork, Spawn};

/// Runs the engine's work on the test's thread.
#[derive(Default)]
pub struct Executor {
    tasks: RefCell<Vec<Option<LocalWork>>>,
    woken: Arc<Mutex<VecDeque<usize>>>,
}

/// What wakes a piece of work: its number, queued to run again.
struct Wakeup {
    task: usize,
    woken: Arc<Mutex<VecDeque<usize>>>,
}

impl Wake for Wakeup {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.lock().expect("the queue").push_back(self.task);
    }
}

impl Spawn for Executor {
    fn spawn(&self, work: LocalWork) {
        let task = {
            let mut tasks = self.tasks.borrow_mut();
            tasks.push(Some(work));
            tasks.len() - 1
        };
        self.woken.lock().expect("the queue").push_back(task);
    }
}

impl Executor {
    /// Runs what is woken, in the order it was, until nothing is: whether
    /// anything ran.
    pub fn run(&self) -> bool {
        let mut ran = false;
        loop {
            let next = self.woken.lock().expect("the queue").pop_front();
            let Some(task) = next else {
                return ran;
            };
            // Ended, or woken again before it ran: nothing to run.
            let work = self.tasks.borrow_mut().get_mut(task).and_then(Option::take);
            let Some(mut work) = work else {
                continue;
            };
            ran = true;
            let waker = Waker::from(Arc::new(Wakeup {
                task,
                woken: Arc::clone(&self.woken),
            }));
            if work
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
            {
                self.tasks.borrow_mut()[task] = Some(work);
            }
        }
    }

    /// Runs `work` as work of its own, with everything else, until nothing
    /// can move: its answer, if it has one by then.
    pub fn finish<T: 'static>(&self, work: impl Future<Output = T> + 'static) -> Option<T> {
        let answer = Rc::new(RefCell::new(None));
        let slot = Rc::clone(&answer);
        self.spawn(Box::pin(async move {
            *slot.borrow_mut() = Some(work.await);
        }));
        self.run();
        let taken = answer.borrow_mut().take();
        taken
    }

    /// How many pieces of work wait, not ended.
    pub fn waiting(&self) -> usize {
        self.tasks.borrow().iter().flatten().count()
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
