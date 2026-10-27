//! Live: what ConsensFlow does with a worker's turn, on a real harness. Two steps, each
//! of them one worker after another, the proof of what the rig's stand-in can only stand
//! in for, and the shape of two field bugs (T-7, 2026-10-12: ConsensFlow 3.0.1 on Windows,
//! Devin 3000.10.21 as the worker, and T-15, 2026-10-13: Claude Code, a headless Edge check
//! its harness moved to the background, which never exits): a worker finished its task, and
//! the board kept it working with `background work running`, for processes the pane host
//! took for the turn's work. The owner decided on 2026-10-13 that a worker's answer is its
//! task's result at once (and turn-end's smallest rule of 2026-10-21 that a turn's end ends
//! nothing, the window's close does). The pane host tells the turn's work by proof: what a
//! shell the harness started ran, and what that started, by a chain of processes that run;
//! what it cannot trace so (a
//! job the agent detached with `nohup … &` from a shell that then ended) it leaves running
//! and says nothing of, as it does a server the harness starts late. These are the runs that
//! show, on a machine, what a real harness leaves, what the host ends of it, what escapes
//! it, and whether it ends anything of the harness's own. They are opt-in (`cargo xtask live
//! finish`, `npm run live:finish`): each spends the machine's quota on one short task of each
//! harness named, and needs each installed and logged in, so they are ignored unless asked
//! for and are no part of `check` or of CI.
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
//!   of it, is what the daemon did with a process the host proved the turn's, which the
//!   harness's own may have been (its servers it leaves alone, and does not name). A worker
//!   that fails says every process the host listed at the turn's end and what the window
//!   recorded and last drew;
//! - **`a_worker_that_leaves_a_long_command_running…`** asks the worker to start a long
//!   command in the background (`sleep 4711`, or the platform's own), not to wait for it
//!   and to answer. A turn's end ends nothing (turn-end's smallest rule, 2026-10-21): the
//!   task is done at once, its result is in the chief's inbox, and its window, free, closes
//!   after it and ends what its tree runs, telling the chief what was still running; or
//!   the harness ended the command itself with its tool call (Codex and Devin, seen on
//!   2026-10-14), which the run says. Claude Code runs the command as a job of its own,
//!   which it wakes its agent for: the task is held for it and the chief told it goes on,
//!   and the run, as the chief, finishes it (T-15's shape: the command never ends within
//!   the run), and goes on as for the others. A command that still runs once the window
//!   has closed escaped it, and fails the run: it says whether the host ever listed it,
//!   and it ends what still runs, so that a run leaves nothing on the machine. An agent
//!   that detaches it from a shell that then ends (`nohup … &`, `setsid`) leaves it with
//!   no chain of processes to a shell the harness started, which the close cannot find:
//!   this step is where that limit shows, harness by harness.
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
//! that runs a command, one that leaves a job running past its turn and answers, and one
//! that detaches its job from its shell, which escapes, and fails the run as it should. They
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
#[path = "live_finish/compacted.rs"]
mod compacted;
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
const CHECKED_LEAVING: &str = "command left running ended by the window's close, chief told";

/// What the line of the worker in the table says the third step checked.
const CHECKED_COMPACTED: &str = "turn compacted by Devin, result reaches the chief";

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
    log.report("what each worker came to", &report::table(&checks));
    report::verdict(&checks)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short task of each harness named; cargo xtask live finish (npm run live:finish)"]
fn a_worker_that_runs_a_command_finishes_its_task_and_its_result_reaches_the_chief_with_nothing_of_its_own_ended(
) -> Result {
    let log = Log::begin("live finish")?;
    each(&log, CHECKED, |who| {
        let task = check::task(who)?;
        check::run(who, &check::plan(who), &task, Path::new(FAKE_AGENT), &log)
    })
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short task of each harness named; cargo xtask live finish (npm run live:finish)"]
fn a_worker_that_leaves_a_long_command_running_and_answers_has_its_task_done_the_command_ended_and_the_chief_told(
) -> Result {
    let log = Log::begin("live finish (a command left running)")?;
    each(&log, CHECKED_LEAVING, |who| {
        leaving::run(who, &leaving::plan(who), Path::new(FAKE_AGENT), &log)
    })
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one task of Devin's that its compactor compacts; cargo xtask live finish devin -- a_devin_worker_whose (npm run live:finish)"]
fn a_devin_worker_whose_turn_devin_compacts_finishes_its_task_and_its_result_reaches_the_chief(
) -> Result {
    let log = Log::begin("live finish (a turn Devin compacts)")?;
    // Devin's alone: a run that names other workers and not Devin runs none of it.
    let named = live::list("harness");
    if !named.is_empty() && !named.iter().any(|word| word == "devin") {
        log.say("no Devin worker is named: this step is Devin's alone");
        return Ok(());
    }
    let who = Who::Harness(Harness::Devin);
    let before = who.version();
    let finished = compacted::run(Path::new(FAKE_AGENT), &log).unwrap_or_else(Finished::failed);
    let version = between(before, who.version());
    log.block(
        &format!(
            "== devin {version}: {}",
            if finished.case.held() { "ok" } else { "FAILED" }
        ),
        &finished.case.text(),
    );
    let checks = [Check::new(
        "devin",
        CHECKED_COMPACTED,
        finished.case.held(),
        format!("{version}: {}", finished.summary),
    )];
    log.report("what the worker came to", &report::table(&checks));
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
