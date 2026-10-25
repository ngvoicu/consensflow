//! A report of 2026-10-21, ConsensFlow 3.0.5, the chief of this project (board task
//! T-74): **a task that waited on a cancelled task stayed blocked for good.**
//!
//! What the chief saw: `--needs T-n` releases a task when T-n is *accepted*, and
//! adding a task that needs an already cancelled one is refused. But `cf task cancel
//! T-n` cancelled a task that open tasks still waited on, and nothing told anyone:
//! those tasks read "blocked by T-n" for good. The chief found it by reading the code
//! and steered around it all night (`--before` on new tasks instead of cancel and
//! re-add).
//!
//! What should have happened: `cf task cancel T-n` is refused while open tasks wait
//! on T-n, in words that name them and say what to do (cancel them first, or let T-n
//! finish), and so is the page's Cancel; once they are cancelled, T-n is.
//!
//! The case is that shape on the rig, with the chief's own `cf` in its window and the
//! page's operation as the human's Cancel button asks it: T-1 stays with its worker,
//! T-2 and T-3 wait on the board for it.

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// What the board answers a cancel of T-1 while T-2 and T-3 wait for it.
const REFUSED: &str =
    "T-1 cannot be cancelled while T-2, T-3 wait for it: cancel them first, or let T-1 finish";

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
fn a_task_open_tasks_wait_for_is_not_cancelled_by_the_chief_or_the_page_until_they_are() -> Outcome
{
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    // T-1 stays with its worker; T-2 and T-3 wait on the board for it.
    cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--tier",
            tier,
            "SLEEP 120 Reply with exactly: ONE",
        ],
    )?;
    rig.wait_for("T-1 to be at work", secs(180), || {
        Ok(state(&project, 1)? == "working")
    })?;
    for word in ["TWO", "THREE"] {
        let body = format!("Reply with exactly: {word}");
        cf(
            &rig,
            &chief,
            &["task", "add", "--tier", tier, "--needs", "T-1", &body],
        )?;
    }
    assert_eq!(state(&project, 2)?, "open");
    assert_eq!(state(&project, 3)?, "open");

    // The chief's `cf` is refused in the board's words, and so is the page's Cancel.
    let ran = rig.cf_in_window(&chief, ["task", "cancel", "T-1"])?;
    assert_eq!(ran.code, Some(1), "{ran}");
    assert!(ran.stdout.is_empty(), "{ran}");
    assert_eq!(ran.stderr, format!("cf: {REFUSED}\n"));
    let refused = rig.page("task.cancel", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"], REFUSED, "{refused}");
    assert_eq!(state(&project, 1)?, "working", "nothing was cancelled");

    // They are cancelled first, and then the task is.
    cf(&rig, &chief, &["task", "cancel", "T-2"])?;
    cf(&rig, &chief, &["task", "cancel", "T-3"])?;
    cf(&rig, &chief, &["task", "cancel", "T-1"])?;
    assert_eq!(state(&project, 1)?, "cancelled");
    rig.close()?;
    Ok(())
}
