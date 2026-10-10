//! A tell and a cancel, end to end through the real pane host
//! (`npm run test:daemons`). The chief eval `six-decisions` (2026-10-07, on the
//! native daemon) counted "every tell the chief sent was answered (1/2)": the
//! chief had told the window of T-2, was in a turn of its own for minutes, and
//! called T-2 off. Nothing is delivered to a window that is at work, so an
//! answer to the tell waited in the chief's queue, and a task called off takes
//! back whatever of it is still on its way: the answer the window gave was
//! cancelled before the chief read it, and the eval's count, which skips a
//! cancelled answer, took the tell for unanswered.

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::{rig, secs, Outcome};

/// The messages of the task T-1's thread of `kind`, in the order they were made.
fn thread(project: &Project, kind: &str) -> cf_e2e::Result<Vec<Value>> {
    Ok(project.task(1)?["messages"]
        .as_array()
        .map(|messages| {
            messages
                .iter()
                .filter(|m| m["kind"] == kind)
                .cloned()
                .collect()
        })
        .unwrap_or_default())
}

/// A project with a worker whose task T-1 is working when the chief tells it:
/// its turn takes fifteen seconds. Once the chief's tell is in, T-1 is paused.
/// The project it is.
fn told_while_working(rig: &Rig) -> cf_e2e::Result<Project<'_>> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} SLEEP 15 Reply with exactly: ONE",
            added["member"]["tier"].as_str().unwrap_or_default()
        ),
    )?;
    rig.wait_for("T-1 to be working", secs(30), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    rig.tell(project.id(), "CF tell T-1 :: Stop now. REPLY stopped")?;
    rig.wait_for("T-1 to be paused", secs(30), || {
        Ok(project.task(1)?["state"] == "paused")
    })?;
    Ok(project)
}

#[test]
fn the_answer_a_window_gave_the_chiefs_tell_reaches_the_chief_once_its_turn_is_over_and_stays_an_answer_when_the_task_is_called_off_after(
) -> Outcome {
    let rig = rig()?;
    let project = told_while_working(&rig)?;

    // The window comes to rest, takes the tell in and answers it; the chief is idle, and has it.
    rig.wait_for("the answer to be delivered", secs(60), || {
        Ok(thread(&project, "answer")?
            .iter()
            .any(|m| m["state"] == "delivered"))
    })?;
    let tell = thread(&project, "question")?[0].clone();
    let answered = thread(&project, "answer")?[0].clone();
    assert_eq!(
        [
            tell["urgent"].clone(),
            tell["state"].clone(),
            answered["replyTo"].clone(),
            answered["recipient"].clone(),
            answered["body"].clone()
        ],
        [
            json!(true),
            json!("delivered"),
            tell["id"].clone(),
            json!("chief"),
            json!("stopped")
        ]
    );

    let cancelled = rig.page("task.cancel", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(cancelled["ok"], true, "{cancelled}");
    let states: Vec<Value> = thread(&project, "answer")?
        .iter()
        .map(|m| m["state"].clone())
        .collect();
    assert_eq!(
        states,
        [json!("delivered")],
        "what the chief has is not taken back"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn the_answer_a_window_gave_the_chiefs_tell_is_withdrawn_when_the_chiefs_task_is_called_off_before_the_chief_read_it(
) -> Outcome {
    let rig = rig()?;
    let project = told_while_working(&rig)?;
    // The chief goes into a turn of its own: nothing is delivered to it until that is over.
    rig.tell(project.id(), "SLEEP 90 Reply with exactly: BUSY")?;

    // The window comes to rest, takes the tell in and answers it with cf: the answer waits for the chief.
    rig.wait_for("the window to answer", secs(60), || {
        Ok(thread(&project, "answer")?.len() == 1)
    })?;
    // Whether the daemon has read the window's record of the tell yet is its own look's to say.
    rig.wait_for("the tell to be delivered", secs(30), || {
        Ok(thread(&project, "question")?[0]["state"] == "delivered")
    })?;
    let tell = thread(&project, "question")?[0].clone();
    assert_eq!(
        [
            tell["urgent"].clone(),
            tell["sender"].clone(),
            tell["body"].clone(),
            tell["state"].clone()
        ],
        [
            json!(true),
            json!("chief"),
            json!("Stop now. REPLY stopped"),
            json!("delivered")
        ],
        "the tell reached the window"
    );
    let answered = thread(&project, "answer")?[0].clone();
    assert_eq!(
        [
            answered["replyTo"].clone(),
            answered["recipient"].clone(),
            answered["body"].clone(),
            answered["state"].clone()
        ],
        [
            tell["id"].clone(),
            json!("chief"),
            json!("stopped"),
            json!("queued")
        ],
        "the window answered it, and the chief has not read the answer"
    );

    // The task is called off before the chief reads it: by the human here, as the chief's own
    // window is busy; the ledger's cancel is the same, and its reason names who made it.
    let cancelled = rig.page("task.cancel", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(cancelled["ok"], true, "{cancelled}");
    let kept = thread(&project, "question")?[0].clone();
    let withdrawn = thread(&project, "answer")?[0].clone();
    assert_eq!(
        [
            kept["state"].clone(),
            withdrawn["state"].clone(),
            withdrawn["reason"].clone()
        ],
        [
            json!("delivered"),
            json!("cancelled"),
            json!("cancelled by @human")
        ],
        "the tell stays as it was delivered, and its answer is taken back"
    );
    // What the chief eval counts as an answer to a tell is one that is not cancelled: none.
    let counted: Vec<Value> = thread(&project, "answer")?
        .into_iter()
        .filter(|m| m["state"] != "cancelled")
        .collect();
    assert_eq!(counted, Vec::<Value>::new());
    rig.close()?;
    Ok(())
}
