//! A report of Gabriel's of 2026-10-26, ConsensFlow 3.0.6-alpha.2 (board task T-721; the
//! issue `the-delete-finished-icon-hides-when-every-finished-task-is-needed` in the brain):
//! **the Finished column could not be cleared, and its Delete finished icon was gone with
//! no word why.**
//!
//! What he saw: 48 finished tasks on the board, every one needed by a task the chief held
//! (the chief's holding stages are paused tasks that wait on accepted steps). The page
//! drew Delete finished only when a finished task could go, and the daemon refused every
//! task another one needed, so none could.
//!
//! What should have happened: deleting a task only takes it off the board, and what waits
//! for it reads it as before, so an accepted task goes though a task needs it, and the task
//! that needs it still reads it as accepted and goes on when it is resumed. (The page's
//! control and its dialog are the browser suite's; a failed task, which the chief can
//! reopen, stays while a task needs it: the ledger's and the page's tests.)
//!
//! The case is that shape on the rig, with the chief's own `cf` in its window and the page's
//! operation as the human's Delete finished asks it: T-1 is accepted, and the held T-2 needs it.

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// `cf` as the chief runs it in its window: what it printed, and a `cf` that
/// fails is an error.
fn cf(rig: &Rig, chief: &Value, words: &[&str]) -> cf_e2e::Result<String> {
    let ran = rig.cf_in_window(chief, words)?;
    if ran.code == Some(0) {
        Ok(ran.stdout)
    } else {
        Err(Error::Daemon(format!("cf {}: {ran}", words.join(" "))))
    }
}

fn state(project: &Project, number: i64) -> cf_e2e::Result<String> {
    Ok(project.task(number)?["state"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

#[test]
fn an_accepted_task_a_held_task_needs_is_deleted_and_the_held_task_goes_on_when_resumed() -> Outcome
{
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    // T-1 is done; T-2 needs it, and the chief holds T-2 until T-1 is accepted.
    cf(
        &rig,
        &chief,
        &["task", "add", "--tier", tier, "Reply with exactly: ONE"],
    )?;
    rig.wait_for("T-1 to be done", secs(120), || {
        Ok(state(&project, 1)? == "done")
    })?;
    cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--tier",
            tier,
            "--needs",
            "T-1",
            "Reply with exactly: TWO",
        ],
    )?;
    cf(&rig, &chief, &["task", "pause", "T-2"])?;
    cf(&rig, &chief, &["task", "accept", "T-1"])?;
    assert_eq!(state(&project, 1)?, "accepted");
    assert_eq!(state(&project, 2)?, "paused", "the hold stays");

    // The human deletes the finished tasks, as the page's control asks: T-1 goes though
    // T-2 needs it, where the daemon refused "T-2 still needs T-1" and the column stayed full.
    let deleted = rig.page(
        "tasks.delete",
        json!({ "project": project.id(), "tasks": [1] }),
    )?;
    assert_eq!(deleted["ok"], true, "{deleted}");
    let listed = |board: &Value| -> Vec<i64> {
        board["open"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(
                board["lanes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|lane| lane["tasks"].as_array().into_iter().flatten()),
            )
            .filter_map(|task| task["number"].as_i64())
            .collect()
    };
    assert!(
        !listed(&project.board()?).contains(&1),
        "T-1 is off the board"
    );

    // T-2 still reads T-1 as accepted, and goes on when the chief resumes it.
    let held = project.task(2)?;
    assert_eq!(held["needs"], json!([{ "number": 1, "state": "accepted" }]));
    cf(&rig, &chief, &["task", "resume", "T-2", "Go on"])?;
    rig.wait_for("T-2 to be done", secs(180), || {
        Ok(state(&project, 2)? == "done")
    })?;
    rig.close()?;
    Ok(())
}
