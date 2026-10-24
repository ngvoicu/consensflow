//! A gap the chief of this project hit on 2026-10-21, ConsensFlow 3.0.5 (board task
//! T-110): **a brief could not be edited once it was added.**
//!
//! What the chief saw: a task waiting on the board for another (`--needs`), or queued
//! for a window that had not begun on it, could only be cancelled and added again to
//! say something else, which gave it a new number and lost its place and the tasks
//! that waited for it.
//!
//! What should have happened: `cf task edit T-n "…"` replaces the brief of a task no
//! window was given yet, the task keeps its place, and the window that gets it gets
//! the new words; a task a window has is refused, in words that say to tell it.
//!
//! The case is that shape on the rig, with the chief's own `cf` in its window: T-2
//! waits for T-1, which is at work.

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
fn a_blocked_task_edited_starts_with_the_new_brief_and_a_task_at_work_is_refused() -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--tier",
            tier,
            "SLEEP 25 Reply with exactly: ONE",
        ],
    )?;
    rig.wait_for("T-1 to be at work", secs(180), || {
        Ok(state(&project, 1)? == "working")
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
            "Reply with exactly: OLD",
        ],
    )?;
    assert_eq!(state(&project, 2)?, "open");

    // The blocked task takes the new brief and keeps its place.
    let said = cf(
        &rig,
        &chief,
        &["task", "edit", "T-2", "Reply with exactly: NEW"],
    )?;
    assert!(said.contains("T-2 has your new brief"), "{said}");
    let read = cf(&rig, &chief, &["task", "get", "T-2"])?;
    assert!(read.contains("Reply with exactly: NEW"), "{read}");
    assert!(!read.contains("OLD"), "{read}");
    assert_eq!(state(&project, 2)?, "open");

    // The task whose window has its brief is told, never edited.
    let ran = rig.cf_in_window(&chief, ["task", "edit", "T-1", "Something else"])?;
    assert_eq!(ran.code, Some(1), "{ran}");
    assert!(ran.stdout.is_empty(), "{ran}");
    assert!(
        ran.stderr
            .starts_with("cf: cannot edit T-1: it was given to @"),
        "{ran}"
    );
    assert!(
        ran.stderr.contains("tell its window: cf tell T-1 \"…\""),
        "{ran}"
    );

    // T-2 starts with the new brief once T-1 is accepted.
    rig.wait_for("T-1 to be done", secs(120), || {
        Ok(state(&project, 1)? == "done")
    })?;
    cf(&rig, &chief, &["task", "accept", "T-1"])?;
    rig.wait_for("T-2 to be done", secs(180), || {
        Ok(state(&project, 2)? == "done")
    })?;
    let result = cf(&rig, &chief, &["task", "get", "T-2"])?;
    assert!(result.contains("NEW"), "{result}");
    assert!(!result.contains("OLD"), "{result}");
    rig.close()?;
    Ok(())
}
