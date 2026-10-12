//! Live: a worker of any harness runs one short shell command, replies, and its result
//! reaches the chief with nothing held back. It is the proof of what the rig's stand-in
//! can only stand in for, and the shape of a field bug (T-7, 2026-10-12: ConsensFlow
//! 3.0.1 on Windows, Devin 3000.10.21 as the worker): the worker finished its task, and
//! the board kept it working with `background work running (10 processes)`. On Windows
//! the pane host has no process groups to look at, so what a turn left running is told
//! by when each process began, and the daemon holds the task's result for it; this is
//! the run that shows, on a machine, whether a real harness's turn that runs a command
//! leaves processes the daemon holds the result for, and which ones. It is opt-in
//! (`cargo xtask live finish`, `npm run live:finish`): it spends the machine's quota on
//! one short task of each harness named, and needs each installed and logged in, so it
//! is ignored unless asked for and is no part of `check` or of CI.
//!
//! The native daemon and the real pane host run in a home of the run's own (never
//! `~/.consensflow`). The chief is a stand-in that only notes what it is sent, and the
//! check plays the chief with the chief's own token (`cf task add`). The worker, one
//! harness after another, is given one task: to run one shell command with its own
//! shell tool, and to reply with a line that holds what it printed (the machine's
//! `hostname`, which [`task`] says why). The run then looks, once a second and for up
//! to five minutes, at
//!
//! - the task's state,
//! - the worker's lane of the board: what its window is doing and, where the board
//!   says why, the reason it gives,
//! - and, at each look the window has a pane, what the pane host lists of its tree
//!   (`pane.background`: the processes that began after the daemon said the turn did,
//!   each by its number and the command line it was started with), which is what the
//!   daemon holds a task's result for,
//!
//! and holds what it saw to the brief ([`verdict`]): the task is done, its result is in
//! the chief's inbox and holds the command's output, and the board never said the
//! window was held for background work. A worker that fails says everything the daemon
//! held its window for (the board's words, and from when to when, which is how long it
//! held), every process the host listed while it did with its whole command line, and
//! what the window recorded and last drew. Whatever the verdict, the run prints the
//! processes the host listed as the turn's work at the turn's end (at the first look
//! that found the window at rest or held, and before one has, at the last look), and
//! what the host said the window left running when it closed (its `pane.exit`, which a
//! look once a second cannot miss): none is the good answer, and a pass on Windows
//! still shows the tree. A time said of what the board or the host showed is on the
//! run's clock, the one the lines the run says as it goes begin with.
//!
//! The tests beside this one (`live_finish/stand_in.rs`) run the same check against the
//! rig's stand-in agent, which the real daemon and pane host take for a harness: a
//! worker that runs a command and one that leaves a job running past its turn. They
//! spend no quota and are part of `check` and of CI.
//!
//! ```text
//! cargo xtask live finish                          Claude, the default
//! cargo xtask live finish devin                    one harness
//! cargo xtask live finish claude codex opencode pi devin    each, one after the other
//! cargo xtask live finish fake                     the rig's stand-in worker: no model, no quota
//! cargo xtask live finish devin --keep             leaves the run's home and project where they are
//! cargo xtask live finish -- --list                lists this test
//! cargo xtask windows --host zeewin -- cargo xtask live finish devin
//! ```
//!
//! The run fails when a worker's task did not end as it should have. Each worker has a
//! folder of its own (`~/.consensflow-candidate/live/finish-<name>`), where Claude Code
//! is asked once to trust it.

// The parts are in `live_finish/`: cargo looks for the modules of a test's root file
// beside it, where each would be a suite of its own.
#[path = "live_finish/check.rs"]
mod check;
#[path = "live_finish/stand_in.rs"]
mod stand_in;
#[path = "live_finish/task.rs"]
mod task;
#[path = "live_finish/verdict.rs"]
mod verdict;

use std::path::Path;

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, between, Harness, Log, Who};
use cf_e2e::Result;

use check::Finished;

/// The stand-in for the chief's window, and for the worker of `fake`.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the line of each worker in the table says it checked.
const CHECKED: &str = "result reaches the chief, nothing held back";

/// The workers the words of the command line name, or Claude where it names none.
fn workers(named: &[String]) -> Result<Vec<Who>> {
    if named.is_empty() {
        return Ok(vec![Who::Harness(Harness::Claude)]);
    }
    named.iter().map(|word| check::named(word)).collect()
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short task of each harness named; cargo xtask live finish (npm run live:finish)"]
fn a_worker_that_runs_a_command_finishes_its_task_and_its_result_reaches_the_chief_with_nothing_held_back(
) -> Result {
    let log = Log::new("live finish");
    let whom = workers(&live::list("harness"))?;
    let mut checks = Vec::new();
    for who in whom {
        // A harness may update itself while it runs: the run names its version before and after.
        let before = who.version();
        let finished = check::task(who)
            .and_then(|task| check::run(who, &check::plan(who), &task, Path::new(FAKE_AGENT), &log))
            .unwrap_or_else(Finished::failed);
        let version = between(before, who.version());
        log.block(
            &format!(
                "== {} {version}: {}",
                who.name(),
                if finished.case.held() { "ok" } else { "FAILED" }
            ),
            &finished.case.text(),
        );
        checks.push(Check::new(
            who.name(),
            CHECKED,
            finished.case.held(),
            format!("{version}: {}", finished.summary),
        ));
    }
    log.block("what each worker came to", &report::table(&checks));
    report::verdict(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_worker_named_claude_is_run_and_those_named_are_run_in_the_order_given() {
        assert_eq!(workers(&[]).unwrap(), [Who::Harness(Harness::Claude)]);
        let some = workers(&["devin".to_owned(), "fake".to_owned(), "claude".to_owned()]).unwrap();
        assert_eq!(
            some,
            [
                Who::Harness(Harness::Devin),
                Who::StandIn,
                Who::Harness(Harness::Claude)
            ]
        );
        assert!(workers(&["nobody".to_owned()]).is_err());
    }
}
