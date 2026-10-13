//! The checks against the rig's stand-in agent, which the real daemon and pane host take
//! for a harness: a worker that runs a command as a process of its window and replies,
//! and a worker that leaves a job running past its turn and answers. No harness and no
//! quota, and the daemon and the host are the real ones, so what this proves is that the
//! checks read what they say (the task, the host's listing of the tree, the chief's
//! inbox, what became of the process) and say what they saw as they do of a real harness,
//! whose own behaviour is what the live run is for.

use std::path::Path;

use cf_e2e::live::members::Plan;
use cf_e2e::live::{Log, Who};
use cf_e2e::Result;

use crate::check::{self, Finished};
use crate::leaving;
use crate::task::Task;
use crate::FAKE_AGENT;

/// The stand-in worker run on `task` in a project of the plan `plan`.
fn run(plan: &Plan, task: &Task) -> Result<Finished> {
    let log = Log::new("live finish test");
    check::run(Who::StandIn, plan, task, Path::new(FAKE_AGENT), &log)
}

#[test]
fn a_stand_in_that_runs_the_command_and_replies_finishes_its_task_and_nothing_is_ended() -> Result {
    let finished = run(&check::plan(Who::StandIn), &Task::stand_in())?;
    let text = finished.case.text();
    assert!(finished.case.held(), "{text}");
    assert!(
        text.contains("ok   the result reached the chief, nothing of its own ended: T-1 done "),
        "{text}"
    );
    assert!(
        finished.summary.starts_with("T-1 done ") && finished.summary.contains("nothing ended"),
        "{}",
        finished.summary
    );
    // The turn's end is said whichever way the run came out, with what the host said the
    // window left when it closed and what the chief was told: the command it ran ended
    // with its turn, and left nothing.
    assert!(
        text.contains("what the pane host listed as the turn's work at "),
        "{text}"
    );
    assert!(
        text.contains(
            "what the pane host said the window left running when it closed (its pane.exit): none"
        ),
        "{text}"
    );
    assert!(
        text.contains("what the chief was told of work the turn left: nothing"),
        "{text}"
    );
    assert!(!text.contains("FAIL"), "{text}");
    Ok(())
}

#[test]
fn a_stand_in_that_leaves_a_job_running_and_answers_has_its_task_done_the_job_ended_and_the_chief_told(
) -> Result {
    let log = Log::new("live finish test");
    let plan = leaving::plan(Who::StandIn);
    let finished = leaving::run(Who::StandIn, &plan, Path::new(FAKE_AGENT), &log)?;
    let text = finished.case.text();
    assert!(finished.case.held(), "{text}");
    assert!(
        text.contains("ok   the command was ended with the result, and the chief told: T-1 done "),
        "{text}"
    );
    // The host listed the job as running while the turn was watched, with the command line
    // it was started with, which holds the number of seconds the check looks for.
    assert!(
        text.contains(&format!("--background-job {} ", leaving::SECONDS)),
        "{text}"
    );
    // The chief was told what was ended, in the note beside the result.
    assert!(
        text.contains(
            "what the chief was told of work the turn left:\n    T-1's turn left background work running. Ended: "
        ),
        "{text}"
    );
    // The window's close found nothing left: the ending was first.
    assert!(
        text.contains("(its pane.exit): none"),
        "what the close ended is not what the exit lists: {text}"
    );
    assert!(!text.contains("FAIL"), "{text}");
    assert!(
        finished.summary.starts_with("T-1 done after "),
        "{}",
        finished.summary
    );
    Ok(())
}
