//! The engine's executor: the one thing that runs its work, in the daemon and
//! in the kit's tests, in the order Node's microtask queue ran it.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::Infallible;
use std::future::{poll_fn, Future};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Wake, Waker};

use super::{LocalWork, Spawn};

/// Runs every piece of work spawned onto it, the engine's and a request's
/// alike, in the order each was woken, until nothing is: what Node's
/// microtask queue did after each callback of its event loop. A chain of
/// turns ([`super::next_turn`]) is woken again by each of its own turns, so
/// it ends inside one drain, before the next frame is read or the next timer
/// is heard; tokio's `LocalSet` runs its tasks a batch at a time, between
/// which another task may run, which Node's microtasks never allowed.
///
/// A piece of work is woken once however often it is woken before it runs,
/// and a wake of work that ended does nothing; work woken while it runs goes
/// behind what was woken before it, as a promise's continuation did.
///
/// # The daemon's part
///
/// - The engine's work is spawned onto the executor ([`Spawn`], which the
///   engine is made with), never onto tokio's `LocalSet` itself. A request
///   the API answers is begun with [`super::begin`]: what it does before its
///   first wait is done where it was called, as JavaScript ran it, and the
///   rest is the executor's.
/// - A frame the bridge's reader hears is handled where it is read, as Node's
///   `emit` did: an exit is told to [`crate::Dispatcher::pane_exited`], which
///   settles what it changes before it returns and gives back the rest as
///   work, which is spawned onto the executor.
/// - [`Executor::drain`] is called at each boundary where Node's event loop
///   went on to the next callback: after the frames of one read of the pane
///   host's bridge, after the first part of an HTTP request, and after a
///   timer fired. Work woken before the call is run by it, to the end.
/// - Spawn [`Executor::driver`] once, as a task on the `LocalSet`. It drains
///   whatever was woken from outside a drain (an answer from the bridge, a
///   timer, a blocking task's result from another thread) that no call of
///   `drain` has run yet. Without it that work waits for the next call of
///   `drain` the daemon happens to make; with it, it is a backstop, and the
///   daemon's own calls are what keep Node's order.
///
/// A panic in a piece of work ends that work and nothing else, as it ended a
/// tokio task, the panic hook having told it; [`Executor::strict`] lets it
/// out of the drain instead, which is what a test wants. What the work held
/// is let go as the panic unwinds (a participant, [`super::Hold`]) and whoever
/// waits for its answer ends with an error ([`super::Begun`]); writing the
/// panic down is the spawner's, which catches it before the executor does.
pub struct Executor {
    /// The work that has not ended, by its number; none of the one polled now.
    tasks: RefCell<HashMap<u64, Task>>,
    next: Cell<u64>,
    woken: Arc<Woken>,
    strict: bool,
}

/// A piece of work, and what wakes it.
struct Task {
    work: LocalWork,
    waker: Waker,
}

/// What is woken, in the order it was, and who wants to know; shared with the
/// wakers, which a thread other than the engine's may call.
#[derive(Default)]
struct Woken {
    queue: Mutex<Queue>,
    /// A drain is running: it picks up what is woken meanwhile itself.
    draining: AtomicBool,
    /// The driver, woken when work is woken and no drain is running.
    driver: Mutex<Option<Waker>>,
}

#[derive(Default)]
struct Queue {
    order: VecDeque<u64>,
    queued: HashSet<u64>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Nothing holds either lock across a call that can fail.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Woken {
    fn wake(&self, task: u64) {
        let queued = {
            let mut queue = locked(&self.queue);
            let queued = queue.queued.insert(task);
            if queued {
                queue.order.push_back(task);
            }
            queued
        };
        // Read after the push: a drain that is ending looks at the queue
        // after it stops draining, so one of the two sees the other.
        if queued && !self.draining.load(Ordering::SeqCst) {
            let driver = locked(&self.driver).clone();
            if let Some(driver) = driver {
                driver.wake();
            }
        }
    }

    /// The next piece of work to run.
    fn next(&self) -> Option<u64> {
        let mut queue = locked(&self.queue);
        let task = queue.order.pop_front()?;
        queue.queued.remove(&task);
        Some(task)
    }

    fn is_idle(&self) -> bool {
        locked(&self.queue).order.is_empty()
    }
}

/// What wakes one piece of work: its number, queued to run again.
struct Wakeup {
    task: u64,
    woken: Arc<Woken>,
}

impl Wake for Wakeup {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.wake(self.task);
    }
}

/// A drain that ends however it ends: what is woken after it is the driver's.
struct Draining<'a>(&'a Woken);

impl Drop for Draining<'_> {
    fn drop(&mut self) {
        self.0.draining.store(false, Ordering::SeqCst);
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

impl Executor {
    /// An executor on which a panic in a piece of work ends that work only.
    pub fn new() -> Self {
        Self {
            tasks: RefCell::new(HashMap::new()),
            next: Cell::new(0),
            woken: Arc::new(Woken::default()),
            strict: false,
        }
    }

    /// An executor on which a panic in a piece of work comes out of
    /// [`Executor::drain`], failing the test that runs it.
    pub fn strict() -> Self {
        Self {
            strict: true,
            ..Self::new()
        }
    }

    /// Runs what is woken, in the order it was, and what that wakes, until
    /// nothing is: whether anything ran. A call from inside work being run
    /// does nothing, the drain that runs it going on to what is woken.
    pub fn drain(&self) -> bool {
        if self.woken.draining.swap(true, Ordering::SeqCst) {
            return false;
        }
        let _draining = Draining(&self.woken);
        let mut ran = false;
        loop {
            while let Some(task) = self.woken.next() {
                // Ended, or woken again before it ran: nothing to run.
                ran |= self.poll(task);
            }
            // A wake from another thread, after the last look at the queue,
            // saw this drain and left the driver asleep: look once more, with
            // the drain let go of, so that wake or this look finds the other.
            self.woken.draining.store(false, Ordering::SeqCst);
            if self.woken.is_idle() {
                return ran;
            }
            self.woken.draining.store(true, Ordering::SeqCst);
        }
    }

    /// The driver of the executor on a `LocalSet`, for a task of its own: it
    /// drains what is woken from outside a drain, and never ends.
    pub fn driver(self: &Rc<Self>) -> impl Future<Output = Infallible> + 'static {
        let executor = Rc::clone(self);
        poll_fn(move |context| {
            // Registered before the drain, so what is woken after it, from
            // anywhere, wakes the driver again.
            *locked(&executor.woken.driver) = Some(context.waker().clone());
            executor.drain();
            Poll::<Infallible>::Pending
        })
    }

    /// How many pieces of work have not ended.
    pub fn waiting(&self) -> usize {
        self.tasks.borrow().len()
    }

    /// Takes `work` in, not woken yet: its number.
    pub(crate) fn add(&self, work: LocalWork) -> u64 {
        let task = self.next.get();
        self.next.set(task + 1);
        let waker = Waker::from(Arc::new(Wakeup {
            task,
            woken: Arc::clone(&self.woken),
        }));
        self.tasks.borrow_mut().insert(task, Task { work, waker });
        task
    }

    /// Polls piece of work `task` once, if it has not ended: whether it ran.
    pub(crate) fn poll(&self, task: u64) -> bool {
        let Some(mut running) = self.tasks.borrow_mut().remove(&task) else {
            return false;
        };
        let Task { work, waker } = &mut running;
        let mut context = Context::from_waker(waker);
        match catch_unwind(AssertUnwindSafe(|| work.as_mut().poll(&mut context))) {
            Ok(Poll::Pending) => {
                self.tasks.borrow_mut().insert(task, running);
            }
            Ok(Poll::Ready(())) => {}
            Err(panic) => {
                drop(running);
                if self.strict {
                    resume_unwind(panic);
                }
            }
        }
        true
    }
}

impl Spawn for Executor {
    fn spawn(&self, work: LocalWork) {
        let task = self.add(work);
        self.woken.wake(task);
    }
}

#[cfg(test)]
mod tests;
