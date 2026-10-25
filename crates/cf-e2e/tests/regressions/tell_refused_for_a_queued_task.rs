//! A field report of 2026-10-15 from a chief on Windows (batch B, item B3): **`cf
//! tell` refused a task still queued.** What the chief saw: it gave a task, and
//! before the worker's window was up it told the task more; ConsensFlow answered
//! "T-n has no window to tell: it is queued", and the words went nowhere. The
//! report is the brain's `issues/field-reports-2026-10-15.md`.
//!
//! What should have happened: the words wait for the window. It gets the brief,
//! then the words, once each; nothing is paused, and the answer to the words is
//! the task's result, the one result, pasted into the chief's window once.
//!
//! The case is that shape on the rig, with the chief's own `cf` in its window:
//! the worker's window comes up late (`CF_TEST_SLOW_START`), as a harness does on a
//! loaded machine; the chief gives T-1, and tells it once the launch has begun
//! with the brief, while the window is not up yet.

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::resume_of_a_chief_with_no_turn::after;
use crate::{config, secs, Outcome};

/// How late the worker's windows come up, in milliseconds: long enough for the
/// chief to tell T-1 while its window is not up.
const LATE_MS: &str = "8000";

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

/// The messages of T-1's thread of `kind`.
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

/// How many of the messages a window was given, in `record` (its stand-in's
/// record, a JSON record to a line), hold `words`: what its agent wrote is not
/// counted, though it may quote them.
fn given(record: &str, words: &str) -> usize {
    record
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["type"] == "user")
        .filter(|record| {
            record["message"]["content"]
                .as_str()
                .is_some_and(|text| text.contains(words))
        })
        .count()
}

#[test]
fn words_told_to_a_task_whose_window_is_not_up_go_in_after_its_brief_once_each() -> Outcome {
    let rig = Rig::start(config().var("CF_TEST_SLOW_START", format!("worker {LATE_MS}")))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    cf(
        &rig,
        &chief,
        &["task", "add", "--tier", tier, "Reply with exactly: BRIEF"],
    )?;
    rig.wait_for("T-1's launch to begin with its brief", secs(30), || {
        Ok(thread(&project, "task")?
            .first()
            .is_some_and(|brief| brief["state"] == "delivering"))
    })?;
    assert_eq!(project.task(1)?["state"], "queued", "its window is not up");

    // What 3.0.5 refused.
    let said = cf(&rig, &chief, &["tell", "T-1", "Reply with exactly: TOLD"])?;
    assert!(
        said.starts_with("T-1 is queued: m-") && said.contains("goes in after its brief"),
        "{said}"
    );
    assert_eq!(project.task(1)?["state"], "queued", "nothing is paused");

    rig.wait_for("T-1's result to reach the chief", secs(120), || {
        Ok(thread(&project, "result")?
            .first()
            .is_some_and(|result| result["state"] == "delivered"))
    })?;
    let results = thread(&project, "result")?;
    assert_eq!(results.len(), 1, "one result: {results:?}");
    assert_eq!(
        results[0]["body"],
        json!("TOLD"),
        "the answer to the words is the result"
    );
    let chiefs = after(&chief, "--session-id").ok_or("the chief's window has no session")?;
    let header = format!(
        "[ConsensFlow m-{} ",
        results[0]["id"].as_i64().unwrap_or_default()
    );
    assert_eq!(
        given(&rig.transcript(chiefs), &header),
        1,
        "pasted into the chief's window once"
    );
    let told = thread(&project, "note")?;
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(
        [&told[0]["state"], &told[0]["sender"]],
        [&json!("delivered"), &json!("chief")]
    );

    // The window had the brief, then the words, once each.
    let pane = format!(
        "p{}-{}",
        project.id(),
        project.task(1)?["assignee"].as_str().unwrap_or_default()
    );
    let windows: Vec<Value> = rig
        .open_frames()
        .into_iter()
        .filter(|frame| frame["id"] == pane.as_str())
        .collect();
    assert_eq!(windows.len(), 1, "one window: {windows:?}");
    let session = after(&windows[0], "--session-id").ok_or("the window has no session")?;
    let record = rig.transcript(session);
    let (brief, words) = ("Reply with exactly: BRIEF", "Reply with exactly: TOLD");
    assert_eq!(given(&record, brief), 1, "{record}");
    assert_eq!(given(&record, words), 1, "{record}");
    assert!(
        record.find(brief) < record.find(words),
        "the brief first: {record}"
    );
    rig.close()?;
    Ok(())
}
