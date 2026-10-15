//! A follow-up of 2026-10-14, ConsensFlow 3.0.3 on the owner's Mac: **a follow-up
//! the chief put before another, for the same idle window, was refused.**
//!
//! What the owner saw: T-43 was a follow-up of T-40 (`--after T-40`) for the
//! session that did T-40, waiting on the board for T-41 and T-42 (`--needs`); the
//! session's window was idle. The chief gave that session another follow-up to do
//! first, T-44 (`--after T-40 --before T-43`), and ConsensFlow refused it:
//! "@artemis-sandy-oriole is still on its work: wait for its result, or open the
//! task for its tier", though the window was on no work. The chief cancelled T-43
//! and folded its work into T-44. The report is the brain's
//! `issues/follow-up-refused-behind-a-waiting-follow-up.md`; the rule it changed,
//! `decisions/2026-10-15-follow-ups-wait-one-at-a-time.md`.
//!
//! What should have happened: T-44 goes to the window at once, and T-43 goes to the
//! same window after it, once T-44 and what T-43 needs are accepted.
//!
//! The case is that shape on the rig, with the chief's own `cf` in its window: T-1,
//! T-2 and T-3 are done, each by a session of its own; T-4 follows T-1 and needs T-2
//! and T-3; T-5 follows T-1, before T-4. Its waits leave room for Windows, where it
//! took 68 s on zeewin (13 s on the Mac).

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

/// Whether each of the tasks `numbers` is in `state`.
fn all_in(project: &Project, numbers: &[i64], state: &str) -> cf_e2e::Result<bool> {
    for number in numbers {
        if task(project, *number)?.0 != state {
            return Ok(false);
        }
    }
    Ok(true)
}

#[test]
fn a_follow_up_put_before_one_that_waits_for_the_same_idle_window_goes_to_it_and_the_waiting_one_after(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    for word in ["ONE", "TWO", "THREE"] {
        let body = format!("Reply with exactly: {word}");
        cf(&rig, &chief, &["task", "add", "--tier", tier, &body])?;
    }
    rig.wait_for("T-1, T-2 and T-3 to be done", secs(180), || {
        all_in(&project, &[1, 2, 3], "done")
    })?;
    let (_, session) = task(&project, 1)?;

    // T-4 follows T-1, and waits on the board for T-2 and T-3.
    cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--after",
            "T-1",
            "--needs",
            "T-2,T-3",
            "Reply with exactly: LATER",
        ],
    )?;
    assert_eq!(task(&project, 4)?, ("open".to_owned(), session.clone()));

    // T-5 follows T-1 too, before T-4: what 3.0.3 refused.
    cf(
        &rig,
        &chief,
        &[
            "task",
            "add",
            "--after",
            "T-1",
            "--before",
            "T-4",
            "Reply with exactly: FIRST",
        ],
    )?;
    assert_eq!(task(&project, 5)?.1, session, "the window that did T-1");
    rig.wait_for("T-5 to be done", secs(120), || {
        all_in(&project, &[5], "done")
    })?;

    // T-4 waits for T-5 and for what it needs, and goes to the window once the
    // last of them is accepted.
    for need in ["T-5", "T-2", "T-3"] {
        assert_eq!(task(&project, 4)?.0, "open", "before {need} is accepted");
        cf(&rig, &chief, &["task", "accept", need])?;
    }
    rig.wait_for("T-4 to be done", secs(120), || {
        all_in(&project, &[4], "done")
    })?;
    assert_eq!(task(&project, 4)?.1, session, "the same window");
    Ok(())
}
