//! The background agents a window has running. Claude Code runs them in its own
//! process (no program of the window's tree, so the pane host sees nothing of
//! them), says how many a turn left on the `turn_duration` that ends it, and
//! reports its status `busy` for as long as any runs, though its prompt takes
//! input; the end of one wakes a turn of its own (the report of 2026-10-11: a
//! chief's four research agents, and a result that waited twelve minutes for the
//! last). The stand-in does the same with a thread for each group of agents: no
//! program is started, and the count is what the records and the status say.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// The agents one window has started.
#[derive(Clone, Default)]
pub struct Agents {
    running: Arc<AtomicU64>,
    started: Arc<AtomicBool>,
}

impl Agents {
    /// Starts `count` agents, which end together after `seconds`; `then` runs
    /// once they have ended, as the turn they wake begins.
    pub fn start(&self, count: u64, seconds: u64, then: impl FnOnce() + Send + 'static) {
        self.start_until(
            count,
            move || thread::sleep(Duration::from_secs(seconds)),
            then,
        );
    }

    /// Starts `count` agents, which end when `until` returns: a case that must look at the
    /// window while they run ends them by what it does (`CLAUDE-HELD`: a file it writes), not
    /// by a clock that a loaded machine can outrun. `then` runs once they have ended.
    pub fn start_until(
        &self,
        count: u64,
        until: impl FnOnce() + Send + 'static,
        then: impl FnOnce() + Send + 'static,
    ) {
        self.started.store(true, Ordering::SeqCst);
        self.running.fetch_add(count, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        thread::spawn(move || {
            until();
            running.fetch_sub(count, Ordering::SeqCst);
            then();
        });
    }

    /// How many agents run now.
    pub fn running(&self) -> u64 {
        self.running.load(Ordering::SeqCst)
    }

    /// Whether the window ever started any: a turn of such a window writes a
    /// `turn_duration`, which says how many it left running (none, when none).
    pub fn used(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::mpsc;

    #[test]
    fn a_group_of_agents_runs_its_seconds_and_wakes_what_waits_for_its_end() {
        let agents = Agents::default();
        assert!(!agents.used());
        assert_eq!(agents.running(), 0);

        let (ended, woken) = mpsc::channel();
        agents.start(3, 0, move || ended.send(()).unwrap());
        woken.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(agents.used(), "it remembers it started some");
        assert_eq!(agents.running(), 0, "and none run now");
    }

    #[test]
    fn agents_started_until_something_happens_run_until_it_does() {
        let agents = Agents::default();
        let (may_end, gate) = mpsc::channel::<()>();
        let (waiting, held) = mpsc::channel::<()>();
        let (ended, woken) = mpsc::channel();
        agents.start_until(
            1,
            move || {
                waiting.send(()).unwrap();
                let _ = gate.recv();
            },
            move || ended.send(()).unwrap(),
        );
        // The agent is held by what it waits for: it said so, and nothing woke for it.
        held.recv_timeout(Duration::from_secs(30))
            .expect("the agent waits for what ends it");
        assert_eq!(agents.running(), 1, "it runs as it is started");
        assert!(woken.try_recv().is_err(), "and nothing wakes for it");
        may_end.send(()).unwrap();
        woken.recv_timeout(Duration::from_secs(30)).unwrap();
        assert_eq!(agents.running(), 0);
    }

    #[test]
    fn the_agents_that_run_are_those_of_the_groups_that_have_not_ended() {
        let agents = Agents::default();
        let (ended, woken) = mpsc::channel();
        agents.start(2, 0, {
            let ended = ended.clone();
            move || ended.send(2).unwrap()
        });
        agents.start(1, 600, move || ended.send(1).unwrap());
        assert_eq!(woken.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
        assert_eq!(agents.running(), 1, "the long one runs on");
    }
}
