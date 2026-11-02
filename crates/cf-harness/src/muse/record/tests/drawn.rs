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
