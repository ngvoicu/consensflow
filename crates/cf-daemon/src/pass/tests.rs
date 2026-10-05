//! The pass loop and the throttles on tokio's paused clock: time passes only
//! when everything that can run has, so a ten-minute heartbeat costs nothing
//! and a wait that never ends fails at once. Ported from
//! `core-daemon.test.mjs:33-155`, and held to what `daemon.js:194-266` does.

use std::cell::{Cell, RefCell};
use std::future::pending;

use tokio::task::LocalSet;
use tokio::time::sleep;

use super::*;
use crate::testing::{worked, Said, Worked};

mod engine;
mod throttle;

/// A loop's surroundings: its home (the log and the trace), the engine's work
/// as the daemon runs it, what it said on the error output.
struct Rig {
    home: tempfile::TempDir,
    spawn: Rc<DaemonSpawn>,
    said: Said,
    console: Rc<Console>,
}

/// Called inside the local set the scene runs in: the executor's driver is
/// spawned on it.
fn rig() -> Rig {
    let Worked { home, spawn } = worked();
    let said = Said::default();
    let console = Rc::new(Console::to(said.clone(), || {}));
    Rig {
        home,
        spawn,
        said,
        console,
    }
}

impl Rig {
    fn start(
        &self,
        work: impl Fn() -> Pin<Box<dyn Future<Output = Result<(), String>>>> + 'static,
    ) -> PassLoop {
        PassLoop::start(
            Box::new(work),
            Rc::clone(&self.spawn),
            Rc::clone(&self.console),
        )
    }

    /// The log's lines as they were said, with no time before each of its
    /// own (what is known of a failure goes underneath, indented, as it is).
    fn lines(&self) -> Vec<String> {
        std::fs::read_to_string(self.home.path().join("daemon.log"))
            .unwrap_or_default()
            .lines()
            .map(|line| {
                if line.starts_with(' ') {
                    line.to_owned()
                } else {
                    line.split_once(' ')
                        .map_or(line, |(_, rest)| rest)
                        .to_owned()
                }
            })
            .collect()
    }
}

/// Runs a scenario on the paused clock, on the local set the daemon uses.
async fn scene<F: Future>(scenario: F) -> F::Output {
    LocalSet::new().run_until(scenario).await
}

/// Lets what is ready run, without the clock moving.
async fn turns() {
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn a_stop_waits_only_a_moment_for_a_pass_held_up_by_a_slow_window() {
    scene(async {
        let rig = rig();
        let loop_ = rig.start(|| Box::pin(pending()));
        loop_.kick();
        sleep(Duration::from_millis(20)).await;
        let stopping = Instant::now();
        loop_.stop().await;
        // A second: the app ends the daemon two after it asks.
        assert_eq!(stopping.elapsed(), Duration::from_secs(1), "and no longer");
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_stop_with_nothing_running_does_not_wait() {
    scene(async {
        let rig = rig();
        let loop_ = rig.start(|| Box::pin(async { Ok(()) }));
        let stopping = Instant::now();
        loop_.stop().await;
        assert_eq!(stopping.elapsed(), Duration::ZERO);
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_stop_waits_for_the_pass_running_and_no_more_than_that() {
    scene(async {
        let rig = rig();
        let loop_ = rig.start(|| {
            Box::pin(async {
                sleep(Duration::from_millis(300)).await;
                Ok(())
            })
        });
        loop_.kick();
        turns().await;
        let stopping = Instant::now();
        loop_.stop().await;
        assert_eq!(stopping.elapsed(), Duration::from_millis(300));
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn one_pass_at_a_time_and_kicks_during_a_pass_run_one_more_after_it() {
    scene(async {
        let rig = rig();
        let (passes, running, most) = (
            Rc::new(Cell::new(0)),
            Rc::new(Cell::new(0)),
            Rc::new(Cell::new(0)),
        );
        let gate = Rc::new(tokio::sync::Notify::new());
        let (count, now, peak, held) = (
            Rc::clone(&passes),
            Rc::clone(&running),
            Rc::clone(&most),
            Rc::clone(&gate),
        );
        let loop_ = rig.start(move || {
            let (count, now, peak, held) = (
                Rc::clone(&count),
                Rc::clone(&now),
                Rc::clone(&peak),
                Rc::clone(&held),
            );
            Box::pin(async move {
                count.set(count.get() + 1);
                now.set(now.get() + 1);
                peak.set(peak.get().max(now.get()));
                if count.get() == 1 {
                    held.notified().await;
                }
                now.set(now.get() - 1);
                Ok(())
            })
        });
        loop_.kick();
        sleep(Duration::from_millis(20)).await;
        loop_.kick();
        loop_.kick();
        sleep(Duration::from_millis(20)).await;
        assert_eq!(passes.get(), 1, "the kicks wait for the pass in progress");
        gate.notify_one();
        sleep(Duration::from_millis(20)).await;
        loop_.stop().await;
        assert_eq!(
            (passes.get(), most.get()),
            (2, 1),
            "two kicks made one more"
        );
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_kick_only_schedules_so_no_pass_runs_inside_it() {
    scene(async {
        let rig = rig();
        let passes = Rc::new(Cell::new(0));
        let count = Rc::clone(&passes);
        let loop_ = rig.start(move || {
            let count = Rc::clone(&count);
            Box::pin(async move {
                count.set(count.get() + 1);
                Ok(())
            })
        });
        loop_.kick();
        assert_eq!(passes.get(), 0, "not inside the call");
        turns().await;
        assert_eq!(passes.get(), 1, "from the next turn");
        loop_.stop().await;
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_kick_runs_its_pass_from_the_turn_after_the_work_begun_just_after_it() {
    scene(async {
        let rig = rig();
        let order: Rc<RefCell<Vec<&'static str>>> = Rc::default();
        let said = Rc::clone(&order);
        let loop_ = rig.start(move || {
            let said = Rc::clone(&said);
            Box::pin(async move {
                said.borrow_mut().push("pass");
                Ok(())
            })
        });
        loop_.kick();
        // What a handler begins after it woke the loop, run where its callback
        // ends, is done before any pass looks (`setImmediate`: after what the
        // turn already holds).
        let said = Rc::clone(&order);
        rig.spawn.apart("a handler's work failed", async move {
            said.borrow_mut().push("handler's work");
        });
        rig.spawn.drain();
        turns().await;
        assert_eq!(*order.borrow(), ["handler's work", "pass"]);
        loop_.stop().await;
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_pass_runs_a_second_after_the_start_and_then_every_second() {
    scene(async {
        let rig = rig();
        let at: Rc<RefCell<Vec<Duration>>> = Rc::default();
        let (started, noted) = (Instant::now(), Rc::clone(&at));
        let loop_ = rig.start(move || {
            let (noted, started) = (Rc::clone(&noted), started);
            Box::pin(async move {
                noted.borrow_mut().push(started.elapsed());
                Ok(())
            })
        });
        sleep(Duration::from_millis(3500)).await;
        loop_.stop().await;
        assert_eq!(
            *at.borrow(),
            [
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(3)
            ]
        );
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_tick_that_comes_while_a_pass_runs_asks_for_one_more_not_for_each_it_missed() {
    scene(async {
        let rig = rig();
        let passes = Rc::new(Cell::new(0));
        let count = Rc::clone(&passes);
        // Each pass takes 3.5 s: five ticks come during the first.
        let loop_ = rig.start(move || {
            let count = Rc::clone(&count);
            Box::pin(async move {
                count.set(count.get() + 1);
                sleep(Duration::from_millis(3500)).await;
                Ok(())
            })
        });
        sleep(Duration::from_millis(1000 + 3500 + 100)).await;
        assert_eq!(passes.get(), 2, "the second began as the first ended");
        loop_.stop().await;
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn after_a_stop_nothing_starts_not_a_kick_and_not_the_timer() {
    scene(async {
        let rig = rig();
        let passes = Rc::new(Cell::new(0));
        let count = Rc::clone(&passes);
        let loop_ = rig.start(move || {
            let count = Rc::clone(&count);
            Box::pin(async move {
                count.set(count.get() + 1);
                Ok(())
            })
        });
        loop_.stop().await;
        loop_.kick();
        sleep(Duration::from_secs(30)).await;
        assert_eq!(passes.get(), 0);
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_pass_longer_than_five_seconds_is_written_down_and_one_of_five_is_not() {
    scene(async {
        let rig = rig();
        // The first passes take 5 and 6 seconds, and the rest (the timer's own) none.
        let durations = Rc::new(RefCell::new(std::collections::VecDeque::from([
            5_000_u64, 6_000,
        ])));
        let given = Rc::clone(&durations);
        let loop_ = rig.start(move || {
            let given = Rc::clone(&given);
            Box::pin(async move {
                let next = given.borrow_mut().pop_front().unwrap_or(0);
                sleep(Duration::from_millis(next)).await;
                Ok(())
            })
        });
        loop_.kick();
        sleep(Duration::from_secs(20)).await;
        assert_eq!(rig.lines(), ["warn a pass took 6000 ms"]);
        loop_.stop().await;
    })
    .await;
}

/// How often the daemon says it is alive, as a number: not the constant it is held to.
const TEN_MINUTES: Duration = Duration::from_secs(600);

/// The count and the slowest of a line that says the daemon is alive, and its size.
fn alive_of(line: &str) -> (u32, u64, u64) {
    let rest = line.strip_prefix("info alive: ").unwrap();
    let (passes, rest) = rest.split_once(" passes, slowest ").unwrap();
    let (slowest, rest) = rest.split_once(" ms, rss ").unwrap();
    let size = rest.strip_suffix(" MB").unwrap();
    (
        passes.parse().unwrap(),
        slowest.parse().unwrap(),
        size.parse().unwrap(),
    )
}

#[tokio::test(start_paused = true)]
async fn every_ten_minutes_a_line_says_it_is_alive_how_its_passes_went_and_how_big_it_is() {
    scene(async {
        let rig = rig();
        let done = Rc::new(Cell::new(0_u32));
        let finished = Rc::clone(&done);
        // Each pass takes 3 s, so each ends between ticks, never on one.
        let loop_ = rig.start(move || {
            let finished = Rc::clone(&finished);
            Box::pin(async move {
                sleep(Duration::from_secs(3)).await;
                finished.set(finished.get() + 1);
                Ok(())
            })
        });
        sleep(TEN_MINUTES + Duration::from_millis(1)).await;
        let first = done.get();
        let lines = rig.lines();
        assert_eq!(lines.len(), 1, "{lines:?}");
        let (passes, slowest, size) = alive_of(&lines[0]);
        assert_eq!((passes, slowest), (first, 3_000));
        assert!(size > 0, "a running process has a size");

        // The next line counts from the one before: what began after it.
        sleep(TEN_MINUTES).await;
        let lines = rig.lines();
        assert_eq!(lines.len(), 2, "{lines:?}");
        let (passes, slowest, _) = alive_of(&lines[1]);
        assert_eq!((passes, slowest), (done.get() - first, 3_000));
        loop_.stop().await;
        assert_eq!(rig.lines().len(), 2, "a stopped loop says nothing more");
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_failed_pass_is_written_down_and_said_and_the_next_one_runs() {
    scene(async {
        let rig = rig();
        let passes = Rc::new(Cell::new(0));
        let count = Rc::clone(&passes);
        let loop_ = rig.start(move || {
            let count = Rc::clone(&count);
            Box::pin(async move {
                count.set(count.get() + 1);
                if count.get() == 1 {
                    Err("boom".to_owned())
                } else {
                    Ok(())
                }
            })
        });
        loop_.kick();
        sleep(Duration::from_millis(30)).await;
        loop_.kick();
        sleep(Duration::from_millis(30)).await;
        loop_.stop().await;
        assert_eq!(passes.get(), 2);
        assert_eq!(rig.lines(), ["error a pass failed", "    boom"]);
        assert_eq!(rig.said.text(), "consensflow dispatcher: boom\n");
        assert!(
            !rig.home.path().join("events.jsonl").exists(),
            "a failure that was returned is no uncaught error"
        );
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn a_pass_that_panics_is_written_down_traced_and_said_and_the_next_one_runs() {
    scene(async {
        let rig = rig();
        let passes = Rc::new(Cell::new(0));
        let count = Rc::clone(&passes);
        let loop_ = rig.start(move || {
            let count = Rc::clone(&count);
            Box::pin(async move {
                count.set(count.get() + 1);
                if count.get() == 1 {
                    sleep(Duration::from_millis(5)).await;
                    panic!("a bug in a step");
                }
                Ok(())
            })
        });
        loop_.kick();
        sleep(Duration::from_millis(30)).await;
        loop_.kick();
        sleep(Duration::from_millis(30)).await;
        loop_.stop().await;
        assert_eq!(passes.get(), 2, "the loop went on");
        assert_eq!(
            rig.lines()[..2],
            ["error a pass failed", "    panic: a bug in a step"]
        );
        assert_eq!(rig.said.text(), "consensflow dispatcher: a bug in a step\n");
        let trace = std::fs::read_to_string(rig.home.path().join("events.jsonl")).unwrap();
        assert!(
            trace.contains(
                r#""kind":"daemon.error","project":null,"reason":"a pass failed: a bug in a step""#
            ),
            "{trace}"
        );
    })
    .await;
}
