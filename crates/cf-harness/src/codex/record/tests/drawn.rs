//! What the history draws of a rollout besides its items' words (chief-harness CHF-15): a
//! call's header, end and time on its output's item, a patch's change, a reasoning's
//! summary beside the items, a command still running, and a compaction. None of it moves
//! the reading.

use super::*;
use crate::shared::record::drawn::{Kind, Status, Timeline};

fn at(mut record: Value, time: &str) -> Value {
    record["timestamp"] = json!(time);
    record
}

fn timeline(record: &Record) -> Vec<(String, Kind, bool)> {
    Timeline::of(record)
        .from(0)
        .map(|(item, beside)| (item.id.to_string(), item.kind(), beside))
        .collect()
}

/// A turn that reasons, runs `ls no-such-folder` (exit 1), patches notes.txt and answers.
fn turn() -> Vec<Value> {
    vec![
        started(json!("t1")),
        response(json!({ "type": "message", "role": "user", "id": "u1", "content": "go" })),
        response(json!({ "type": "reasoning", "id": "rs1",
            "summary": [{ "type": "summary_text", "text": "**Order** Read, then run." }] })),
        at(
            response(
                json!({ "type": "function_call", "name": "exec_command", "call_id": "c1",
                "arguments": "{\"cmd\":\"ls no-such-folder\"}" }),
            ),
            "2026-11-02T10:00:01.000Z",
        ),
        at(
            response(
                json!({ "type": "function_call_output", "call_id": "c1", "id": "out-c1",
                "output": "Chunk ID: 1\nWall time: 0.0500 seconds\nProcess exited with code 1\nOutput:\nls: no-such-folder: No such file or directory" }),
            ),
            "2026-11-02T10:00:02.000Z",
        ),
        response(
            json!({ "type": "custom_tool_call", "name": "apply_patch", "call_id": "c2",
            "input": "*** Begin Patch\n*** Update File: notes.txt\n@@\n-The light is red.\n+The light is blue.\n*** End Patch" }),
        ),
        response(
            json!({ "type": "custom_tool_call_output", "call_id": "c2", "id": "out-c2",
            "output": "{\"output\":\"Success. Updated the following files:\\nM notes.txt\\n\",\"metadata\":{\"exit_code\":0,\"duration_seconds\":0.0}}" }),
        ),
        response(json!({ "type": "message", "role": "assistant", "id": "m1", "content": "DONE" })),
        event(json!({ "type": "task_complete", "turn_id": "t1", "last_agent_message": "DONE" })),
    ]
}

/// The same turn with nothing the history alone reads: a reasoning that says nothing, in
/// its place (a record's place is its time where it says none).
fn without_reasoning() -> Vec<Value> {
    let mut records = turn();
    records[2] = response(json!({ "type": "reasoning", "id": "rs1", "summary": [] }));
    records
}

#[test]
fn a_reasonings_summary_stands_beside_the_items_and_moves_no_reading() {
    let record = read(&turn()).unwrap();
    assert_eq!(
        timeline(&record),
        [
            ("u1".to_owned(), Kind::Prompt, false),
            ("rs1:thinking".to_owned(), Kind::Thinking, true),
            ("out-c1".to_owned(), Kind::Tool, false),
            ("out-c2".to_owned(), Kind::Tool, false),
            ("m1".to_owned(), Kind::Text, false),
        ]
    );
    assert_eq!(&*record.beside[0].1.text, "**Order** Read, then run.");
    assert_eq!(
        serde_json::to_value(&record).unwrap(),
        serde_json::to_value(read(&without_reasoning()).unwrap()).unwrap()
    );
}

#[test]
fn an_outputs_item_says_its_calls_header_end_and_time_and_a_patch_its_change() {
    let record = read(&turn()).unwrap();
    let failed = record.items[1].detail.clone().unwrap();
    assert_eq!(failed.name.as_deref(), Some("exec_command"));
    assert_eq!(failed.header.as_deref(), Some("$ ls no-such-folder"));
    assert_eq!(
        (failed.status, failed.took_ms),
        (Some(Status::Failed), Some(50))
    );
    let patched = record.items[2].detail.clone().unwrap();
    assert_eq!(patched.header.as_deref(), Some("edit notes.txt"));
    assert_eq!(patched.status, Some(Status::Ok));
    assert_eq!(
        patched.diff.as_deref(),
        Some("@@\n-The light is red.\n+The light is blue.")
    );
}

#[test]
fn an_output_that_says_no_time_took_the_time_between_its_records() {
    let mut records = turn();
    records[4]["payload"]["output"] = json!("Process exited with code 0\nok");
    let detail = read(&records).unwrap().items[1].detail.clone().unwrap();
    assert_eq!(
        (detail.status, detail.took_ms),
        (Some(Status::Ok), Some(1_000))
    );
}

#[test]
fn a_command_codex_started_runs_beside_the_items_until_it_completes() {
    let command = |completed: bool| {
        let mut item =
            json!({ "type": "CommandExecution", "id": "cmd1", "command": "ls no-such-folder" });
        if completed {
            item["exit_code"] = json!(1);
            item["aggregated_output"] = json!("ls: no-such-folder: No such file");
            item["duration_ms"] = json!(30);
        }
        event(
            json!({ "type": if completed { "item_completed" } else { "item_started" }, "turn_id": "t1", "item": item }),
        )
    };
    let mut records = vec![started(json!("t1")), command(false)];
    let running = read(&records).unwrap();
    assert!(running.items.is_empty());
    let (place, call) = &running.beside[0];
    assert_eq!(*place, 0);
    assert_eq!(&*call.id, "cmd1");
    assert_eq!(
        call.detail.as_deref().unwrap().status,
        Some(Status::Running)
    );
    records.push(command(true));
    let done = read(&records).unwrap();
    assert!(done.beside.is_empty());
    let detail = done.items[0].detail.clone().unwrap();
    assert_eq!(detail.header.as_deref(), Some("$ ls no-such-folder"));
    assert_eq!(
        (detail.status, detail.took_ms),
        (Some(Status::Failed), Some(30))
    );
}

#[test]
fn a_compaction_stands_beside_the_items_where_codex_wrote_it() {
    let mut records = turn();
    records.push(
        json!({ "type": "compacted", "timestamp": "2026-11-02T10:01:00.000Z",
        "payload": { "message": "", "replacement_history": [] } }),
    );
    let record = read(&records).unwrap();
    let (place, compacted) = record.beside.last().unwrap();
    assert_eq!(*place, record.items.len());
    assert_eq!(compacted.kind(), Kind::Compaction);
    assert!(compacted.compaction);
    assert_eq!(&*compacted.id, format!("compacted:{}", records.len() - 1));
    assert_eq!(
        serde_json::to_value(&record).unwrap(),
        serde_json::to_value(read(&turn()).unwrap()).unwrap()
    );
}

#[test]
fn an_edit_codex_completed_stands_beside_the_items_with_its_change() {
    let records = vec![
        started(json!("t1")),
        event(json!({ "type": "item_completed", "turn_id": "t1", "item": {
            "type": "FileChange", "id": "exec-1", "status": "completed",
            "stdout": "Success. Updated the following files:\nM /w/notes.txt\n",
            "changes": { "/w/notes.txt": { "type": "update",
                "unified_diff": "@@ -1,2 +1,2 @@\n-The light is red.\n+The light is blue.\n" } } } })),
    ];
    let record = read(&records).unwrap();
    assert!(record.items.is_empty(), "no item of the reading");
    let (_, edit) = &record.beside[0];
    assert_eq!(edit.kind(), Kind::Tool);
    let detail = edit.detail.as_deref().unwrap();
    assert_eq!(detail.header.as_deref(), Some("edit /w/notes.txt"));
    assert_eq!(
        detail.diff.as_deref(),
        Some("@@ -1,2 +1,2 @@\n-The light is red.\n+The light is blue.")
    );
}

/// The live run's rollout (T-728: Codex 0.166.0 on gpt-5.6-luna, its calls made through
/// `exec` scripts, a command that fails, a patch and `/compact`), with what its reader
/// reads alone kept and the folders replaced.
#[test]
fn a_live_chiefs_rollout_reads_as_the_live_check_read_it() {
    let records: Vec<Value> = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/history-items/codex.jsonl"
    ))
    .unwrap()
    .lines()
    .map(|line| serde_json::from_str(line).unwrap())
    .collect();
    let record = read(&records).unwrap();
    let drawn: Vec<(Kind, Option<String>, Option<Status>, bool)> = Timeline::of(&record)
        .from(0)
        .filter(|(item, _)| matches!(item.kind(), Kind::Tool | Kind::Compaction))
        .map(|(item, _)| {
            let detail = item.detail.as_deref();
            (
                item.kind(),
                detail.and_then(|detail| detail.header.as_deref().map(str::to_owned)),
                detail.and_then(|detail| detail.status),
                detail.is_some_and(|detail| detail.diff.is_some()),
            )
        })
        .collect();
    let header = |said: &str| Some(said.to_owned());
    assert_eq!(
        drawn,
        [
            (
                Kind::Tool,
                header("$ sed -n '1,200p' notes.txt"),
                Some(Status::Ok),
                false
            ),
            (
                Kind::Tool,
                header("$ sed -n '1,200p' notes.txt"),
                Some(Status::Ok),
                false
            ),
            (
                Kind::Tool,
                header("$ ls no-such-folder"),
                Some(Status::Failed),
                false
            ),
            (
                Kind::Tool,
                header("$ ls no-such-folder"),
                Some(Status::Ok),
                false
            ),
            (
                Kind::Tool,
                header("edit /work/app/notes.txt"),
                Some(Status::Ok),
                true
            ),
            (
                Kind::Tool,
                header("edit /work/app/notes.txt"),
                Some(Status::Ok),
                true
            ),
            (Kind::Compaction, None, None, false),
        ]
    );
}
