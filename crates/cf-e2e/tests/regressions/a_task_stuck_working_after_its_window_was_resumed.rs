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
//! T-526 (the consensflow board, 3.0.5 then 3.0.6-alpha.2, Claude Code 2.1.310, 2026-10-25)
//! was that shape with a restart between the close and the reopen. The worker's window
//! failed when the internet went down, and the close ended the landing it ran in the
//! background, whose notification Claude queued and took as its program was ended, never
//! beginning the turn. The app was updated to a new daemon, and the chief's reopen came
//! back on the conversation; the worker's final answer came two and a half hours later,
//! and T-526 showed working for four more, until the chief finished it. The daemon that
//! proved the closed program gone kept the proof in its memory, which went with it: the
//! new one opened the conversation telling Claude's reader nothing, and the taken
//! notification read as a turn still to begin. The rig's case of it is a notification
//! queued at the close, with the daemon started again before the reopen. T-526's own
//! (taken, its turn never begun) holds only where the new process takes each of its own
//! prompts from its queue, as Claude Code did there and the rig's fake does not: it is
//! Claude's reader's test (`claude::record`'s `restart`).
//!
//! The cases are that shape on the rig: the worker answers T-1 and its window closes;
//! its record then ends as the closed process left it; the chief reopens T-1, and the
//! window that comes back answers. T-442's process left a job's notification queued
//! and the turn of another's begun, both written as Claude Code 2.1.302 writes them,
//! and the window that comes back closes the cut turn, as Claude does. One at rest
//! with a notification queued leaves no sign of itself at all: no turn is cut, and
//! ConsensFlow's settings run no hook as Claude resumes.

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

/// A queue operation of the session `session`, at `at`, naming `content` where it
/// names one.
fn operation(session: &str, operation: &str, content: Option<String>, at: &str) -> Value {
    let mut record = json!({
        "type": "queue-operation", "operation": operation, "timestamp": at,
        "sessionId": session,
    });
    if let Some(content) = content {
        record["content"] = json!(content);
    }
    record
}

/// `records` written at the end of the record of `session`, in the Claude
/// configuration `config`.
fn left(config: &str, session: &str, records: &[Value]) -> std::io::Result<()> {
    let file = Path::new(config)
        .join("projects")
        .join("integration")
        .join(format!("{session}.jsonl"));
    let mut record = OpenOptions::new().append(true).open(file)?;
    for line in records {
        writeln!(record, "{line}")?;
    }
    Ok(())
}

/// What the closed process left at the end of the record of `session`, in the Claude
/// configuration `config`, as T-442's did at 18:30:09: the notifications of the two
/// jobs the turn's end ended queued, the first taken and its turn begun, the second
/// still queued when the window closed.
fn closed_in_a_turn_with_a_notification_queued(config: &str, session: &str) -> std::io::Result<()> {
    let begun = json!({
        "type": "user", "uuid": "cut-notification", "parentUuid": null,
        "message": { "role": "user", "content": failed("bmonitor1") },
        "timestamp": "2026-10-17T18:30:09.177Z", "promptSource": "system",
        "origin": { "kind": "task-notification", "producer": "session-task" },
        "turnOrigin": "task_notification", "sessionId": session, "isSidechain": false,
    });
    left(
        config,
        session,
        &[
            operation(
                session,
                "enqueue",
                Some(failed("bmonitor1")),
                "2026-10-17T18:30:09.143Z",
            ),
            operation(session, "dequeue", None, "2026-10-17T18:30:09.172Z"),
            operation(
                session,
                "enqueue",
                Some(failed("bjob00001")),
                "2026-10-17T18:30:09.183Z",
            ),
            begun,
        ],
    )
}

/// What a process that closed at rest left: a job's notification, queued after its
/// turn ended, that it never took.
fn closed_at_rest_with_a_notification_queued(config: &str, session: &str) -> std::io::Result<()> {
    left(
        config,
        session,
        &[operation(
            session,
            "enqueue",
            Some(failed("bjob00001")),
            "2026-10-17T18:30:09.183Z",
        )],
    )
}

/// What a process that closed with a job and a monitor of its turn still running left (T-442
/// with its Monitor, turn-end of 2026-10-21): the turn started a command in the background
/// and armed a monitor on it, as Claude Code 2.1.305 writes them, and ended; the window
/// closed with both, and no word of their end was written. Their notifications went with
/// the process, and the window that comes back waits on neither.
fn closed_with_a_job_and_a_monitor_pending(config: &str, session: &str) -> std::io::Result<()> {
    let at = "2026-10-17T18:29:00.000Z";
    let record = |fields: Value| {
        let mut record = json!({ "sessionId": session, "isSidechain": false, "timestamp": at });
        if let (Some(record), Some(fields)) = (record.as_object_mut(), fields.as_object()) {
            record.extend(fields.clone());
        }
        record
    };
    let call =
        |id: &str, name: &str| json!({ "type": "tool_use", "id": id, "name": name, "input": {} });
    let result = |uuid: &str, call: &str, said: &str, how: Value| {
        record(json!({
            "type": "user", "uuid": uuid, "parentUuid": null,
            "message": { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": call, "content": said },
            ] },
            "toolUseResult": how,
        }))
    };
    left(
        config,
        session,
        &[
            record(json!({
                "type": "user", "uuid": "run-it", "parentUuid": null,
                "message": { "role": "user", "content": "Run the suite and watch it" },
            })),
            record(json!({
                "type": "assistant", "uuid": "starting", "parentUuid": "run-it",
                "message": {
                    "id": "msg-run", "role": "assistant", "stop_reason": "tool_use",
                    "content": [call("toolu-run", "Bash"), call("toolu-watch", "Monitor")],
                },
            })),
            result(
                "started",
                "toolu-run",
                "Command running in background with ID: bsuite001.",
                json!({ "backgroundTaskId": "bsuite001" }),
            ),
            result(
                "armed",
                "toolu-watch",
                "Monitor started (task bwatch001, expires in 30m).",
                json!({ "taskId": "bwatch001", "timeoutMs": 1_800_000, "persistent": false }),
            ),
            record(json!({
                "type": "assistant", "uuid": "waiting", "parentUuid": "armed",
                "message": {
                    "id": "msg-wait", "role": "assistant", "stop_reason": "end_turn",
                    "content": [{ "type": "text", "text": "WAITING" }],
                },
            })),
            record(json!({
                "type": "system", "subtype": "turn_duration", "uuid": "waited",
                "parentUuid": "waiting", "durationMs": 1000, "messageCount": 9,
            })),
        ],
    )
}

#[test]
fn a_claude_workers_answer_to_a_reopen_is_its_result_though_its_window_closed_with_its_job_and_monitor_running(
) -> Outcome {
    let results = reopened_after(closed_with_a_job_and_a_monitor_pending)?;
    assert!(results[1].starts_with("AGAIN_OK\n\n(ConsensFlow: no word came that the 2 background jobs an earlier window of @"), "the result says what it no longer waits on, and that it may run: {results:?}");
    Ok(())
}

#[test]
fn a_claude_workers_answer_to_a_reopen_is_its_result_though_its_window_closed_with_a_notification_queued(
) -> Outcome {
    let results = reopened_after(closed_in_a_turn_with_a_notification_queued)?;
    assert_eq!(results, ["FIRST_OK", "AGAIN_OK"]);
    Ok(())
}

#[test]
fn a_claude_workers_answer_to_a_reopen_is_its_result_though_its_window_closed_at_rest_with_a_notification_queued(
) -> Outcome {
    let results = reopened_after(closed_at_rest_with_a_notification_queued)?;
    assert_eq!(results, ["FIRST_OK", "AGAIN_OK"]);
    Ok(())
}

#[test]
fn a_claude_workers_answer_to_a_reopen_after_a_restart_is_its_result_though_its_window_closed_at_rest_with_a_notification_queued(
) -> Outcome {
    let results = reopened_after_a_restart(closed_at_rest_with_a_notification_queued)?;
    assert_eq!(results, ["FIRST_OK", "AGAIN_OK"]);
    Ok(())
}

/// The worker answers T-1, its window closes, its record then ends as `left` writes it,
/// and the daemon stops and starts again, as an update does, before the chief reopens
/// T-1: the answer to the reopen is T-1's result. The results the chief was given.
fn reopened_after_a_restart(
    left: fn(&str, &str) -> std::io::Result<()>,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut rig = Rig::start(config())?;
    let (id, session) = {
        let project = Project::open(&rig, "chief", json!({}))?;
        let session = answered_and_closed(&rig, &project)?;
        (project.id(), session)
    };
    let claude = rig
        .var("CLAUDE_CONFIG_DIR")
        .ok_or("the rig gives Claude no configuration")?
        .to_owned();
    left(&claude, &session)?;

    // The app is updated: its daemon stops with no window open, and a new one starts.
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(config().existing_root(root))?;
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    if resumed["ok"] != true {
        return Err(Error::Daemon(format!("project.resume: {resumed}")).into());
    }
    let project = Project::new(&rig, id);
    let given = answered_again(&rig, &project, &session, 0)?;
    rig.close()?;
    Ok(given)
}

/// The chief gives the worker T-1, which it answers, and its window closes: the
/// worker's session.
fn answered_and_closed(rig: &Rig, project: &Project) -> Result<String, Box<dyn std::error::Error>> {
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
    assert_eq!(results(project)?, ["FIRST_OK"]);
    let first = workers_windows(rig, project);
    Ok(first
        .first()
        .and_then(|window| after(window, "--session-id"))
        .ok_or("the worker's window has no session")?
        .to_owned())
}

/// The chief reopens T-1, the worker's window comes back on `session` (after the
/// `before` windows of the worker's that this daemon opened) and answers: the results the
/// chief was given, the reopen's last.
fn answered_again(
    rig: &Rig,
    project: &Project,
    session: &str,
    before: usize,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let reopened = rig.cf_in_window(
        &chief,
        ["task", "reopen", "T-1", "Reply with exactly: AGAIN_OK"],
    )?;
    if reopened.code != Some(0) {
        return Err(Error::Daemon(format!("cf task reopen T-1: {reopened}")).into());
    }
    rig.wait_for("the worker's window to come back", secs(60), || {
        Ok(workers_windows(rig, project).len() == before + 1)
    })?;
    let again = workers_windows(rig, project).remove(before);
    assert_eq!(
        resumed_on(&again),
        Some(session),
        "the window came back on its conversation: {:?}",
        again["argv"]
    );
    rig.wait_for("the worker to answer", secs(60), || {
        Ok(rig.transcript(session).contains("\"text\":\"AGAIN_OK\""))
    })?;

    // What the chief should have seen: the answer is T-1's result.
    rig.wait_for("T-1 to be done again", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(results(project)?.len() == 2)
    })?;
    let given = results(project)?;
    assert_eq!(given[0], "FIRST_OK");
    assert!(given[1].starts_with("AGAIN_OK"), "{given:?}");
    Ok(given)
}

/// The worker answers T-1, its window closes, and its record then ends as `left`
/// writes it, before the chief reopens T-1: the answer to the reopen is T-1's
/// result. The results the chief was given.
fn reopened_after(
    left: fn(&str, &str) -> std::io::Result<()>,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let session = answered_and_closed(&rig, &project)?;
    let claude = rig
        .var("CLAUDE_CONFIG_DIR")
        .ok_or("the rig gives Claude no configuration")?
        .to_owned();
    left(&claude, &session)?;
    let before = workers_windows(&rig, &project).len();
    let given = answered_again(&rig, &project, &session, before)?;
    rig.close()?;
    Ok(given)
}
