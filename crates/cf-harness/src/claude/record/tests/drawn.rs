//! What the history draws of a transcript besides its items' words (chief-harness CHF-14,
//! CHF-15): a message's thinking beside the items, a call with its header, how it ended,
//! how long it took and the change it made, a call still running, and the tokens a
//! compaction compacted from. None of it moves the reading: the items, their words and
//! what they say of the turn are what they were without it.

use super::compaction::compacted;
use super::*;
use crate::shared::record::drawn::{Kind, Status, Timeline};
use crate::shared::record::reading::{Record, Role};

/// The record a look at `records` reads.
fn record_of(records: &[Value]) -> Record {
    let mut stage = Stage::new();
    stage.write(records);
    match &*stage.look_told(&Options::default()) {
        Reading::Known(record) => record.clone(),
        other => panic!("unread: {other:?}"),
    }
}

/// The timeline of `record`: each item's id and kind, and whether it stands beside the
/// items.
fn timeline(record: &Record) -> Vec<(String, Kind, bool)> {
    Timeline::of(record)
        .from(0)
        .map(|(item, beside)| (item.id.to_string(), item.kind(), beside))
        .collect()
}

fn thinking(words: &str) -> Value {
    json!({ "type": "thinking", "thinking": words, "signature": "sig" })
}

fn at(record: Value, time: &str) -> Value {
    having(record, json!({ "timestamp": time }))
}

fn call(id: &str, name: &str, input: Value) -> Value {
    json!({ "type": "tool_use", "id": id, "name": name, "input": input })
}

/// A message `m1` that thinks, then says, then calls Bash, each a record of its own as
/// Claude Code writes a message; and the call's result.
fn thought_said_called(result: Value) -> Vec<Value> {
    vec![
        at(hello(), "2026-11-02T10:00:00.000Z"),
        at(
            assistant(
                "a1",
                "u1",
                json!("m1"),
                json!([thinking("Which file?")]),
                Value::Null,
            ),
            "2026-11-02T10:00:01.000Z",
        ),
        at(
            assistant(
                "a2",
                "a1",
                json!("m1"),
                json!([text(json!("Looking."))]),
                Value::Null,
            ),
            "2026-11-02T10:00:02.000Z",
        ),
        at(
            assistant(
                "a3",
                "a2",
                json!("m1"),
                json!([call(
                    "t1",
                    "Bash",
                    json!({ "command": "ls nope", "description": "List" })
                )]),
                json!("tool_use"),
            ),
            "2026-11-02T10:00:03.000Z",
        ),
        result,
    ]
}

fn failed_result() -> Value {
    at(
        user(
            "u2",
            json!("a3"),
            json!([{ "type": "tool_result", "tool_use_id": "t1", "is_error": true,
                     "content": "ls: nope: No such file or directory" }]),
        ),
        "2026-11-02T10:00:04.500Z",
    )
}

/// `records` with every thinking block and every call's input taken out: what the reading
/// said before the history read them.
fn without_drawn(records: &[Value]) -> Vec<Value> {
    records
        .iter()
        .map(|record| {
            let mut record = record.clone();
            if let Some(Value::Array(blocks)) = record.pointer_mut("/message/content") {
                blocks.retain(|block| block["type"] != "thinking");
                for block in blocks.iter_mut() {
                    if let Some(fields) = block.as_object_mut() {
                        fields.remove("input");
                        fields.remove("is_error");
                    }
                }
            }
            record
        })
        .collect()
}

#[test]
fn thinking_stands_beside_the_items_before_the_words_of_its_message() {
    let records = thought_said_called(failed_result());
    let record = record_of(&records);
    assert_eq!(
        timeline(&record),
        [
            ("u1".to_owned(), Kind::Prompt, false),
            ("a1:thinking:0".to_owned(), Kind::Thinking, true),
            ("m1".to_owned(), Kind::Text, false),
            ("t1".to_owned(), Kind::Tool, false),
        ]
    );
    let (_, thought) = &record.beside[0];
    assert_eq!(&*thought.text, "Which file?");
    // No reader of the agent's words takes it: its role is none of theirs.
    assert_eq!(thought.role, Role::Custom);
    // The reading is what it was with no thinking, and no input of a call, in it.
    assert_eq!(once(&records), once(&without_drawn(&records)));
    assert!(record
        .items
        .iter()
        .all(|item| item.kind() != Kind::Thinking));
}

#[test]
fn thinking_said_in_the_record_of_its_words_stands_before_them() {
    // A message written whole in one record (no streaming): its thinking still stands
    // before its words, as it was thought before them.
    let records = vec![
        hello(),
        assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([thinking("First this."), text(json!("Done."))]),
            json!("end_turn"),
        ),
    ];
    let record = record_of(&records);
    assert_eq!(
        timeline(&record),
        [
            ("u1".to_owned(), Kind::Prompt, false),
            ("a1:thinking:0".to_owned(), Kind::Thinking, true),
            ("m1".to_owned(), Kind::Text, false),
        ]
    );
}

#[test]
fn a_message_that_only_thought_answers_with_no_words_as_it_did() {
    let records = vec![
        hello(),
        assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([thinking("Nothing to say.")]),
            json!("end_turn"),
        ),
        duration("d1", "a1"),
    ];
    let record = record_of(&records);
    // The turn's answer is the message's, with its words (none), never its thinking.
    let answer = record.items.last().unwrap();
    assert_eq!((answer.role, &*answer.text), (Role::Assistant, ""));
    assert_eq!(once(&records), once(&without_drawn(&records)));
    assert_eq!(record.beside.len(), 1);
}

#[test]
fn a_call_is_its_results_item_with_its_header_how_it_ended_and_how_long_it_took() {
    let record = record_of(&thought_said_called(failed_result()));
    let tool = record.items.iter().find(|item| &*item.id == "t1").unwrap();
    let detail = tool.detail.as_deref().unwrap();
    assert_eq!(detail.kind, Kind::Tool);
    assert_eq!(detail.name.as_deref(), Some("Bash"));
    assert_eq!(detail.header.as_deref(), Some("$ ls nope"));
    assert_eq!(detail.status, Some(Status::Failed));
    // From the call's record to its result's: 1.5 s.
    assert_eq!(detail.took_ms, Some(1_500));
    assert_eq!(&*tool.text, "ls: nope: No such file or directory");
    // Answered, it stands beside the items no more.
    assert!(record
        .beside
        .iter()
        .all(|(_, item)| item.kind() == Kind::Thinking));
}

#[test]
fn a_call_says_the_time_its_record_says_it_took_and_an_edits_change() {
    let result = having(
        user(
            "u2",
            json!("a3"),
            json!([tool_result(
                json!("t1"),
                json!("The file a.rs has been updated.")
            )]),
        ),
        json!({
            "timestamp": "2026-11-02T10:00:09.000Z",
            "toolUseResult": {
                "durationMs": 42,
                "structuredPatch": [{
                    "oldStart": 3, "oldLines": 1, "newStart": 3, "newLines": 2,
                    "lines": ["-let a = 1;", "+let a = 2;", "+let b = 3;"],
                }],
            },
        }),
    );
    let mut records = thought_said_called(result);
    records[3] = at(
        assistant(
            "a3",
            "a2",
            json!("m1"),
            json!([call(
                "t1",
                "Edit",
                json!({ "file_path": "src/a.rs", "old_string": "x" })
            )]),
            json!("tool_use"),
        ),
        "2026-11-02T10:00:03.000Z",
    );
    let record = record_of(&records);
    let detail = record
        .items
        .iter()
        .find(|item| &*item.id == "t1")
        .unwrap()
        .detail
        .clone()
        .unwrap();
    assert_eq!(detail.header.as_deref(), Some("edit src/a.rs"));
    assert_eq!(detail.status, Some(Status::Ok));
    assert_eq!(detail.took_ms, Some(42));
    assert_eq!(
        detail.diff.as_deref(),
        Some("@@ -3,1 +3,2 @@\n-let a = 1;\n+let a = 2;\n+let b = 3;")
    );
}

#[test]
fn a_read_says_its_lines_and_another_tool_what_it_was_given() {
    for (name, input, header) in [
        (
            "Read",
            json!({ "file_path": "a.md", "offset": 10, "limit": 20 }),
            "read a.md:10-29",
        ),
        ("Read", json!({ "file_path": "a.md" }), "read a.md"),
        (
            "Write",
            json!({ "file_path": "b.md", "content": "x" }),
            "write b.md",
        ),
        (
            "Grep",
            json!({ "pattern": "fn main", "path": "src" }),
            "Grep fn main",
        ),
        ("TodoWrite", json!({ "todos": [] }), "TodoWrite"),
    ] {
        let mut records = thought_said_called(at(
            user(
                "u2",
                json!("a3"),
                json!([tool_result(json!("t1"), json!("out"))]),
            ),
            "2026-11-02T10:00:04.000Z",
        ));
        records[3] = assistant(
            "a3",
            "a2",
            json!("m1"),
            json!([call("t1", name, input)]),
            json!("tool_use"),
        );
        let record = record_of(&records);
        let detail = record
            .items
            .iter()
            .find(|item| &*item.id == "t1")
            .unwrap()
            .detail
            .clone()
            .unwrap();
        assert_eq!(detail.header.as_deref(), Some(header), "{name}");
        // A call whose record says no time took none the history knows of.
        assert_eq!(detail.took_ms, None, "{name}");
    }
}

#[test]
fn a_call_with_no_result_yet_runs_beside_the_items_after_its_message() {
    let mut records = thought_said_called(Value::Null);
    records.pop();
    let record = record_of(&records);
    assert_eq!(
        timeline(&record).last().unwrap(),
        &("t1".to_owned(), Kind::Tool, true)
    );
    let (place, running) = record.beside.last().unwrap();
    // It stands after the message that called it, the last of the items.
    assert_eq!(*place, record.items.len());
    let detail = running.detail.as_deref().unwrap();
    assert_eq!(detail.status, Some(Status::Running));
    assert_eq!(detail.header.as_deref(), Some("$ ls nope"));
    assert_eq!(&*running.text, "");
    // No item of the reading is the call's until its result is.
    assert!(record.items.iter().all(|item| &*item.id != "t1"));
}

#[test]
fn a_call_its_turn_left_without_a_result_says_no_more_that_it_runs() {
    let mut records = thought_said_called(Value::Null);
    records.pop();
    records.push(super::projection::interrupt());
    let record = record_of(&records);
    let (_, call) = record
        .beside
        .iter()
        .find(|(_, item)| &*item.id == "t1")
        .unwrap();
    assert_eq!(call.detail.as_deref().unwrap().status, None);
}

#[test]
fn a_compactions_summary_says_the_tokens_its_boundary_compacted_from() {
    let mut boundary = compacted("b1", "a1");
    boundary["compactMetadata"] = json!({ "trigger": "manual", "preTokens": 51_234 });
    let records = vec![
        hello(),
        answer("a1", "u1"),
        duration("d1", "a1"),
        boundary,
        super::compaction::compact_summary("s1", "b1"),
    ];
    let record = record_of(&records);
    let summary = record.items.last().unwrap();
    assert!(summary.compaction);
    assert_eq!(summary.kind(), Kind::Compaction);
    assert_eq!(summary.detail.as_deref().unwrap().tokens, Some(51_234));
    // A boundary that says no tokens says none.
    let mut plain = records.clone();
    plain[3] = compacted("b1", "a1");
    let record = record_of(&plain);
    assert_eq!(
        record
            .items
            .last()
            .unwrap()
            .detail
            .as_deref()
            .unwrap()
            .tokens,
        None
    );
    assert_eq!(once(&records), once(&plain));
}

#[test]
fn the_items_ids_are_those_they_had_and_thinking_ids_hold_from_look_to_look() {
    let records = thought_said_called(failed_result());
    let mut stage = Stage::new();
    stage.write(&records[..3]);
    let first = match &*stage.look_told(&Options::default()) {
        Reading::Known(record) => timeline(record),
        other => panic!("{other:?}"),
    };
    stage.append(&records[3..]);
    let second = match &*stage.look_told(&Options::default()) {
        Reading::Known(record) => timeline(record),
        other => panic!("{other:?}"),
    };
    assert_eq!(first[..2], second[..2]);
    assert_eq!(first[1].0, "a1:thinking:0");
}

/// The live run's transcript (T-728: Claude Code 2.1.316 on Sonnet 5.5, a read, a command
/// that fails, an edit and `/compact`), made a fixture by `cargo xtask scrub-transcript`.
#[test]
fn a_live_chiefs_transcript_reads_as_the_live_check_read_it() {
    let text = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/history-items/claude.jsonl"
    ))
    .unwrap()
    .replace("$SESSION", SESSION);
    let records: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let record = record_of(&records);
    let tools: Vec<(Option<&str>, Option<Status>, bool)> = record
        .items
        .iter()
        .filter(|item| item.kind() == Kind::Tool)
        .map(|item| {
            let detail = item.detail.as_deref().unwrap();
            (
                detail.header.as_deref(),
                detail.status,
                detail.diff.is_some(),
            )
        })
        .collect();
    assert_eq!(
        tools,
        [
            (Some("read /work/app/notes.txt"), Some(Status::Ok), false),
            (Some("$ ls no-such-folder"), Some(Status::Failed), false),
            (Some("edit /work/app/notes.txt"), Some(Status::Ok), true),
        ]
    );
    let compacted = record
        .items
        .iter()
        .find(|item| item.kind() == Kind::Compaction)
        .unwrap();
    assert_eq!(compacted.detail.as_deref().unwrap().tokens, Some(43_079));
    // Nothing running is left beside the items once every call was answered.
    assert!(record
        .beside
        .iter()
        .all(|(_, item)| item.kind() != Kind::Tool));
}

/// What the history adds to ConsensFlow's copy of a real transcript (CHF-18, "the ledger's
/// growth measured on a real long chief"): run by hand on a transcript named by
/// `CF_MEASURE_TRANSCRIPT`, read where it is (a second name of it in a folder of the test's own),
/// it prints counts and sizes alone, never a word of it.
#[test]
#[ignore = "by hand: CF_MEASURE_TRANSCRIPT=<a Claude transcript> cargo test -p cf-harness --lib measure -- --ignored --nocapture"]
fn measure_what_the_history_adds_to_a_real_transcripts_copy() {
    let Some(path) = Env::from_process()
        .path("CF_MEASURE_TRANSCRIPT")
        .map(Path::to_path_buf)
    else {
        return;
    };
    let session = path.file_stem().unwrap().to_string_lossy().into_owned();
    let root = tempfile::tempdir().unwrap();
    let projects = root.path().join("projects").join("measured");
    fs::create_dir_all(&projects).unwrap();
    // A second name of the same file (a link is passed over by the search), which goes with
    // the folder.
    fs::hard_link(&path, projects.join(format!("{session}.jsonl"))).unwrap();
    let env = Env::from_vars([("CLAUDE_CONFIG_DIR", root.path().as_os_str())]);
    let started = std::time::Instant::now();
    let reading = reader(&session, &env, &local()).look(&Options::default(), 0);
    let took = started.elapsed();
    let Reading::Known(record) = &*reading else {
        panic!("unread: {:?}", reading.reason());
    };
    let cut = |text: &str, max: usize| text.chars().take(max).map(char::len_utf8).sum::<usize>();
    const MAX: usize = 64_000;
    let words: usize = record
        .items
        .iter()
        .map(|item| {
            if item.role == Role::Tool {
                cut(&item.text, MAX)
            } else {
                item.text.len()
            }
        })
        .sum();
    let detail_bytes = |item: &crate::shared::record::reading::Item| {
        item.detail.as_deref().map_or(0, |detail| {
            [detail.name.as_deref(), detail.header.as_deref()]
                .into_iter()
                .flatten()
                .map(str::len)
                .sum::<usize>()
                + detail.diff.as_deref().map_or(0, |diff| cut(diff, MAX))
                + 48
        })
    };
    let details: usize = record.items.iter().map(detail_bytes).sum();
    let thinking: Vec<&crate::shared::record::reading::Item> = record
        .beside
        .iter()
        .map(|(_, item)| item)
        .filter(|item| item.kind() == Kind::Thinking)
        .collect();
    let thought: usize = thinking.iter().map(|item| cut(&item.text, MAX)).sum();
    let diffs = record
        .items
        .iter()
        .filter(|item| {
            item.detail
                .as_deref()
                .is_some_and(|detail| detail.diff.is_some())
        })
        .count();
    println!(
        "{}",
        serde_json::json!({
            "file_bytes": fs::metadata(&path).unwrap().len(),
            "read_ms": took.as_millis(),
            "items": record.items.len(),
            "item_bytes": words,
            "tools": record.items.iter().filter(|item| item.kind() == Kind::Tool).count(),
            "detail_bytes": details,
            "diffs": diffs,
            "thinking_items": thinking.len(),
            "thinking_bytes": thought,
            "beside": record.beside.len(),
        })
    );
}
