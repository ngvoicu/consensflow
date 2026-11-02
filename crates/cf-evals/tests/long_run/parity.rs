//! The long run's watch reads Claude Code's record with a reader of its own
//! (`src/long_run/record.rs`), on purpose: it holds the daemon to a reading that is not
//! the daemon's, and this crate depends on no crate of the product. A reader of its own
//! is a copy of the product's rules, and every change of the product's can leave the copy
//! behind: T-483 (a cancelled `/compact` read at rest, and `/mcp` and `/plugin` written
//! whole), T-550 (the synthetic items), and a successful `TaskStop` the product reads and
//! the watch did not (T-1069's four false breaks).
//!
//! This is how the copy is held to the product's reader. Every record the product keeps as
//! a fixture of Claude Code's (`crates/cf-harness/tests/fixtures/claude`), and the shapes
//! made here for the rules those do not hold, is read by both after each of its lines:
//! the product's reader through `harness-ask` (`claude_prefixes`, which reads the file the
//! way the dispatcher does), the watch's [`Record`] over the same lines. What a turn is
//! doing (at work, at rest, or nothing said), how many jobs it left running and whether
//! background agents run must be the same after every line. A disagreement is a failure
//! that names the first line it began at.

use std::path::Path;

use cf_evals::long_run::record::{Record, Stood};
use serde_json::{json, Value};

/// The session every fixture's `$SESSION` becomes.
const SESSION: &str = "s1";

/// What a reader says of a transcript after some of its lines: whether a turn is at work
/// (none where nothing says), how many jobs run, and whether agents do.
#[derive(Debug, PartialEq, Eq)]
struct Said {
    at_work: Option<bool>,
    jobs: usize,
    agents: bool,
}

/// What the product's reading `reading` says.
fn product(reading: &Value) -> Said {
    Said {
        at_work: match reading["settlement"]["state"].as_str() {
            Some("in-flight") => Some(true),
            Some("settled") => Some(false),
            _ => None,
        },
        jobs: usize::try_from(reading["jobsPending"].as_u64().unwrap_or(0)).unwrap(),
        agents: reading["agentsPending"].as_bool().unwrap_or(false)
            || reading["agentsPending"]
                .as_u64()
                .is_some_and(|count| count > 0),
    }
}

/// What the watch's record says.
fn watch(record: &Record) -> Said {
    Said {
        at_work: record
            .stood_at(i64::MAX)
            .map(|(stood, _)| stood == Stood::InFlight),
        jobs: record.jobs_pending_at(i64::MAX).len(),
        agents: record.agents_pending_at(i64::MAX) > 0,
    }
}

/// The product's reading after each prefix of `text`, by the number of lines.
fn product_readings(text: &str) -> Vec<Value> {
    let answered = cf_e2e::live::ask::question(&json!({
        "op": "claude_prefixes", "session": SESSION, "text": text,
    }))
    .unwrap();
    answered
        .as_array()
        .unwrap()
        .iter()
        .map(|prefix| prefix["reading"].clone())
        .collect()
}

/// The line `at` (1-based) of `text`, as the kind of record it is.
fn kind_of(text: &str, at: usize) -> String {
    let line: Value =
        serde_json::from_str(text.lines().nth(at - 1).unwrap_or("{}")).unwrap_or_default();
    let name = |field: &str| line[field].as_str().unwrap_or("").to_owned();
    match line["type"].as_str() {
        Some("system") => format!("system {}", name("subtype")),
        Some("queue-operation") => format!("queue-operation {}", name("operation")),
        Some("attachment") => format!(
            "attachment {}",
            line["attachment"]["type"].as_str().unwrap_or("")
        ),
        Some(other) => other.to_owned(),
        None => "no type".to_owned(),
    }
}

/// Where the two readers disagree about `text`, one line of words for each run of
/// prefixes they disagree over: the first line the disagreement began at and what each says.
fn disagreements(name: &str, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let readings = product_readings(text);
    assert_eq!(
        readings.len(),
        lines.len(),
        "{name}: a reading for each line"
    );
    let mut found = Vec::new();
    let mut running: Option<(usize, Said, Said)> = None;
    for (index, reading) in readings.iter().enumerate() {
        let at = index + 1;
        let (ours, theirs) = (
            watch(&Record::claude(&lines[..at].join("\n"))),
            product(reading),
        );
        match (&running, ours == theirs) {
            (None, false) => running = Some((at, ours, theirs)),
            (Some(_), true) | (None, true) => {
                if let Some((from, ours, theirs)) = running.take() {
                    found.push(format!(
                        "{name}: from line {from} ({}) to line {}: the watch reads {ours:?}, the product {theirs:?}",
                        kind_of(text, from),
                        at - 1
                    ));
                }
            }
            (Some(_), false) => {}
        }
    }
    if let Some((from, ours, theirs)) = running {
        found.push(format!(
            "{name}: from line {from} ({}) to the end: the watch reads {ours:?}, the product {theirs:?}",
            kind_of(text, from)
        ));
    }
    found
}

/// The transcripts the product keeps as fixtures of Claude Code's, by name.
fn fixtures() -> Vec<(String, String)> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../cf-harness/tests/fixtures/claude");
    let mut found: Vec<(String, String)> = std::fs::read_dir(&folder)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read_to_string(entry.path())
                    .unwrap()
                    .replace("$SESSION", SESSION),
            )
        })
        .collect();
    found.sort();
    assert!(found.len() >= 10, "the fixtures are found: {found:?}");
    found
}

#[test]
fn the_watch_reads_every_fixture_of_the_products_as_the_product_does_after_every_line() {
    let found: Vec<String> = fixtures()
        .iter()
        .flat_map(|(name, text)| disagreements(name, text))
        .collect();
    assert!(found.is_empty(), "\n{}", found.join("\n"));
}

// ---- the rules the product's fixtures do not hold ------------------------------------------
//
// Each shape below is a transcript made here, as Claude Code writes its records, and read by
// both readers after every line.

/// A line of the record at `second` past 10:00 of 2026-10-15, of the session's main
/// conversation, with `fields`.
fn rec(second: u32, fields: Value) -> String {
    let mut line = json!({
        "timestamp": format!("2026-10-15T10:{:02}:{:02}.000Z", second / 60, second % 60),
        "sessionId": SESSION,
        "isSidechain": false,
        "uuid": format!("u-{second}"),
    });
    for (name, value) in fields.as_object().into_iter().flatten() {
        line[name] = value.clone();
    }
    line.to_string()
}

fn typed(second: u32, parent: Option<u32>, text: &str) -> String {
    rec(
        second,
        json!({ "type": "user", "parentUuid": parent.map(|parent| format!("u-{parent}")),
                "message": { "role": "user", "content": text } }),
    )
}

fn says(second: u32, parent: u32, text: &str, stop: &str) -> String {
    rec(
        second,
        json!({ "type": "assistant", "parentUuid": format!("u-{parent}"),
                "message": { "id": format!("m-{second}"), "role": "assistant", "stop_reason": stop,
                             "content": [{ "type": "text", "text": text }] } }),
    )
}

fn calls(second: u32, parent: u32, id: &str, name: &str, input: Value) -> String {
    rec(
        second,
        json!({ "type": "assistant", "parentUuid": format!("u-{parent}"),
                "message": { "id": format!("m-{second}"), "role": "assistant", "stop_reason": "tool_use",
                             "content": [{ "type": "tool_use", "id": id, "name": name, "input": input }] } }),
    )
}

fn result(second: u32, parent: u32, call: &str, said: &str, tool_use_result: Value) -> String {
    rec(
        second,
        json!({ "type": "user", "parentUuid": format!("u-{parent}"), "toolUseResult": tool_use_result,
                "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": call,
                                                           "content": said }] } }),
    )
}

fn took(second: u32, parent: u32, nothing: &str) -> String {
    rec(
        second,
        json!({ "type": "system", "subtype": "turn_duration", "parentUuid": format!("u-{parent}"),
                "durationMs": 1000, "messageCount": 4, "note": nothing }),
    )
}

/// Claude Code's own notification of a job, as its record of the turn it begins by itself.
fn notified(second: u32, parent: u32, text: &str) -> String {
    rec(
        second,
        json!({ "type": "user", "parentUuid": format!("u-{parent}"), "promptSource": "system",
                "turnOrigin": "task_notification", "origin": { "kind": "task-notification" },
                "message": { "role": "user", "content": text } }),
    )
}

fn notice(id: &str, status: Option<&str>, body: &str) -> String {
    let status = status.map_or_else(String::new, |status| format!("<status>{status}</status>\n"));
    format!("<task-notification>\n<task-id>{id}</task-id>\n<tool-use-id>t</tool-use-id>\n{status}<summary>s</summary>\n</task-notification>{body}")
}

fn queue(second: u32, operation: &str, content: &str) -> String {
    rec(
        second,
        json!({ "type": "queue-operation", "operation": operation, "content": content }),
    )
}

/// Both readers over `lines` (a prompt first, as every turn begins): the disagreements.
fn over(name: &str, lines: &[String]) -> Vec<String> {
    disagreements(name, &lines.join("\n"))
}

/// A turn that started the background job `b1` with a command, at second 2.
fn started() -> Vec<String> {
    vec![
        typed(0, None, "Run it."),
        calls(
            1,
            0,
            "c1",
            "Bash",
            json!({ "command": "sleep 90", "run_in_background": true }),
        ),
        result(
            2,
            1,
            "c1",
            "Command running in background with ID: b1",
            json!({ "backgroundTaskId": "b1" }),
        ),
        says(3, 2, "Running.", "end_turn"),
        took(4, 3, ""),
    ]
}

#[test]
fn a_job_stopped_by_taskstop_is_over_and_what_only_looks_like_a_stop_is_none() {
    let mut stopped = started();
    stopped.extend([
        typed(10, None, "Stop it."),
        calls(11, 10, "c2", "TaskStop", json!({ "task_id": "b1" })),
        result(
            12,
            11,
            "c2",
            "Successfully stopped task: b1",
            json!({ "task_id": "b1" }),
        ),
        says(13, 12, "Stopped.", "end_turn"),
        took(14, 13, ""),
    ]);
    let mut failed = started();
    failed.extend([
        typed(10, None, "Stop it."),
        calls(11, 10, "c2", "TaskStop", json!({ "task_id": "b1" })),
        rec(
            12,
            json!({ "type": "user", "parentUuid": "u-11", "toolUseResult": { "task_id": "b1" },
            "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "c2",
            "is_error": true, "content": "Task b1 is not running" }] } }),
        ),
    ]);
    let mut another_call = started();
    another_call.extend([
        calls(11, 4, "c2", "Bash", json!({ "command": "true" })),
        result(
            12,
            11,
            "c2",
            "Successfully stopped task: b1",
            json!({ "task_id": "b1" }),
        ),
    ]);
    let mut two_results = started();
    two_results.extend([
        calls(11, 4, "c2", "TaskStop", json!({ "task_id": "b1" })),
        rec(
            12,
            json!({ "type": "user", "parentUuid": "u-11", "toolUseResult": { "task_id": "b1" },
            "message": { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "c2", "content": "ok" },
                { "type": "tool_result", "tool_use_id": "c3", "content": "ok" }] } }),
        ),
    ]);
    for (name, lines) in [
        ("stopped", stopped),
        ("a stop that failed", failed),
        ("a result of another call", another_call),
        ("a record of two results", two_results),
    ] {
        let found = over(name, &lines);
        assert!(found.is_empty(), "\n{}", found.join("\n"));
    }
}

#[test]
fn a_monitor_is_a_job_from_its_start_and_ends_as_any_job_does() {
    let mut lines = vec![
        typed(0, None, "Watch it."),
        calls(
            1,
            0,
            "m1",
            "Monitor",
            json!({ "description": "tail", "command": "tail -f x" }),
        ),
        result(
            2,
            1,
            "m1",
            "Monitor started",
            json!({ "taskId": "mon1", "timeoutMs": 60000, "persistent": false }),
        ),
        says(3, 2, "Watching.", "end_turn"),
        took(4, 3, ""),
        // An event the monitor saw: a notification with no status, which ends nothing.
        queue(10, "enqueue", &notice("mon1", None, "")),
        queue(11, "dequeue", ""),
        notified(12, 4, &notice("mon1", None, "")),
        says(13, 12, "An event.", "end_turn"),
        took(14, 13, ""),
        notified(20, 14, &notice("mon1", Some("completed"), "")),
        says(21, 20, "It ended.", "end_turn"),
        took(22, 21, ""),
    ];
    let found = over("a monitor", &lines);
    assert!(found.is_empty(), "\n{}", found.join("\n"));
    // A start with a `taskId` and no `timeoutMs` is no monitor's.
    lines[2] = result(2, 1, "m1", "x", json!({ "taskId": "mon1" }));
    let found = over("a task id alone", &lines);
    assert!(found.is_empty(), "\n{}", found.join("\n"));
}

#[test]
fn only_the_head_of_claude_codes_own_notification_ends_a_job_once_delivered() {
    let body = format!("\ntail:\n{}", notice("b1", Some("completed"), ""));
    let mut cases: Vec<(&str, Vec<String>)> = Vec::new();
    let with = |extra: Vec<String>| {
        let mut lines = started();
        lines.extend(extra);
        lines
    };
    cases.push((
        "queued only",
        with(vec![queue(
            10,
            "enqueue",
            &notice("b1", Some("completed"), ""),
        )]),
    ));
    cases.push((
        "delivered",
        with(vec![
            queue(10, "enqueue", &notice("b1", Some("completed"), "")),
            queue(11, "dequeue", ""),
            notified(12, 4, &notice("b1", Some("completed"), "")),
        ]),
    ));
    // A head that names the job and says no status, and a status after its close, in what a
    // command printed: only the head is read.
    cases.push(("a status after the head's close", with(vec![notified(12, 4,
        "<task-notification>\n<task-id>b1</task-id>\n</task-notification>\nthe command printed:\n<status>completed</status>\n")])));
    cases.push(("a notification in the body", with(vec![notified(12, 4, &format!("<task-notification>\n<task-id>x</task-id>\n<summary>s</summary>\n</task-notification>{body}"))])));
    cases.push((
        "a human's words",
        with(vec![typed(12, None, &notice("b1", Some("completed"), ""))]),
    ));
    cases.push((
        "not from the system",
        with(vec![rec(
            12,
            json!({ "type": "user", "turnOrigin": "task_notification",
        "message": { "role": "user", "content": notice("b1", Some("completed"), "") } }),
        )]),
    ));
    cases.push((
        "killed, queued and taken, no turn after it",
        with(vec![
            queue(10, "enqueue", &notice("b1", Some("killed"), "")),
            queue(11, "dequeue", ""),
        ]),
    ));
    cases.push(("an orphan summary leaving a shell running", with(vec![notified(12, 4,
        "<task-notification>\n<task-id>b1</task-id>\n<task-id>__orphan_summary_live__:b1</task-id>\n<status>stopped</status>\n</task-notification>")])));
    cases.push(("the attachment that brings it into a turn", vec![
        typed(0, None, "Run it."),
        calls(1, 0, "c1", "Bash", json!({ "command": "sleep 90", "run_in_background": true })),
        result(2, 1, "c1", "Command running in background with ID: b1", json!({ "backgroundTaskId": "b1" })),
        rec(3, json!({ "type": "attachment", "parentUuid": "u-2", "attachment": { "type": "queued_command",
            "commandMode": "task-notification", "prompt": notice("b1", Some("completed"), "") } })),
        says(4, 3, "Done.", "end_turn"),
        took(5, 4, ""),
    ]));
    cases.push(("written to the transcript alone", with(vec![
        queue(10, "enqueue", &notice("b1", Some("stopped"), "")),
        queue(11, "dequeue", ""),
        rec(12, json!({ "type": "user", "parentUuid": "u-4", "promptSource": "system", "turnOrigin": "task_notification",
            "origin": { "kind": "task-notification" }, "queueTranscriptOnly": true,
            "message": { "role": "user", "content": notice("b1", Some("stopped"), "") } })),
    ])));
    let found: Vec<String> = cases
        .iter()
        .flat_map(|(name, lines)| over(name, lines))
        .collect();
    assert!(found.is_empty(), "\n{}", found.join("\n"));
}

#[test]
fn what_opens_a_turn_and_what_only_goes_into_the_record_is_read_as_the_product_reads_it() {
    let mut cases: Vec<(&str, Vec<String>)> = Vec::new();
    // The agent's thinking alone is the agent at work: a turn that ended and the next record
    // of the agent's a thought and nothing more.
    cases.push(("thinking alone", vec![
        typed(0, None, "Think."),
        says(1, 0, "Thought.", "end_turn"),
        took(2, 1, ""),
        rec(3, json!({ "type": "assistant", "parentUuid": "u-2", "message": { "id": "m-3", "role": "assistant",
            "stop_reason": null, "content": [{ "type": "thinking", "thinking": "hm" }] } })),
    ]));
    // An interrupt is a list of one text block; the same words typed are a prompt.
    for (name, text) in [
        (
            "an interrupt",
            json!([{ "type": "text", "text": "[Request interrupted by user]" }]),
        ),
        ("a typed interrupt", json!("[Request interrupted by user]")),
        (
            "an interrupt for a tool",
            json!([{ "type": "text", "text": "[Request interrupted by user for tool use]" }]),
        ),
        (
            "an interrupt with more",
            json!([{ "type": "text", "text": "[Request interrupted by user] and" }]),
        ),
    ] {
        cases.push((name, vec![
            typed(0, None, "Work."),
            rec(1, json!({ "type": "user", "parentUuid": "u-0", "message": { "role": "user", "content": text } })),
        ]));
    }
    // A human's message taken into a turn under way begins none and ends none.
    cases.push(("a message absorbed into a turn", vec![
        typed(0, None, "Work."),
        calls(1, 0, "c1", "Bash", json!({ "command": "true" })),
        rec(2, json!({ "type": "attachment", "parentUuid": "u-1", "attachment": { "type": "queued_command",
            "commandMode": "prompt", "prompt": "And another thing", "origin": { "kind": "human" } } })),
        result(3, 1, "c1", "ok", json!({})),
        says(4, 3, "Done.", "end_turn"),
        took(5, 4, ""),
    ]));
    // A turn's companion (the sources of the images it attached) is no prompt.
    cases.push((
        "a companion",
        vec![
            typed(0, None, "Look."),
            says(1, 0, "Seen.", "end_turn"),
            took(2, 1, ""),
            rec(
                3,
                json!({ "type": "user", "parentUuid": "u-1", "turnCompanion": true,
            "message": { "role": "user", "content": "[Image: source: /tmp/a.png]" } }),
            ),
        ],
    ));
    // The agents a turn's boundary counted.
    cases.push(("agents pending", vec![
        typed(0, None, "Go."),
        says(1, 0, "Started them.", "end_turn"),
        rec(2, json!({ "type": "system", "subtype": "turn_duration", "parentUuid": "u-1", "durationMs": 5,
            "messageCount": 2, "pendingBackgroundAgentCount": 2 })),
        typed(10, None, "More."),
        says(11, 10, "Done.", "end_turn"),
        took(12, 11, ""),
    ]));
    // A stop hook's summary ends a turn that answered, unless it prevented continuation.
    for (name, prevented) in [("a stop hook", false), ("a stop hook that prevented", true)] {
        cases.push((
            name,
            vec![
                typed(0, None, "Go."),
                says(1, 0, "Done.", "end_turn"),
                rec(
                    2,
                    json!({ "type": "system", "subtype": "stop_hook_summary", "parentUuid": "u-1",
                "preventedContinuation": prevented }),
                ),
            ],
        ));
    }
    // The close of a turn Claude Code found cut is no model's answer.
    cases.push(("a cut turn closed by Claude Code", vec![
        typed(0, None, "Go."),
        rec(1, json!({ "type": "assistant", "parentUuid": "u-0", "message": { "id": "m-1", "role": "assistant",
            "model": "<synthetic>", "stop_reason": "stop_sequence",
            "content": [{ "type": "text", "text": "No response requested." }] } })),
        took(2, 1, ""),
    ]));
    let found: Vec<String> = cases
        .iter()
        .flat_map(|(name, lines)| over(name, lines))
        .collect();
    assert!(found.is_empty(), "\n{}", found.join("\n"));
}

#[test]
fn a_compaction_is_cancelled_only_by_the_words_before_a_command_beside_the_bare_compact() {
    let caveat = |second: u32, parent: u32, meta: bool| {
        rec(
            second,
            json!({ "type": "user", "parentUuid": format!("u-{parent}"), "isMeta": meta,
                    "message": { "role": "user", "content": "<local-command-caveat>Run by the user.</local-command-caveat>" } }),
        )
    };
    let base = |compact: &str| {
        vec![
            typed(0, None, "Hi."),
            says(1, 0, "Hello.", "end_turn"),
            took(2, 1, ""),
            typed(10, Some(2), compact),
        ]
    };
    let with = |compact: &str, extra: Vec<String>| {
        let mut lines = base(compact);
        lines.extend(extra);
        lines
    };
    let cases = [
        ("cancelled", with("/compact", vec![caveat(11, 2, true)])),
        (
            "cancelled with instructions",
            with("/compact keep the plan", vec![caveat(11, 2, true)]),
        ),
        (
            "a caveat that is no sibling",
            with("/compact", vec![caveat(11, 10, true)]),
        ),
        (
            "a caveat that is no meta record",
            with("/compact", vec![caveat(11, 2, false)]),
        ),
        (
            "something between them",
            with(
                "/compact",
                vec![says(11, 10, "Ok.", "end_turn"), caveat(12, 2, true)],
            ),
        ),
        (
            "a command that is no compact",
            with("/compacted", vec![caveat(11, 2, true)]),
        ),
        (
            "compacting",
            with(
                "/compact",
                vec![
                    rec(
                        11,
                        json!({ "type": "system", "subtype": "compact_boundary", "parentUuid": null }),
                    ),
                    rec(
                        12,
                        json!({ "type": "user", "parentUuid": "u-11", "isCompactSummary": true,
                "message": { "role": "user", "content": "This session is being continued." } }),
                    ),
                    caveat(13, 12, true),
                ],
            ),
        ),
    ];
    let found: Vec<String> = cases
        .iter()
        .flat_map(|(name, lines)| over(name, lines))
        .collect();
    assert!(found.is_empty(), "\n{}", found.join("\n"));
}

#[test]
fn a_line_that_is_no_json_in_the_middle_of_a_record_is_passed_over_by_both() {
    let lines = vec![
        typed(0, None, "Work."),
        "{\"type\":\"user\",\"message\":".to_owned(),
        says(2, 0, "Done.", "end_turn"),
        took(3, 2, ""),
    ];
    let found = over("a torn line", &lines);
    assert!(found.is_empty(), "\n{}", found.join("\n"));
}
