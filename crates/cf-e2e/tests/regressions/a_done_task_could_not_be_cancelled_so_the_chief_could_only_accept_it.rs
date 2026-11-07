//! A field report of 2026-11-02, ConsensFlow 3.0.7-alpha.1, Gabriel's other board
//! (board task T-1122): **a task that was done could not be cancelled, so the chief
//! could only accept work that was never done.**
//!
//! What the chief saw: T-6 was done, and its result said the worker had stopped
//! before doing any work. `cf task cancel T-6` answered "cf: cannot cancel T-6: it is
//! done"; `cf task reopen T-6` answered that its window was still on its work (a
//! follow-up that went into the window before T-6 was decided was paused). The
//! chief's own text tells it to decide a result with `cf task accept`, `cf task
//! reopen` or `cf task cancel`; only the first was left.
//!
//! What should have happened: a done task is cancelled by the chief's `cf`, as its text
//! says, and so decided: it is not accepted afterwards. Another done task is not
//! touched, and is accepted as before.
//!
//! The case is that shape on the rig, with the chief's own `cf` in its window: T-1
//! and T-2 are done, each by a session of its own, and the chief cancels T-1.
//!
//! The report's own window is the second case. T-6's window held a paused follow-up
//! (T-8, `--after T-6`), which went in before T-6 was decided, and the chief could
//! neither send T-6 back nor cancel it. That shape cannot arise through a follow-up
//! now (T-1118): a follow-up goes into no window that holds a result the chief has not
//! decided, whichever task it names, so T-2 (`--after T-1`) waits for T-1 and nothing
//! enters T-1's window. No door of `cf` builds it otherwise: a task named for a window is
//! the ledger's, and a follow-up resumed early still needs T-1, so T-1 is refused its
//! cancel (`waited-for`) until that follow-up is cancelled. The case pins that: T-2
//! waits; T-1 cannot be cancelled while it does, in words that say what to do; T-1 is
//! sent back to its window; and with T-2 cancelled, T-1 is cancelled.

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
fn a_done_task_is_cancelled_by_the_chiefs_cf_and_no_other_is_touched() -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    for word in ["ONE", "TWO"] {
        let body = format!("Reply with exactly: {word}");
        cf(&rig, &chief, &["task", "add", "--tier", tier, &body])?;
    }
    rig.wait_for("T-1 and T-2 to be done", secs(180), || {
        Ok(state(&project, 1)? == "done" && state(&project, 2)? == "done")
    })?;
    // What 3.0.7-alpha.1 refused: "cannot cancel T-1: it is done".
    let said = cf(&rig, &chief, &["task", "cancel", "T-1"])?;
    assert!(said.contains("T-1 [cancelled]"), "{said}");
    assert_eq!(state(&project, 1)?, "cancelled");

    // Nothing of it is left to decide, and the other task is as it was.
    let ran = rig.cf_in_window(&chief, ["task", "accept", "T-1"])?;
    assert_eq!(ran.code, Some(1), "{ran}");
    assert_eq!(ran.stderr, "cf: cannot accept T-1: it is cancelled\n");
    assert_eq!(state(&project, 2)?, "done");
    cf(&rig, &chief, &["task", "accept", "T-2"])?;
    assert_eq!(state(&project, 2)?, "accepted");
    rig.close()?;
    Ok(())
}

#[test]
fn a_follow_up_waits_for_the_done_task_the_chief_decides_and_that_task_is_sent_back_or_cancelled(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    cf(
        &rig,
        &chief,
        &["task", "add", "--tier", tier, "Reply with exactly: ONE"],
    )?;
    rig.wait_for("T-1 to be done", secs(180), || {
        Ok(state(&project, 1)? == "done")
    })?;
    let session = project.task(1)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned();

    // T-2 follows T-1, whose result is in and not decided: it waits, and the window
    // takes nothing (what 3.0.7-alpha.1 did: T-2 went in, and could be paused there).
    let said = cf(
        &rig,
        &chief,
        &["task", "add", "--after", "T-1", "Reply with exactly: TWO"],
    )?;
    assert!(said.contains("It waits until T-1 is accepted."), "{said}");
    assert_eq!(project.task(2)?["state"], "open");
    assert_eq!(project.task(2)?["assignee"], session.as_str());

    // T-1 cannot be cancelled while T-2 waits for it, and the words say what to do.
    let ran = rig.cf_in_window(&chief, ["task", "cancel", "T-1"])?;
    assert_eq!(ran.code, Some(1), "{ran}");
    assert_eq!(
        ran.stderr,
        "cf: T-1 cannot be cancelled while T-2 waits for it: accept T-1, or cancel T-2 first\n"
    );

    // The window holds nothing, so T-1 goes back to it; T-2 still waits.
    cf(
        &rig,
        &chief,
        &["task", "reopen", "T-1", "Reply with exactly: AGAIN"],
    )?;
    assert_ne!(state(&project, 1)?, "done", "T-1 went back to its window");
    rig.wait_for("T-1 to be done again", secs(180), || {
        Ok(state(&project, 1)? == "done")
    })?;
    assert_eq!(project.task(1)?["assignee"], session.as_str());
    assert_eq!(project.task(2)?["state"], "open", "T-2 still waits");

    // With the waiter cancelled, T-1 is cancelled: a done task is decided by cancel.
    cf(&rig, &chief, &["task", "cancel", "T-2"])?;
    let said = cf(&rig, &chief, &["task", "cancel", "T-1"])?;
    assert!(said.contains("T-1 [cancelled]"), "{said}");
    assert_eq!(state(&project, 1)?, "cancelled");
    rig.close()?;
    Ok(())
}
