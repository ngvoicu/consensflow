//! What the history draws of Devin's chain besides its items' words (chief-harness
//! CHF-15): an assistant's thinking beside the items, before its message; a tool's answer
//! headed by the call it answers (Devin's store says no call's end or time: an honest
//! gap, none said); and the summary its compactor begins a chain with, a compaction's.
//! None of it moves the reading.

use super::*;
use crate::shared::record::drawn::{Kind, Timeline};
use crate::shared::record::reading::{Record, Role};

fn record(staged: &Staged) -> Record {
    match &*look(&mut staged.reader()) {
        Reading::Known(record) => record.clone(),
        other => panic!("{other:?}"),
    }
}

fn timeline(record: &Record) -> Vec<(String, Kind, bool)> {
    Timeline::of(record)
        .from(0)
        .map(|(item, beside)| (item.id.to_string(), item.kind(), beside))
        .collect()
}

/// A step that thinks and calls `name` with `arguments`, and the call's answer.
fn called(name: &str, arguments: Value) -> [Value; 2] {
    [
        json!({ "message_id": "a-1", "role": "assistant", "content": "",
            "thinking": "The file first.",
            "tool_calls": [{ "id": "call-1", "name": name, "arguments": arguments }] }),
        json!({ "message_id": "t-1", "role": "tool", "content": "out", "tool_call_id": "call-1" }),
    ]
}

#[test]
fn an_assistants_thinking_stands_beside_the_items_before_its_message() {
    let staged = Staged::new();
    let [step, answer] = called("exec", json!({ "command": "ls no-such-folder" }));
    staged.chain(&[user("u-1", None), step, answer, stopped("a-2", "DONE")]);
    let record = record(&staged);
    assert_eq!(
        timeline(&record),
        [
            ("u-1".to_owned(), Kind::Prompt, false),
            ("a-1:thinking".to_owned(), Kind::Thinking, true),
            ("a-1".to_owned(), Kind::Text, false),
            ("t-1".to_owned(), Kind::Tool, false),
            ("a-2".to_owned(), Kind::Text, false),
        ]
    );
    assert_eq!(&*record.beside[0].1.text, "The file first.");
    // The reading is the one with no thinking in the store.
    let plain = Staged::new();
    let [mut step, answer] = called("exec", json!({ "command": "ls no-such-folder" }));
    step.as_object_mut().unwrap().remove("thinking");
    plain.chain(&[user("u-1", None), step, answer, stopped("a-2", "DONE")]);
    assert_eq!(read_once(&staged), read_once(&plain));
}

#[test]
fn a_tools_answer_is_headed_by_the_call_it_answers_and_says_no_end_devin_keeps_none() {
    for (name, arguments, header) in [
        (
            "exec",
            json!({ "command": "ls no-such-folder" }),
            "$ ls no-such-folder",
        ),
        (
            "read",
            json!({ "file_path": "notes.txt" }),
            "read notes.txt",
        ),
        (
            "edit",
            json!("{\"file_path\":\"notes.txt\",\"old_string\":\"red\"}"),
            "edit notes.txt",
        ),
    ] {
        let staged = Staged::new();
        let [step, answer] = called(name, arguments);
        staged.chain(&[user("u-1", None), step, answer]);
        let record = record(&staged);
        let detail = record.items[2].detail.clone().unwrap();
        assert_eq!(detail.kind, Kind::Tool);
        assert_eq!(detail.name.as_deref(), Some(name));
        assert_eq!(detail.header.as_deref(), Some(header), "{name}");
        assert_eq!((detail.status, detail.took_ms), (None, None));
    }
}

#[test]
fn the_summary_its_compactor_begins_a_chain_with_is_a_compaction() {
    let staged = Staged::new();
    staged.chain(&[
        json!({ "message_id": "s-1", "role": "system", "content": "The conversation so far.",
            "metadata": { "extensions": { "devin-rs/summary": { "source": "async_file_compactor" } } } }),
        user("u-1", None),
        stopped("a-1", "DONE"),
    ]);
    let record = record(&staged);
    assert_eq!(record.items[0].kind(), Kind::Compaction);
    // Still the harness's own words to every reader of the conversation's.
    assert_eq!(record.items[0].role, Role::Custom);
    assert!(!record.items[0].compaction);
}
