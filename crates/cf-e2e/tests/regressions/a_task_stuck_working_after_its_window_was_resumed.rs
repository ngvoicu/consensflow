//! A task stuck "working" after its turn ended, its worker's window resumed with a
//! notification still queued.
//!
//! Seen on poker-lab on 2026-10-17, ConsensFlow 3.0.4, Claude Code 2.1.302 (T-442).
//! The worker's turn ended at 18:30 and was taken for T-442's result; its window was
//! closed, as a finished worker's is, while Claude had queued the notification of a
//! job that the end of the turn ended, and was in the turn of another. The chief
//! reopened T-442, and the window came back on its conversation. The reopen's turn
//! ended at 19:25 with the worker's report, and ConsensFlow showed T-442 working for
//! good, never gave the chief the result, and said at `cf task pause` that the window
//! "ignored the interrupt and is still on its earlier turn". Claude's queue is the
//! process's own, in memory: the queued notification went with the closed window,
//! and no record says so, so the record read as a message still to come. The report
//! is `issues/field-reports-2026-10-18-a-task-stuck-working-after-its-turn-ended.md`
//! in consensflow-sme.
//!
//! What the chief should have seen: the reopen's answer is T-1's result.
//!
//! The case is that shape on the rig: the worker answers T-1 and its window closes;
//! its record then ends as the closed process left it (a job's notification queued,
//! and the turn of another's begun, both written as Claude Code 2.1.302 writes them);
//! the chief reopens T-1, and the window that comes back closes the cut turn, as
//! Claude does, and answers.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::resume_of_a_chief_with_no_turn::{after, resumed_on};
use crate::{config, secs, Outcome};

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// The bodies of the results the chief has been given or is to be given, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<String>> {
    let mut results: Vec<(i64, String)> = project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .map(|message| {
            (
                message["id"].as_i64().unwrap_or_default(),
                message["body"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    results.sort();
    Ok(results.into_iter().map(|(_, body)| body).collect())
}

/// The windows of the project's worker the daemon opened so far, oldest first.
fn workers_windows(rig: &Rig, project: &Project) -> Vec<Value> {
    let chief = format!("p{}-chief", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] != chief.as_str())
        .collect()
}

/// A notification of the job `job` that says it `failed`, as Claude Code writes one.
fn failed(job: &str) -> String {
    format!(
        "<task-notification>\n<task-id>{job}</task-id>\n<tool-use-id>toolu_01{job}</tool-use-id>\n<output-file>/tmp/claude/tasks/{job}.output</output-file>\n<status>failed</status>\n<summary>Background command \"{job}\" failed with exit code 143</summary>\n</task-notification>"
    )
}

/// What the closed process left at the end of the record of `session`, in the Claude
/// configuration `config`, as T-442's did at 18:30:09: the notifications of the two
/// jobs the turn's end ended queued, the first taken and its turn begun, the second
/// still queued when the window closed.
fn closed_with_a_notification_queued(config: &str, session: &str) -> std::io::Result<()> {
    let operation = |operation: &str, content: Option<String>, at: &str| {
        let mut record = json!({
            "type": "queue-operation", "operation": operation, "timestamp": at,
            "sessionId": session,
        });
        if let Some(content) = content {
            record["content"] = json!(content);
        }
        record
    };
    let begun = json!({
        "type": "user", "uuid": "cut-notification", "parentUuid": null,
        "message": { "role": "user", "content": failed("bmonitor1") },
        "timestamp": "2026-10-17T18:30:09.177Z", "promptSource": "system",
        "origin": { "kind": "task-notification", "producer": "session-task" },
        "turnOrigin": "task_notification", "sessionId": session, "isSidechain": false,
    });
    let file = Path::new(config)
        .join("projects")
        .join("integration")
        .join(format!("{session}.jsonl"));
    let mut record = OpenOptions::new().append(true).open(file)?;
    for line in [
        operation(
            "enqueue",
            Some(failed("bmonitor1")),
            "2026-10-17T18:30:09.143Z",
        ),
        operation("dequeue", None, "2026-10-17T18:30:09.172Z"),
        operation(
            "enqueue",
            Some(failed("bjob00001")),
            "2026-10-17T18:30:09.183Z",
        ),
        begun,
    ] {
        writeln!(record, "{line}")?;
    }
    Ok(())
}

#[test]
fn a_claude_workers_answer_to_a_reopen_is_its_result_though_its_window_closed_with_a_notification_queued(
) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: FIRST_OK",
            tier(&added)
        ),
    )?;
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the worker's window to close", secs(60), || {
        Ok(project.lane("worker")?["pane"].is_null())
    })?;
    assert_eq!(results(&project)?, ["FIRST_OK"]);
    let first = workers_windows(&rig, &project);
    let session = first
        .first()
        .and_then(|window| after(window, "--session-id"))
        .ok_or("the worker's window has no session")?
        .to_owned();
    let claude = rig
        .var("CLAUDE_CONFIG_DIR")
        .ok_or("the rig gives Claude no configuration")?
        .to_owned();
    closed_with_a_notification_queued(&claude, &session)?;

    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let reopened = rig.cf_in_window(
        &chief,
        ["task", "reopen", "T-1", "Reply with exactly: AGAIN_OK"],
    )?;
    if reopened.code != Some(0) {
        return Err(Error::Daemon(format!("cf task reopen T-1: {reopened}")).into());
    }
    rig.wait_for("the worker's window to come back", secs(60), || {
        Ok(workers_windows(&rig, &project).len() == first.len() + 1)
    })?;
    let again = workers_windows(&rig, &project).remove(first.len());
    assert_eq!(
        resumed_on(&again),
        Some(session.as_str()),
        "the window came back on its conversation: {:?}",
        again["argv"]
    );
    rig.wait_for("the worker to answer", secs(60), || {
        Ok(rig.transcript(&session).contains("\"text\":\"AGAIN_OK\""))
    })?;

    // What the chief should have seen: the answer is T-1's result.
    rig.wait_for("T-1 to be done again", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(results(&project)?.len() == 2)
    })?;
    assert_eq!(results(&project)?, ["FIRST_OK", "AGAIN_OK"]);
    rig.close()?;
    Ok(())
}
