//! What the history draws of a Muse log besides its items' words (chief-harness CHF-15,
//! provisional until a live run shows Muse's calls whole): a reasoning's words beside the
//! items; a call running beside them until its result, then its result's item headed by
//! it, with how it ended; and a compaction's summary a compaction's. None of it moves the
//! reading.

use super::*;
use crate::shared::record::drawn::{Kind, Status, Timeline};

fn timeline(record: &Record) -> Vec<(String, Kind, bool)> {
    Timeline::of(record)
        .from(0)
        .map(|(item, beside)| (item.id.to_string(), item.kind(), beside))
        .collect()
}

fn records(answered: bool) -> Vec<Value> {
    let mut records = vec![
        intent(1, "i1", "go"),
        run(2, "run-1", json!({ "kind": "started" })),
        run(
            3,
            "run-1",
            json!({ "kind": "reasoning_committed", "message_id": "rm1", "text": "Read first." }),
        ),
        run(
            4,
            "run-1",
            json!({ "kind": "assistant_tool_calls_committed", "tool_calls": [
            { "name": "shell", "args": { "command": "ls no-such-folder" }, "call_id": "call-1" },
        ] }),
        ),
    ];
    if answered {
        records.push(run(5, "run-1", json!({ "kind": "tool_result_batch_committed", "results": [
            { "tool_call_id": "call-1", "text": "ls: no-such-folder: No such file", "is_error": true },
        ] })));
        records.push(run(
            6,
            "run-1",
            json!({ "kind": "assistant_message_committed", "message_id": "m1", "text": "DONE" }),
        ));
        records.push(run(
            7,
            "run-1",
            json!({ "kind": "terminal", "terminal": "completed" }),
        ));
    }
    records
}

#[test]
fn a_reasoning_stands_beside_the_items_and_a_call_runs_beside_them_until_its_result() {
    let running = record_of(&records(false));
    assert_eq!(
        timeline(&running),
        [
            ("i1".to_owned(), Kind::Prompt, false),
            ("rm1:thinking".to_owned(), Kind::Thinking, true),
            ("call-1".to_owned(), Kind::Tool, true),
        ]
    );
    let call = running.beside[1].1.detail.clone().unwrap();
    assert_eq!(call.header.as_deref(), Some("$ ls no-such-folder"));
    assert_eq!(call.status, Some(Status::Running));
    let done = record_of(&records(true));
    assert_eq!(
        timeline(&done),
        [
            ("i1".to_owned(), Kind::Prompt, false),
            ("rm1:thinking".to_owned(), Kind::Thinking, true),
            ("call-1".to_owned(), Kind::Tool, false),
            ("m1".to_owned(), Kind::Text, false),
        ]
    );
    let result = done.items[1].detail.clone().unwrap();
    assert_eq!(result.name.as_deref(), Some("shell"));
    assert_eq!(result.header.as_deref(), Some("$ ls no-such-folder"));
    assert_eq!(result.status, Some(Status::Failed));
}

#[test]
fn what_the_history_alone_reads_moves_no_reading() {
    let with = records(true);
    let without: Vec<Value> = with
        .iter()
        .filter(|record| record["payload"]["event"]["kind"] != "reasoning_committed")
        .cloned()
        .collect();
    assert_eq!(
        serde_json::to_value(record_of(&with)).unwrap(),
        serde_json::to_value(record_of(&without)).unwrap()
    );
}

#[test]
fn a_compactions_summary_is_a_compaction_with_its_tokens_where_muse_says_them() {
    let mut said = records(true);
    said.push(run(
        8,
        "run-1",
        json!({ "kind": "context_compaction_installed", "tokens_before": 9_000,
        "replacement": { "kind": "summary_text", "text": "So far: done." } }),
    ));
    let record = record_of(&said);
    let summary = record.items.last().unwrap();
    assert!(summary.compaction);
    assert_eq!(summary.kind(), Kind::Compaction);
    assert_eq!(summary.detail.as_deref().unwrap().tokens, Some(9_000));
}

#[test]
fn a_commands_result_says_its_exit_and_an_edits_its_change_as_muse_writes_them() {
    // As Muse 1.4.14 wrote them live (T-728): a command's result is JSON text, an edit's
    // names its two sides and its hunk.
    let called =
        |id: &str, name: &str, args: &str| json!({ "name": name, "args": args, "call_id": id });
    let records = vec![
        intent(1, "i1", "go"),
        run(2, "run-1", json!({ "kind": "started" })),
        run(
            3,
            "run-1",
            json!({ "kind": "assistant_tool_calls_committed", "tool_calls": [
            called("c1", "bash", "{\"command\":\"ls no-such-folder\"}"),
            called("c2", "edit_file", "{\"find\":\"red\",\"path\":\"notes.txt\",\"replace\":\"blue\"}"),
        ] }),
        ),
        run(
            4,
            "run-1",
            json!({ "kind": "tool_result_batch_committed", "results": [
            { "tool_call_id": "c1", "text": "{\n  \"command\": \"ls no-such-folder\",\n  \"exit_code\": 1,\n  \"terminal_status\": \"failed\",\n  \"output\": \"ls: no-such-folder\\n\"\n}" },
            { "tool_call_id": "c2", "text": "edited\nchanged lines: line 1\n--- original\n+++ updated\n@@\n-The light is red.\n+The light is blue.\n" },
        ] }),
        ),
    ];
    let record = record_of(&records);
    let command = record.items[1].detail.clone().unwrap();
    assert_eq!(command.header.as_deref(), Some("$ ls no-such-folder"));
    assert_eq!(command.status, Some(Status::Failed));
    assert_eq!(command.diff, None);
    let edit = record.items[2].detail.clone().unwrap();
    assert_eq!(edit.header.as_deref(), Some("edit notes.txt"));
    assert_eq!(edit.status, Some(Status::Ok));
    assert_eq!(
        edit.diff.as_deref(),
        Some("@@\n-The light is red.\n+The light is blue.")
    );
}

/// The live run's session (T-728: Muse Code 1.4.14 on muse-spark-1.3 at minimal, a read, a
/// command that fails, an edit and `/compact`; its reasoning is encrypted, no words), with
/// what its reader reads alone kept and the folders replaced.
#[test]
fn a_live_chiefs_session_reads_as_the_live_check_read_it() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/history-items/muse.jsonl"
    ))
    .unwrap();
    let session = text
        .lines()
        .find_map(|line| {
            let record: Value = serde_json::from_str(line).ok()?;
            record["stream"]["id"].as_str().map(str::to_owned)
        })
        .unwrap();
    let record = known(&fresh_reading(&session, &text)).clone();
    let tools: Vec<(Option<String>, Option<Status>, bool)> = record
        .items
        .iter()
        .filter(|item| item.kind() == Kind::Tool)
        .map(|item| {
            let detail = item.detail.as_deref().unwrap();
            (
                detail.header.as_deref().map(str::to_owned),
                detail.status,
                detail.diff.is_some(),
            )
        })
        .collect();
    assert_eq!(
        tools,
        [
            (Some("read notes.txt".to_owned()), Some(Status::Ok), false),
            (
                Some("$ ls no-such-folder".to_owned()),
                Some(Status::Failed),
                false
            ),
            (Some("edit notes.txt".to_owned()), Some(Status::Ok), true),
        ]
    );
    assert!(record
        .items
        .iter()
        .any(|item| item.kind() == Kind::Compaction));
    // Its reasoning holds no words: no thinking to show.
    assert!(record
        .beside
        .iter()
        .all(|(_, item)| item.kind() != Kind::Thinking));
}

/// What the same session says of its assistant's messages: each is complete, whether a call
/// follows it or not.
#[test]
fn a_live_chiefs_session_leaves_no_assistant_message_unfinished() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/history-items/muse.jsonl"
    ))
    .unwrap();
    let session = text
        .lines()
        .find_map(|line| {
            let record: Value = serde_json::from_str(line).ok()?;
            record["stream"]["id"].as_str().map(str::to_owned)
        })
        .unwrap();
    let record = known(&fresh_reading(&session, &text)).clone();
    let said: Vec<bool> = Timeline::of(&record)
        .from(0)
        .map(|(item, _)| item)
        .filter(|item| item.role == Role::Assistant)
        .map(|item| item.complete)
        .collect();
    assert_eq!(said, [true, true, true, true]);
}
