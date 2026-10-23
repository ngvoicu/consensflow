//! A gap the chief of this project hit on 2026-10-21, ConsensFlow 3.0.5 (board task
//! T-110): **a follow-up could not be queued for a window that was still at work.**
//!
//! What the chief saw: `cf task add --after T-3 "…"` was refused while T-3's window
//! was still on T-3 ("@… is still on its work: wait for its result, or open the task
//! for its tier"). So it could not queue the next step of a window's own work while
//! the window ran, and gave the steps to windows of their own that night.
//!
//! What should have happened: the follow-up is taken, waits on the board for T-3 to
//! be accepted, and goes into that same window then, as the follow-up of a finished
//! task does; of two queued behind one window, the first goes first, and the second
//! once the first has its result.
//!
//! The case is that shape on the rig, with the chief's own `cf` in its window: T-1
//! is at work when T-2 and T-3 are added after it.

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
fn a_follow_up_added_while_its_window_is_at_work_waits_for_the_task_and_goes_into_the_same_window(
) -> Outcome {
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
        Ok(task(&project, 1)?.0 == "working")
    })?;
    let session = task(&project, 1)?.1;

    // What 3.0.5 refused: both are taken, and say what they wait for.
    let said = cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--after",
            "T-1",
            "SLEEP 10 Reply with exactly: TWO",
        ],
    )?;
    assert!(said.contains("It waits until T-1 is accepted."), "{said}");
    cf(
        &rig,
        &chief,
        &["task", "add", "--after", "T-1", "Reply with exactly: THREE"],
    )?;
    for number in [2, 3] {
        assert_eq!(
            task(&project, number)?,
            ("open".to_owned(), session.clone())
        );
    }

    // T-1 has its result, and is not accepted: they still wait.
    rig.wait_for("T-1 to be done", secs(120), || {
        Ok(task(&project, 1)?.0 == "done")
    })?;
    assert_eq!(task(&project, 2)?.0, "open");

    // Accepted, T-2 goes to the same window; T-3 waits behind it.
    cf(&rig, &chief, &["task", "accept", "T-1"])?;
    rig.wait_for("T-2 to be at work", secs(120), || {
        Ok(task(&project, 2)?.0 == "working")
    })?;
    assert_eq!(task(&project, 2)?.1, session, "the same window");
    assert_eq!(task(&project, 3)?.0, "open", "behind T-2");
    rig.wait_for("T-3 to be done", secs(180), || {
        Ok(task(&project, 3)?.0 == "done")
    })?;
    assert_eq!(task(&project, 2)?.0, "done", "T-2 had its result first");
    assert_eq!(task(&project, 3)?.1, session, "the same window");
    rig.close()?;
    Ok(())
}
