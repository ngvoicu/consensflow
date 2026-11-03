//! A field report of 2026-11-02, ConsensFlow 3.0.7-alpha.1, Gabriel's other board
//! (board task T-1122): **a window that held a paused task was refused in the words
//! of one at work, naming nothing.**
//!
//! What the chief saw: sending T-6 back was refused with "@sepa-lunar-comet is still
//! on its work: wait for its result before sending T-6 back, or open a new task for
//! its tier". The window's "work" was T-8, which was paused, and waits for the chief's
//! resume, not for a result: waiting would never end it. The words named neither the
//! task nor its state, and offered no way out but a new task.
//!
//! What should have happened: the refusal names the task that holds the window and
//! its state, and says how the chief frees it: for a paused task, resume it and wait
//! for its result, or cancel it. Every door that refusal serves says it: a follow-up,
//! a task sent back, a task named for the window and a resume.
//!
//! The case is that shape on the rig, with the chief's own `cf` in its window, at the
//! door the chief reaches whatever else its board does: T-1 is accepted, T-2 follows
//! it into the same window and is paused, and a second follow-up of T-1 is refused.
//! The chief does what the words say (cancels T-2), and the follow-up goes.

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

/// The state of the task `number`, and the session it is given to.
fn task(project: &Project, number: i64) -> cf_e2e::Result<(String, String)> {
    let task = project.task(number)?;
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    Ok((text(&task["state"]), text(&task["assignee"])))
}

#[test]
fn a_follow_up_for_a_window_that_holds_a_paused_task_is_refused_in_words_that_name_it_and_say_how_to_free_the_window(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    // T-1 is accepted, and T-2 follows it into the same window and is paused.
    cf(
        &rig,
        &chief,
        &["task", "add", "--tier", tier, "Reply with exactly: ONE"],
    )?;
    rig.wait_for("T-1 to be done", secs(180), || {
        Ok(task(&project, 1)?.0 == "done")
    })?;
    let session = task(&project, 1)?.1;
    cf(&rig, &chief, &["task", "accept", "T-1"])?;
    cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--after",
            "T-1",
            "SLEEP 120 Reply with exactly: TWO",
        ],
    )?;
    rig.wait_for("T-2 to be at work", secs(120), || {
        Ok(task(&project, 2)?.0 == "working")
    })?;
    cf(&rig, &chief, &["task", "pause", "T-2"])?;
    rig.wait_for("T-2 to be paused", secs(60), || {
        Ok(task(&project, 2)?.0 == "paused")
    })?;

    // What 3.0.7-alpha.1 said: "@… is still on its work: wait for its result, or
    // open the task for its tier".
    let ran = rig.cf_in_window(
        &chief,
        ["task", "add", "--after", "T-1", "Reply with exactly: THREE"],
    )?;
    assert_eq!(ran.code, Some(1), "{ran}");
    assert!(ran.stdout.is_empty(), "{ran}");
    assert_eq!(
        ran.stderr,
        format!(
            "cf: @{session}'s window holds T-2, paused: resume T-2 and wait for its result, or cancel T-2, before giving it another task; or open the task for its tier\n"
        )
    );

    // The chief does what the words say, and the window takes the follow-up.
    cf(&rig, &chief, &["task", "cancel", "T-2"])?;
    cf(
        &rig,
        &chief,
        &["task", "add", "--after", "T-1", "Reply with exactly: THREE"],
    )?;
    rig.wait_for("T-3 to be done", secs(180), || {
        Ok(task(&project, 3)?.0 == "done")
    })?;
    assert_eq!(task(&project, 3)?.1, session, "the same window");
    rig.close()?;
    Ok(())
}
