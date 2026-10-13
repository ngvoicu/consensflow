//! Live: what ConsensFlow does with a worker's turn, on a real harness. Two steps, each
//! of them one worker after another, the proof of what the rig's stand-in can only stand
//! in for, and the shape of two field bugs (T-7, 2026-10-12: ConsensFlow 3.0.1 on Windows,
//! Devin 3000.10.21 as the worker, and T-15, 2026-10-13: Claude Code, a headless Edge check
//! its harness moved to the background, which never exits): a worker finished its task, and
//! the board kept it working with `background work running`, for processes the pane host
//! took for the turn's work. On Windows the pane host has no process groups to look at, so
//! what a turn left running is told by when each process began, and by what descends from
//! a shell the harness started; the owner decided on 2026-10-13 that a worker's answer is its
//! task's result at once and that what its turn left running is ended, and these are the
//! runs that show, on a machine, what a real harness leaves, what the host ends of it and
//! whether it ends anything of the harness's own. They are opt-in (`cargo xtask live
//! finish`, `npm run live:finish`): each spends the machine's quota on one short task of
//! each harness named, and needs each installed and logged in, so they are ignored unless
//! asked for and are no part of `check` or of CI.
//!
//! The native daemon and the real pane host run in a home of the run's own (never
//! `~/.consensflow`). The chief is a stand-in that only notes what it is sent, and the
//! check plays the chief with the chief's own token (`cf task add`). Each worker has a
//! folder of its own (`~/.consensflow-candidate/live/finish-<name>` and `leaving-<name>`),
//! where Claude Code is asked once to trust it. The run looks, once a second and for up to
//! five minutes, at
//!
//! - the task's state,
//! - the worker's lane of the board: what its window is doing,
//! - and, at each look the window has a pane, what the pane host lists of its tree
//!   (`pane.background`: the processes that began after the daemon said the turn did,
//!   each by its number and the command line it was started with),
//!
//! and holds what it saw to the brief of each step ([`verdict`], [`leaving`]):
//!
//! - **`a_worker_that_runs_a_command…`** gives the worker one short shell command to run
//!   with its own shell tool (the machine's `hostname`, which [`task`] says why), and a
//!   reply with a line that holds what it printed. The turn leaves nothing running, so the
//!   task is done, its result is in the chief's inbox and holds the command's output, and the
//!   chief is told nothing of work: a note that says the turn left some, and what was ended
//!   of it, is what the daemon did with a process it took for the turn's work, which the
//!   harness's own may have been. A worker that fails says every process the host listed at
//!   the turn's end and what the window recorded and last drew;
//! - **`a_worker_that_leaves_a_long_command_running…`** asks the worker to start a long
//!   command in the background (`sleep 4711`, or the platform's own), not to wait for it
//!   and to answer. The task is done at once, its result is in the chief's inbox, the host
//!   listed the command as running (a worker that left none proves nothing, and fails), the
//!   command is gone after, and the chief was told that it was ended in a note beside the
//!   result; a note that says part of it still runs and was not ended fails the run, with
//!   what the host listed.
//!
//! Whatever the verdict, a run prints the processes the host listed as the turn's work at
//! the turn's end (at the first look that found the window at rest, and before one has, at
//! the last look), what the host said the window left running when it closed (its
//! `pane.exit`, which a look once a second cannot miss), and what the chief was told: none
//! is the good answer of the first step, and a pass on Windows still shows the tree. A time
//! said of what the board or the host showed is on the run's clock, the one the lines the
//! run says as it goes begin with.
//!
//! The tests beside this one (`live_finish/stand_in.rs`) run the same checks against the
//! rig's stand-in agent, which the real daemon and pane host take for a harness: a worker
//! that runs a command and one that leaves a job running past its turn and answers. They
//! spend no quota and are part of `check` and of CI.
//!
//! Every harness has a shell tool, and an agent that is asked to can detach a command from
//! it (`nohup … &`, `Start-Process`); Claude Code and Codex have an option to run one in the
//! background as well. Whether a harness keeps what was started when its tool call ends is
//! what the second step finds out, so each harness runs both.
//!
//! ```text
//! cargo xtask live finish                          Claude, the default: both steps
//! cargo xtask live finish devin                    one harness
//! cargo xtask live finish claude codex opencode pi devin    each, one after the other
//! cargo xtask live finish claude -- a_worker_that_leaves    only the step that leaves a command
//! cargo xtask live finish fake                     the rig's stand-in worker: no model, no quota
//! cargo xtask live finish devin --keep             leaves the run's home and project where they are
//! cargo xtask live finish -- --list                lists the steps
//! cargo xtask windows --host zeewin -- cargo xtask live finish devin
//! ```
//!
//! The run fails when a worker's task did not end as it should have. Each worker has a
//! folder of its own, where Claude Code is asked once to trust it.

// The parts are in `live_finish/`: cargo looks for the modules of a test's root file
// beside it, where each would be a suite of its own.
#[path = "live_finish/check.rs"]
mod check;
#[path = "live_finish/leaving.rs"]
mod leaving;
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

/// What the line of each worker in the table says the first step checked.
const CHECKED: &str = "result reaches the chief, nothing of its own ended";

/// What the line of each worker in the table says the second step checked.
const CHECKED_LEAVING: &str = "command left running ended with the result, chief told";

/// The workers the words of the command line name, or Claude where it names none.
fn workers(named: &[String]) -> Result<Vec<Who>> {
    if named.is_empty() {
        return Ok(vec![Who::Harness(Harness::Claude)]);
    }
    named.iter().map(|word| check::named(word)).collect()
}

/// `step` run for each worker the command line names, one after the other, and the run
/// ended with what each came to: `checked` is what the table says was checked.
fn each(log: &Log, checked: &str, step: impl Fn(Who) -> Result<Finished>) -> Result {
    let mut checks = Vec::new();
    for who in workers(&live::list("harness"))? {
        // A harness may update itself while it runs: the run names its version before and after.
        let before = who.version();
        let finished = step(who).unwrap_or_else(Finished::failed);
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
            checked,
            finished.case.held(),
            format!("{version}: {}", finished.summary),
        ));
    }
    log.block("what each worker came to", &report::table(&checks));
    report::verdict(&checks)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short task of each harness named; cargo xtask live finish (npm run live:finish)"]
fn a_worker_that_runs_a_command_finishes_its_task_and_its_result_reaches_the_chief_with_nothing_of_its_own_ended(
) -> Result {
    let log = Log::new("live finish");
    each(&log, CHECKED, |who| {
        let task = check::task(who)?;
        check::run(who, &check::plan(who), &task, Path::new(FAKE_AGENT), &log)
    })
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short task of each harness named; cargo xtask live finish (npm run live:finish)"]
fn a_worker_that_leaves_a_long_command_running_and_answers_has_its_task_done_the_command_ended_and_the_chief_told(
) -> Result {
    let log = Log::new("live finish (a command left running)");
    each(&log, CHECKED_LEAVING, |who| {
        leaving::run(who, &leaving::plan(who), Path::new(FAKE_AGENT), &log)
    })
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
