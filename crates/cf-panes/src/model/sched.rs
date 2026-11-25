//! The scheduler of the model: each actor is a thread, exactly one runs at a time, from one
//! named point to the next, and the scheduler chooses who goes.
//!
//! A point is a call of [`at`] (the host names them where it reads or moves a count, takes a
//! lock or writes: `sync::point`), and every lock of the model is taken through [`lock`], which
//! tries it and, where another actor holds it, tells the scheduler that this one waits. Time does
//! not move here: the model's clock moves when an actor rests ([`super::clock`]).
//!
//! Outside a schedule `at` does nothing and `lock` is `Mutex::lock`.

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, LockResult, Mutex, MutexGuard, TryLockError};
use std::thread::JoinHandle;
use std::time::Duration;

/// Who an actor is, in the order it was spawned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Actor(pub(crate) usize);

/// Where an actor stands while it waits to be chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Stands {
    /// At a point it reached (or `start`): it runs on when chosen.
    At(&'static str),
    /// Waiting for a lock another actor holds, or for a condition: chosen only when no other can run.
    Blocked(&'static str),
    /// Resting: time passes for it when it is chosen, which is when no other actor has anything to
    /// do, and now and then before: work takes no time that a rest does not.
    Resting,
    Done,
    /// It panicked: its message.
    Failed(String),
}

struct Core {
    /// Who may run now; none while the scheduler chooses.
    running: Option<usize>,
    stands: Vec<Stands>,
    /// Every step taken, for a failure to print.
    trace: Vec<String>,
}

struct Shared {
    core: Mutex<Core>,
    changed: Condvar,
}

thread_local! {
    static CURRENT: RefCell<Option<(Arc<Shared>, usize)>> = const { RefCell::new(None) };
    /// Points are not taken while it is set: the oracle reads the host between two steps of an
    /// actor, and its read is no step.
    static QUIET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `read` with no point taken in it.
pub(crate) fn without_points<T>(read: impl FnOnce() -> T) -> T {
    QUIET.with(|quiet| quiet.set(true));
    let read = read();
    QUIET.with(|quiet| quiet.set(false));
    read
}

/// A schedule: its actors and the choice of who runs.
pub(crate) struct Sched {
    shared: Arc<Shared>,
    names: Vec<&'static str>,
    threads: Vec<JoinHandle<()>>,
}

/// How long the scheduler waits for an actor to reach its next point before it says that one
/// waits on something that is not the model's.
const STUCK: Duration = Duration::from_secs(30);

impl Sched {
    pub(crate) fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                core: Mutex::new(Core {
                    running: None,
                    stands: Vec::new(),
                    trace: Vec::new(),
                }),
                changed: Condvar::new(),
            }),
            names: Vec::new(),
            threads: Vec::new(),
        }
    }

    /// A new actor, parked at its start until it is chosen.
    pub(crate) fn spawn(
        &mut self,
        name: &'static str,
        work: impl FnOnce() + Send + 'static,
    ) -> Actor {
        let index = self.names.len();
        self.names.push(name);
        {
            let mut core = self.shared.core.lock().unwrap();
            core.stands.push(Stands::At("start"));
        }
        let shared = Arc::clone(&self.shared);
        let handle = std::thread::Builder::new()
            .name(format!("model-{name}"))
            .spawn(move || {
                CURRENT.with(|slot| *slot.borrow_mut() = Some((Arc::clone(&shared), index)));
                park(&shared, index, Stands::At("start"));
                let outcome = catch_unwind(AssertUnwindSafe(work));
                let mut core = shared.core.lock().unwrap();
                core.stands[index] = match outcome {
                    Ok(()) => Stands::Done,
                    Err(panic) => Stands::Failed(
                        panic
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                            .unwrap_or_else(|| "a panic".to_owned()),
                    ),
                };
                core.running = None;
                shared.changed.notify_all();
            })
            .expect("a thread for the actor");
        self.threads.push(handle);
        Actor(index)
    }

    /// Where `actor` stands now: all actors are parked whenever the scheduler has the word.
    pub(crate) fn stands(&self, actor: Actor) -> Stands {
        self.shared.core.lock().unwrap().stands[actor.0].clone()
    }

    pub(crate) fn name(&self, actor: Actor) -> &'static str {
        self.names[actor.0]
    }

    pub(crate) fn actors(&self) -> Vec<Actor> {
        (0..self.names.len()).map(Actor).collect()
    }

    /// Whether `actor` can be chosen: it is at a point, or waits (and may find what it waits for).
    pub(crate) fn alive(&self, actor: Actor) -> bool {
        matches!(
            self.stands(actor),
            Stands::At(_) | Stands::Blocked(_) | Stands::Resting
        )
    }

    /// Lets `actor` run to its next point, or its end. The steps are written down.
    pub(crate) fn step(&self, actor: Actor) -> Stands {
        let mut core = self.shared.core.lock().unwrap();
        assert!(
            matches!(
                core.stands[actor.0],
                Stands::At(_) | Stands::Blocked(_) | Stands::Resting
            ),
            "{} is not parked: {:?}",
            self.names[actor.0],
            core.stands[actor.0]
        );
        let from = core.stands[actor.0].clone();
        core.running = Some(actor.0);
        self.shared.changed.notify_all();
        let (mut core, timeout) = self
            .shared
            .changed
            .wait_timeout_while(core, STUCK, |core| core.running.is_some())
            .unwrap();
        assert!(
            !timeout.timed_out(),
            "{} waits on something that is not the model's, after {from:?}\n{}",
            self.names[actor.0],
            core.trace.join("\n")
        );
        let to = core.stands[actor.0].clone();
        core.trace
            .push(format!("{}: {:?} -> {:?}", self.names[actor.0], from, to));
        to
    }

    /// Runs `actor` until it stands at the point `name` (and not past it), or ends or waits on a
    /// lock or condition (none to find: it is not stepped past that). Gives where it stands.
    pub(crate) fn run_to(&self, actor: Actor, name: &'static str) -> Stands {
        for _ in 0..100_000 {
            match self.stands(actor) {
                Stands::At(at) if at == name => return Stands::At(at),
                Stands::At(_) | Stands::Resting => {
                    self.step(actor);
                }
                Stands::Blocked(_) => {
                    // It may find what it waits for now: one turn, and if it still waits, it waits.
                    if let waiting @ Stands::Blocked(_) = self.step(actor) {
                        return waiting;
                    }
                }
                other => return other,
            }
        }
        panic!("{} never reached {name}", self.names[actor.0]);
    }

    /// The last of the steps taken so far, one a line: a schedule that does not end has taken
    /// hundreds of thousands, and a failure's words are not a log.
    pub(crate) fn trace(&self) -> String {
        const LAST: usize = 120;
        let core = self.shared.core.lock().unwrap();
        let from = core.trace.len().saturating_sub(LAST);
        let mut shown = core.trace[from..].join("\n");
        if from > 0 {
            shown = format!("({from} steps before)\n{shown}");
        }
        shown
    }

    /// Lets every actor go to its end, a thread's join.
    pub(crate) fn join(self) {
        for handle in self.threads {
            let _ = handle.join();
        }
    }
}

/// Parks the calling actor at `stands` and waits until it is chosen.
fn park(shared: &Arc<Shared>, index: usize, stands: Stands) {
    let mut core = shared.core.lock().unwrap();
    core.stands[index] = stands;
    core.running = None;
    shared.changed.notify_all();
    let _core = shared
        .changed
        .wait_while(core, |core| core.running != Some(index))
        .unwrap();
}

/// A point: where the schedule may choose another actor to run. Does nothing outside a schedule.
pub(crate) fn at(name: &'static str) {
    if QUIET.with(std::cell::Cell::get) {
        return;
    }
    CURRENT.with(|slot| {
        if let Some((shared, index)) = slot.borrow().as_ref() {
            park(shared, *index, Stands::At(name));
        }
    });
}

/// Whether the calling thread is an actor of a schedule.
pub(crate) fn scheduled() -> bool {
    CURRENT.with(|slot| slot.borrow().is_some())
}

/// A lock, taken through the scheduler: where another actor holds it, this one waits (and the
/// scheduler chooses another) until it is free.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> LockResult<MutexGuard<'_, T>> {
    if !scheduled() {
        return mutex.lock();
    }
    at("lock");
    loop {
        match mutex.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(poisoned)) => return Err(poisoned),
            Err(TryLockError::WouldBlock) => waiting("a lock held by another actor"),
        }
    }
}

/// Waits, as an actor, until `done` says so: the scheduler chooses others meanwhile.
pub(crate) fn wait_until(why: &'static str, mut done: impl FnMut() -> bool) {
    while !done() {
        waiting(why);
    }
}

/// The calling actor rests: time passes for it when it is chosen.
pub(crate) fn resting() {
    CURRENT.with(|slot| {
        if let Some((shared, index)) = slot.borrow().as_ref() {
            park(shared, *index, Stands::Resting);
        }
    });
}

/// The calling actor waits one turn: chosen again only when no other can run, or at random.
fn waiting(why: &'static str) {
    CURRENT.with(|slot| {
        if let Some((shared, index)) = slot.borrow().as_ref() {
            park(shared, *index, Stands::Blocked(why));
        } else {
            // A thread that is no actor of a schedule has nobody to wait for.
            std::thread::yield_now();
        }
    });
}
