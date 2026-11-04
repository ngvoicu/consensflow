//! A worker's whole write-up was lost when a notice of its harness's began the next turn.
//!
//! Seen on this board, 2026-11-04 02:0x, 3.0.7-alpha.1 (T-1200, T-1219; found out in the record).
//! The worker's Bash tool moved a command to the background; the command ended while the turn
//! was at work, and Claude Code queued its `<task-notification>`; the turn ended with an 8.8 KB
//! write-up while that message was still queued (a turn is not over while one is), and the
//! notification began the next turn 0.1 s later, which ended with three lines ("My result … is
//! unchanged"). Those three lines were the task's result (m-7349); the write-up never reached
//! the board.
//!
//! The same loss happens to a turn held for a job that runs, which the hold's note told once and
//! the result did not give: the rig's `CLAUDE-JOB` is that door, a job in the background, the
//! worker's words at once and the report when the job's end wakes it (the stand-in queues the
//! notification when it takes the turn, so the race of the queue itself is the dispatcher's
//! case `result_after_held_turn`, and the Claude reader's test of the record's shape).
//!
//! What the owner should have seen: a result that begins with the write-up, labelled with what
//! began the next turn, and ends with "What it said last:" and the report. A short line about
//! where it stands is still not in the result.

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::{config, secs, Outcome};

/// The write-up the worker's turn ended with, in one line as the stand-in says one.
const WRITE_UP: &str = "T-1200: integration a2-x is merged, checked and pushed. The three branches are merged onto a3-1 and the checks are green: the dispatcher suite passes in full, the rig cases pass on the Mac and on zeewin2, and the plant tables of both branches are caught. The pins moved by 4 and 6 bytes and stay under their ceilings. The build folder was removed and the tree is clean. Open points: the Windows leg of the Muse cases waits for the isolation of T-1084, and the notice of the removal should name the folder it freed.";

/// What the turn the job's end woke ended with.
const REPORT: &str = "My result above is unchanged and the removal ended.";

/// The label of a turn that a notice of the harness's followed.
const BEFORE_A_NOTICE: &str =
    "Written before a notice of its harness's (or words typed into its window) began its next turn:";

/// A board whose worker is given T-1 with `line`: the project.
fn given<'a>(rig: &'a Rig, line: &str) -> cf_e2e::Result<Project<'a>> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added: Value = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} {line}", tier(&added)),
    )?;
    Ok(project)
}

/// The result of T-1 once it is done, with the thread it was told in.
fn the_result(rig: &Rig, project: &Project) -> cf_e2e::Result<(String, Vec<(String, String)>)> {
    rig.wait_for("T-1 to be done", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let thread = told(project)?;
    let results: Vec<String> = thread
        .iter()
        .filter(|(kind, _)| kind == "result")
        .map(|(_, body)| body.clone())
        .collect();
    assert_eq!(results.len(), 1, "{thread:?}");
    Ok((results[0].clone(), thread))
}

#[test]
fn a_write_up_the_turn_ended_with_while_a_job_ran_is_in_the_result_before_the_turn_its_end_woke(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = given(&rig, &format!("CLAUDE-JOB 8 {WRITE_UP} :: {REPORT}"))?;
    let (result, thread) = the_result(&rig, &project)?;

    assert_eq!(
        result,
        format!("{BEFORE_A_NOTICE}\n\n{WRITE_UP}\n\nWhat it said last:\n\n{REPORT}"),
        "the write-up, then the report"
    );
    let kinds: Vec<&str> = thread.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["note", "result"],
        "the hold was told once: {thread:?}"
    );
    assert!(
        thread[0].1.ends_with(WRITE_UP),
        "with the words: {}",
        thread[0].1
    );
    rig.close()?;
    Ok(())
}

/// Where the worker stands is still no part of the result: the report alone, as it was.
#[test]
fn where_the_worker_stands_when_its_job_runs_is_still_not_in_the_result() -> Outcome {
    let rig = Rig::start(config())?;
    let project = given(
        &rig,
        &format!("CLAUDE-JOB 8 The removal is still running in the background :: {REPORT}"),
    )?;
    let (result, _) = the_result(&rig, &project)?;

    assert_eq!(result, REPORT);
    rig.close()?;
    Ok(())
}
