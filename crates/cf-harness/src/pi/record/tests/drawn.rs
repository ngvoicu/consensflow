//! What the history draws of a Pi session besides its items' words (chief-harness CHF-15):
//! a step's thinking beside the items, before its words; a call's header, how it ended,
//! how long it took and an edit's change, on its result's item; a compaction beside the
//! items. None of it moves the reading.

use super::*;
use crate::shared::record::drawn::{Kind, Status, Timeline};

/// A record of `fields` written at `at`.
fn written(mut record: Value, at: &str) -> Value {
    record["timestamp"] = json!(at);
    record
}

/// A step that thought, said a line and called `name` with `arguments`, written at 18:00:01.
fn thought_and_called(name: &str, arguments: Value) -> Value {
    message(
        "a1",
        json!({ "role": "assistant", "stopReason": "toolUse", "timestamp": 1_790_000_001_000_i64,
            "content": [
                { "type": "thinking", "thinking": "Read it first." },
                { "type": "text", "text": "Reading." },
                { "type": "toolCall", "id": "call-1", "name": name, "arguments": arguments },
            ] }),
    )
}

/// The result of `call-1`, an error where `failed`, with `fields` besides.
fn answered(failed: bool, fields: Value) -> Value {
    let mut said = json!({ "role": "toolResult", "toolCallId": "call-1", "toolName": "bash",
        "content": [{ "type": "text", "text": "ls: no-such-folder: No such file or directory" }],
        "isError": failed, "timestamp": 1_790_000_003_500_i64 });
    if let (Value::Object(said), Value::Object(fields)) = (&mut said, fields) {
        said.extend(fields);
    }
    written(message("r1", said), "2026-08-24T18:00:03.500Z")
}

fn timeline(record: &Record) -> Vec<(String, Kind, bool)> {
    Timeline::of(record)
        .from(0)
        .map(|(item, beside)| (item.id.to_string(), item.kind(), beside))
        .collect()
}

#[test]
fn a_steps_thinking_stands_beside_the_items_before_its_words() {
    let records = [
        header(),
        user("u1", "go"),
        thought_and_called("bash", json!({ "command": "ls no-such-folder" })),
        answered(true, json!({})),
    ];
    let record = Stage::with(&records).at(0);
    assert_eq!(
        timeline(&record),
        [
            ("u1".to_owned(), Kind::Prompt, false),
            ("a1:thinking:0".to_owned(), Kind::Thinking, true),
            ("a1".to_owned(), Kind::Text, false),
            ("r1".to_owned(), Kind::Tool, false),
        ]
    );
    assert_eq!(&*record.beside[0].1.text, "Read it first.");
    assert_eq!(record.beside[0].1.role, Role::Custom);
    // The step's words are its text alone, as they were.
    assert_eq!(&*record.items[1].text, "Reading.");
}

#[test]
fn a_result_says_its_calls_header_how_it_ended_and_how_long_it_took() {
    let records = [
        header(),
        user("u1", "go"),
        thought_and_called("bash", json!({ "command": "ls no-such-folder" })),
        answered(true, json!({})),
    ];
    let record = Stage::with(&records).at(0);
    let detail = record.items[2].detail.as_deref().unwrap();
    assert_eq!(detail.name.as_deref(), Some("bash"));
    assert_eq!(detail.header.as_deref(), Some("$ ls no-such-folder"));
    assert_eq!(detail.status, Some(Status::Failed));
    // From the step's time to the result's.
    assert_eq!(detail.took_ms, Some(2_500));
    // What Pi says of the time wins.
    let timed = [
        header(),
        user("u1", "go"),
        thought_and_called("bash", json!({ "command": "ls" })),
        answered(false, json!({ "durationMs": 12 })),
    ];
    let detail = Stage::with(&timed).at(0).items[2].detail.clone().unwrap();
    assert_eq!(
        (detail.status, detail.took_ms),
        (Some(Status::Ok), Some(12))
    );
}

#[test]
fn a_read_and_an_edit_are_headed_by_their_file_and_an_edit_says_its_change() {
    for (name, arguments, headed) in [
        (
            "read",
            json!({ "path": "notes.txt", "offset": 2, "limit": 3 }),
            "read notes.txt:2-4",
        ),
        (
            "edit",
            json!({ "path": "notes.txt", "oldText": "red", "newText": "blue" }),
            "edit notes.txt",
        ),
        (
            "write",
            json!({ "path": "b.md", "content": "x" }),
            "write b.md",
        ),
        ("grep", json!({ "pattern": "fn main" }), "grep fn main"),
    ] {
        let records = [
            header(),
            user("u1", "go"),
            thought_and_called(name, arguments),
            answered(
                false,
                json!({ "details": { "diff": "-1 The light is red.\n+1 The light is blue." } }),
            ),
        ];
        let detail = Stage::with(&records).at(0).items[2].detail.clone().unwrap();
        assert_eq!(detail.header.as_deref(), Some(headed), "{name}");
        assert_eq!(
            detail.diff.as_deref(),
            Some("-1 The light is red.\n+1 The light is blue.")
        );
    }
}

#[test]
fn a_compaction_stands_beside_the_items_with_its_tokens_and_summary() {
    let records = [
        header(),
        user("u1", "go"),
        assistant("a1", "stop", "done"),
        json!({ "type": "compaction", "id": "c1", "parentId": "a1",
            "timestamp": "2026-08-24T18:00:05.000Z", "summary": "The human asked; it is done.",
            "firstKeptEntryId": "a1", "tokensBefore": 14_321 }),
    ];
    let record = Stage::with(&records).at(0);
    let (place, compacted) = record.beside.last().unwrap();
    assert_eq!(*place, record.items.len());
    assert!(compacted.compaction);
    assert_eq!(compacted.kind(), Kind::Compaction);
    assert_eq!(&*compacted.text, "The human asked; it is done.");
    assert_eq!(compacted.detail.as_deref().unwrap().tokens, Some(14_321));
    // No item of the reading says it: its last word is the answer, as it was.
    assert_eq!(&*record.items.last().unwrap().id, "a1");
    assert_eq!(
        serde_json::to_value(&record).unwrap(),
        serde_json::to_value(Stage::with(&records[..3]).at(0)).unwrap()
    );
}
