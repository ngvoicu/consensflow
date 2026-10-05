//! The pass loop and the throttles (`passLoop` and `throttle`,
//! `src/core/daemon.js:187-266`).
//!
//! [`PassLoop`] runs the engine's pass on a timer and on demand, never two at
//! once: a kick during a pass runs one more after it. A pass that fails or
//! panics goes to the log and the next one runs; one that takes more than five
//! seconds is a line of its own; and every ten minutes a line says the daemon
//! is alive, how big it is and how its passes have been. A kick only
//! schedules (`setImmediate`): none runs a pass inside the call, so what a
//! handler builds after waking the loop is built before any pass looks at it.
//! A pass is the engine's work: it is begun where its timer or its kick came,
//! so that what it does before its first wait is done then, the rest is the
//! executor's, and what the first part woke is run to its end before the loop
//! goes on to its next callback.
//!
//! [`throttle`] sends an event at most once in a while: the first call starts
//! the wait, and what comes during it is the same event, not a postponement.

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;

use cf_engine::runtime::begin;
use cf_process::{megabytes, rss};
use tokio::sync::watch;
use tokio::time::{interval_at, sleep, Instant, MissedTickBehavior};

use crate::console::Console;
use crate::errors::contain;
use crate::seams::DaemonSpawn;

/// How often a pass runs on its own.
pub const PASS: Duration = Duration::from_millis(1000);

/// A pass this long is worth a line in the log.
pub const SLOW_PASS: Duration = Duration::from_millis(5_000);

/// How often the daemon writes down that it is alive, and how big it is.
pub const HEARTBEAT: Duration = Duration::from_secs(10 * 60);

/// How long a stop waits for the pass in progress: the app ends the daemon 2 s
/// after asking it to stop, and its exit hooks must run before that.
pub const STOP_WAIT: Duration = Duration::from_millis(1_000);

/// The pass the loop runs: its failure, in words, if it fails.
pub type Pass = Box<dyn Fn() -> Pin<Box<dyn Future<Output = Result<(), String>>>>>;

/// The loop that runs the pass.
#[derive(Clone)]
pub struct PassLoop {
    state: Rc<State>,
}

struct State {
    work: Pass,
    spawn: Rc<DaemonSpawn>,
    console: Rc<Console>,
    /// Whether a pass is running.
    running: watch::Sender<bool>,
    again: Cell<bool>,
    stopped: Cell<bool>,
    /// The passes since the last line that said the daemon is alive, and
    /// the longest of them.
    passes: Cell<u32>,
    slowest: Cell<Duration>,
    /// Told when the loop stops, to end the timers.
    stop: watch::Sender<bool>,
}

impl PassLoop {
    /// Arms the timers now, on the local set the daemon runs on, and runs
    /// `work` on each tick and kick, on `spawn`'s executor. Its errors get the
    /// lines that say how the passes went and a panic written down; `console`
    /// what a failed pass says on the error output.
    pub fn start(work: Pass, spawn: Rc<DaemonSpawn>, console: Rc<Console>) -> Self {
        let state = Rc::new(State {
            work,
            spawn,
            console,
            running: watch::Sender::new(false),
            again: Cell::new(false),
            stopped: Cell::new(false),
            passes: Cell::new(0),
            slowest: Cell::new(Duration::ZERO),
            stop: watch::Sender::new(false),
        });
        let loop_ = Self { state };
        loop_.arm();
        loop_
    }

    /// Wakes the loop: a pass runs once whatever is running is done, from the
    /// next turn of the loop, never inside this call.
    pub fn kick(&self) {
        schedule(&self.state);
    }

    /// No pass starts after this, the timers end, and what is running is
    /// waited for: at most [`STOP_WAIT`]. A pass still waiting on a window is
    /// left behind, as what it had on its way is settled at the next start.
    pub async fn stop(&self) {
        let state = &self.state;
        state.stopped.set(true);
        state.stop.send_replace(true);
        let mut running = state.running.subscribe();
        let idle = running.wait_for(|running| !*running);
        let _ = tokio::time::timeout(STOP_WAIT, idle).await;
    }

    /// The timer of the passes and the heartbeat, one local task.
    fn arm(&self) {
        let state = Rc::clone(&self.state);
        let mut stopped = state.stop.subscribe();
        drop(tokio::task::spawn_local(async move {
            let now = Instant::now();
            // As `setInterval`: the first tick is a period away, and a tick that
            // came late does not bring the ones it missed after it.
            let mut passes = interval_at(now + PASS, PASS);
            let mut heartbeat = interval_at(now + HEARTBEAT, HEARTBEAT);
            passes.set_missed_tick_behavior(MissedTickBehavior::Delay);
            heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = passes.tick() => run(&state).await,
                    _ = heartbeat.tick() => alive(&state),
                    _ = stopped.wait_for(|stopped| *stopped) => return,
                }
            }
        }));
    }
}

/// `setImmediate(run)`: a pass is asked for from the next turn of the loop.
fn schedule(state: &Rc<State>) {
    let state = Rc::clone(state);
    drop(tokio::task::spawn_local(async move { run(&state).await }));
}

/// Starts a pass if none is running, else asks for one more after it. The
/// pass is begun here, and what its first part woke is run to its end before
/// this returns, as Node's microtasks ran after the timer's callback.
async fn run(state: &Rc<State>) {
    if state.stopped.get() {
        return;
    }
    if *state.running.borrow() {
        state.again.set(true);
        return;
    }
    state.running.send_replace(true);
    let pass = Rc::clone(state);
    let begun = begin(&*state.spawn, async move {
        let started = Instant::now();
        let outcome = contain((pass.work)()).await;
        let errors = pass.spawn.errors();
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(cause)) => {
                pass.console
                    .line(&format!("consensflow dispatcher: {cause}"));
                errors.log().error("a pass failed", Some(&cause));
            }
            Err(panicked) => {
                pass.console
                    .line(&format!("consensflow dispatcher: {}", panicked.message));
                errors.caught("a pass failed", &panicked);
            }
        }
        let took = started.elapsed();
        pass.passes.set(pass.passes.get() + 1);
        pass.slowest.set(pass.slowest.get().max(took));
        if took > SLOW_PASS {
            errors
                .log()
                .warn(&format!("a pass took {} ms", took.as_millis()), None);
        }
        pass.running.send_replace(false);
        if pass.again.get() && !pass.stopped.get() {
            pass.again.set(false);
            schedule(&pass);
        }
    })
    .await;
    drop(begun);
    state.spawn.drain();
}

/// The line every ten minutes: the daemon is alive, how its passes have
/// been, and how big it is. What it counts starts over.
fn alive(state: &State) {
    let size = rss().map_or(0, megabytes);
    state.spawn.errors().log().info(&format!(
        "alive: {} passes, slowest {} ms, rss {size} MB",
        state.passes.get(),
        state.slowest.get().as_millis()
    ));
    state.passes.set(0);
    state.slowest.set(Duration::ZERO);
}

/// `work`, at most once every `wait`: the first call begins the wait, and the
/// calls that come during it are that event again, not another (the wait is
/// not restarted by them). When the wait is over `work` runs, and the next
/// call begins another wait.
pub fn throttle(wait: Duration, work: impl Fn() + 'static) -> Rc<dyn Fn()> {
    let pending = Rc::new(Cell::new(false));
    let work = Rc::new(work);
    Rc::new(move || {
        if pending.replace(true) {
            return;
        }
        let (pending, work) = (Rc::clone(&pending), Rc::clone(&work));
        drop(tokio::task::spawn_local(async move {
            sleep(wait).await;
            pending.set(false);
            work();
        }));
    })
}

#[cfg(test)]
mod tests;
