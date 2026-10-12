//! The check against the rig's stand-in agent, which the real daemon and the real pane
//! host take for a harness: a worker that runs a command as a process of its window
//! and replies, and a worker that leaves a job running past its turn. No harness and no
//! quota, and the daemon and the host are the real ones, so what this proves is that the
//! check reads what they say (the task, the lane's activity and its reason, the host's
//! listing of the tree, the chief's inbox) and says what it saw as it does of a real
//! harness, whose own behaviour is what the live run is for.

use std::path::Path;

use cf_e2e::live::members::Plan;
use cf_e2e::live::{Log, Who};
use cf_e2e::Result;

use crate::check::{self, Finished};
use crate::task::Task;
use crate::FAKE_AGENT;

/// How long the daemon waits, once a job has ended, for the agent to go on
/// (`CONSENSFLOW_BACKGROUND_SETTLE_MS`): its default is ten seconds, which these tests
/// have no use for.
const SETTLE_MS: &str = "1500";

/// How long the stand-in's job runs: past the passes the daemon takes it in, so that the
/// check looks at the held window many times.
const JOB_SECONDS: u64 = 10;

/// The stand-in worker run on `task` in a project of the plan `plan`.
fn run(plan: &Plan, task: &Task) -> Result<Finished> {
    let log = Log::new("live finish test");
    check::run(Who::StandIn, plan, task, Path::new(FAKE_AGENT), &log)
}

#[test]
fn a_stand_in_that_runs_the_command_and_replies_finishes_its_task_with_nothing_held_back() -> Result
{
    let finished = run(&check::plan(Who::StandIn), &Task::stand_in())?;
    let text = finished.case.text();
    assert!(finished.case.held(), "{text}");
    assert!(
        text.contains("ok   the result reached the chief, nothing held back: T-1 done "),
        "{text}"
    );
    assert!(
        finished.summary.starts_with("T-1 done ") && finished.summary.contains("nothing held back"),
        "{}",
        finished.summary
    );
    // The turn's end is said whichever way the run came out, with what the board said of
    // the window, and what the host said the window left when it closed: the command it
    // ran ended with its turn, and left nothing.
    assert!(
        text.contains("what the pane host listed as the turn's work at "),
        "{text}"
    );
    assert!(
        text.contains("what the board said of the worker's window: "),
        "{text}"
    );
    assert!(
        text.contains(
            "what the pane host said the window left running when it closed (its pane.exit): none"
        ),
        "{text}"
    );
    assert!(!text.contains("FAIL"), "{text}");
    Ok(())
}

#[test]
fn a_stand_in_that_leaves_a_job_running_past_its_turn_fails_and_says_what_held_it_and_for_how_long(
) -> Result {
    let plan = check::plan(Who::StandIn).with_var("CONSENSFLOW_BACKGROUND_SETTLE_MS", SETTLE_MS);
    let finished = run(&plan, &Task::stand_in_leaving(JOB_SECONDS))?;
    let text = finished.case.text();
    assert!(!finished.case.held(), "{text}");
    // What the daemon held the window for, in the board's words, and from when to when.
    assert!(
        text.contains(
            "FAIL the board said the worker's window was held for work its turn left, from "
        ),
        "{text}"
    );
    assert!(
        text.contains("\"background work running ("),
        "the board's own words: {text}"
    );
    assert!(text.contains(" of the run, which is "), "{text}");
    // The process the host listed while it did, with the command line it was started with.
    assert!(
        text.contains(
            "FAIL what the pane host listed as the turn's work while the window was held"
        ),
        "{text}"
    );
    assert!(
        text.contains(&format!("--background-job {JOB_SECONDS} ")),
        "{text}"
    );
    // The turn's end is the window held, with the job the host listed then, a line to each
    // process; the job ended by itself before the window closed, which the host says.
    assert!(
        text.contains("at the turn's end (at ") && text.contains(", the window held):\n    "),
        "{text}"
    );
    assert!(
        text.contains(
            "what the pane host said the window left running when it closed (its pane.exit): none"
        ),
        "{text}"
    );
    // The task ended and its result reached the chief with what the job's turn said: that
    // is what held, and what did not is only that the window was held.
    assert!(!text.contains("not done"), "{text}");
    assert!(!text.contains("the chief was given no result"), "{text}");
    assert!(!text.contains("the result does not hold"), "{text}");
    // The table says the first of what was wrong, which is the hold.
    assert!(
        finished
            .summary
            .starts_with("the board said the worker's window was held for work its turn left"),
        "{}",
        finished.summary
    );
    // And the worker's lane, its window and the log are said beside it.
    assert!(
        text.contains("     the worker's lane of the board:\n"),
        "{text}"
    );
    Ok(())
}
