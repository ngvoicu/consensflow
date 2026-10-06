//! What comes to the engine's work from outside the executor, as callbacks.
//!
//! In Node a timer's expiry, a worker thread's message, a socket's data were
//! each a callback of the event loop, and the microtasks they began ran to
//! their end before the next callback. The executor keeps that order for what
//! is woken while it drains, and the daemon drains where a callback ended
//! ([`DaemonSpawn::drain`]); but a timer that elapses, or an answer that a
//! worker thread sends, wakes the work that waits for it directly, from tokio's
//! timers and from the thread, where no drain is. Two that come at once are
//! both woken before either runs, and the driver then runs both together, a
//! turn of one and a turn of the other, which Node never did; one that comes
//! from a thread while a drain runs joins that drain, in the middle of
//! another's chain.
//!
//! [`DaemonSpawn::arrival`] makes such a wait a callback of its own. The wait
//! is polled by the work that awaits it, as ever, but with a waker whose wake,
//! from any thread, only tells a task of tokio's: that task wakes the work and
//! drains what it woke, to its end, before the next task runs. The timers the
//! engine and its adapters sleep on ([`DaemonTime`]) and the answers of the
//! records' worker ([`DaemonRecords`]) are such waits.
//!
//! Once a wait has answered that it has not ended, its answer is taken only
//! from the poll its relay's wake brings, and from no other: work is polled
//! again for other reasons (the executor takes up what [`begin`] began with a
//! poll of its own, a `join` of waits polls them all at each wake), and an
//! answer that came meanwhile, taken by such a poll, would be run inside
//! whatever drain was running, its relay discarded: a callback that was not its
//! own.
//!
//! [`begin`]: cf_engine::runtime::begin
//!
//! Not every wait is one: the bridge's answers are the reader's, which drains
//! after the frames of its read; a request's body is its connection's; a
//! request to the host that passes its deadline is woken by the bridge's own
//! timer, which is not made here; and what an adapter waits on of the system's
//! own (a request to a harness's server on loopback, a child's output) is
//! woken by tokio's sockets and pipes, as its timers were: the same
//! [`DaemonSpawn::arrival`] would make those callbacks too.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use cf_base::time::{Clock, SystemClock};
use cf_harness::contract::{Records, Work};
use cf_harness::records::{Options, Reading};
use cf_harness::seams::Time;
use cf_proto::agents::Harness;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use super::DaemonSpawn;

impl DaemonSpawn {
    /// `wait`, which ends from outside the executor (a timer, a thread's
    /// answer), as a callback of its own: whatever wakes it, from wherever,
    /// is run as a task of tokio's, which wakes the work that awaits it and
    /// drains what that woke before any other task runs.
    pub fn arrival<'a, T: 'a>(self: &Rc<Self>, wait: Work<'a, T>) -> Work<'a, T> {
        Box::pin(Arrival {
            wait,
            spawn: Rc::clone(self),
            tell: Arc::new(Tell::default()),
            waiting: Rc::default(),
            relayed: Rc::default(),
            relay: None,
        })
    }
}

/// A wait that ends as a callback ([`DaemonSpawn::arrival`]).
struct Arrival<'a, T> {
    wait: Work<'a, T>,
    spawn: Rc<DaemonSpawn>,
    /// What the wait's waker does: tells the relay.
    tell: Arc<Tell>,
    /// The work that awaits this, which the relay wakes.
    waiting: Rc<RefCell<Option<Waker>>>,
    /// Set by the relay as it wakes the work, and cleared when the wait is
    /// polled and has not ended: the wait is polled for its answer only while
    /// it is set, once there is a relay.
    relayed: Rc<Cell<bool>>,
    /// Made when the wait first answers that it has not ended: one that ends
    /// at its first poll needs no task.
    relay: Option<Relay>,
}

impl<T> Future for Arrival<'_, T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<T> {
        let this = &mut *self;
        *this.waiting.borrow_mut() = Some(context.waker().clone());
        if this.relay.is_some() && !this.relayed.get() {
            // Polled for some other reason than the relay's wake: whatever
            // the wait has to say is the relay's to bring. The wait keeps its
            // waker, and wakes it when it has something to say.
            return Poll::Pending;
        }
        let waker = Waker::from(Arc::clone(&this.tell));
        match this.wait.as_mut().poll(&mut Context::from_waker(&waker)) {
            Poll::Ready(answer) => {
                this.relay = None;
                Poll::Ready(answer)
            }
            Poll::Pending => {
                this.relayed.set(false);
                if this.relay.is_none() {
                    this.relay = Some(Relay::start(
                        &this.spawn,
                        &this.tell,
                        &this.waiting,
                        &this.relayed,
                    ));
                }
                Poll::Pending
            }
        }
    }
}

/// What the wait's waker does, from any thread: a wake is kept until the
/// relay takes it, so one that comes before the relay is there is not lost.
#[derive(Default)]
struct Tell(Notify);

impl Wake for Tell {
    fn wake(self: Arc<Self>) {
        self.0.notify_one();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.notify_one();
    }
}

/// The task that turns a wake into a callback, which ends with the wait.
struct Relay(JoinHandle<()>);

impl Relay {
    fn start(
        spawn: &Rc<DaemonSpawn>,
        tell: &Arc<Tell>,
        waiting: &Rc<RefCell<Option<Waker>>>,
        relayed: &Rc<Cell<bool>>,
    ) -> Self {
        let (spawn, tell, waiting, relayed) = (
            Rc::clone(spawn),
            Arc::clone(tell),
            Rc::clone(waiting),
            Rc::clone(relayed),
        );
        Self(tokio::task::spawn_local(async move {
            loop {
                tell.0.notified().await;
                relayed.set(true);
                let work = waiting.borrow().clone();
                if let Some(work) = work {
                    work.wake();
                }
                spawn.drain();
            }
        }))
    }
}

impl Drop for Relay {
    /// A wait that is dropped (a timer that no longer bounds anything) has no
    /// callback to make.
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The system's time, whose waits are callbacks: what elapses is woken by a
/// task of tokio's, and the chain it began is over before the next timer is
/// heard. The engine's and the adapters' sleeps.
pub struct DaemonTime {
    spawn: Rc<DaemonSpawn>,
}

impl DaemonTime {
    /// The time of day, and waits that end as callbacks on `spawn`'s executor.
    pub fn new(spawn: Rc<DaemonSpawn>) -> Self {
        Self { spawn }
    }
}

impl Time for DaemonTime {
    fn wall_ms(&self) -> i64 {
        SystemClock.now_ms()
    }

    fn sleep(&self, duration: Duration) -> Work<'_, ()> {
        self.spawn.arrival(Box::pin(tokio::time::sleep(duration)))
    }
}

/// The records, whose answers come from the worker's thread: each is a
/// callback of its own, and none joins a drain that is running.
pub struct DaemonRecords {
    records: Rc<dyn Records>,
    spawn: Rc<DaemonSpawn>,
}

impl DaemonRecords {
    /// `records`, its answers made callbacks on `spawn`'s executor.
    pub fn new(records: Rc<dyn Records>, spawn: Rc<DaemonSpawn>) -> Self {
        Self { records, spawn }
    }
}

impl Records for DaemonRecords {
    fn look<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
        options: &'a Options,
    ) -> Work<'a, Arc<Reading>> {
        self.spawn
            .arrival(self.records.look(harness, session, options))
    }

    fn has_transcript<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
    ) -> Work<'a, Result<bool, String>> {
        self.spawn
            .arrival(self.records.has_transcript(harness, session))
    }
}
