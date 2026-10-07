//! The projection (`project`): a record waits as what replay reads of it, and
//! the line it is read from is built into no more than that (`keep`).
//!
//! What a record says that nothing reads changes nothing, wherever it is
//! said (the fields that are read are in `fields`). That holds of a record
//! read whole and of one read as `keep` says, since the second is what a
//! look reads; and what replay fails on, it fails on after every line is read.

use std::collections::BTreeSet;

use cf_base::json::{from_slice_lossy, from_slice_lossy_keeping};

use super::*;
use keep::RECORD;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/engine/fixtures/completion/claude-code"
);

/// The real transcripts of the fixtures: each file's name, its session, and
/// its records.
fn fixtures() -> Vec<(String, String, Vec<Value>)> {
    let mut names: Vec<_> = fs::read_dir(FIXTURES)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".jsonl"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let text = fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap();
            let records: Vec<Value> = text
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let session = records
                .iter()
                .find_map(|record| record["sessionId"].as_str())
                .unwrap()
                .to_owned();
            (name, session, records)
        })
        .collect()
}

/// A look at a transcript of `session`, as `show` writes it.
fn read_as(session: &str, records: &[Value]) -> String {
    let root = tempfile::tempdir().unwrap();
    let file = root
        .path()
        .join("projects")
        .join(format!("{session}.jsonl"));
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, Stage::lines(records)).unwrap();
    let env = Env::from_vars([("CLAUDE_CONFIG_DIR", root.path().as_os_str())]);
    show(&reader(session, &env, &local()).look(&Options::default(), 0))
}

/// What a look reads of `record` as `keep` says: the record built as a line
/// is, and no more.
fn kept(record: &Value) -> Value {
    from_slice_lossy_keeping(record.to_string().as_bytes(), &RECORD).unwrap()
}

/// What the projection holds of a record: of the record whole, and of the
/// record as a line builds it. They are one.
pub(super) fn projected(record: &Value, session: &str) -> Projected {
    let whole = project(
        &from_slice_lossy(record.to_string().as_bytes()).unwrap(),
        session,
    );
    assert_eq!(whole, project(&kept(record), session), "{record}");
    whole
}

/// A value nothing reads: deep, with every kind of number and of text in it.
fn unread() -> Value {
    json!({
        "stdout": "line\n\"quoted\" \\ é 日本 😀 \u{1b}[0m",
        "numbers": [0, -0.0, 1.5e300, 9_007_199_254_740_993_u64, -9_007_199_254_740_993_i64, 5e-324],
        "flags": [true, false, null],
        "deep": (0..12).fold(json!("bottom"), |inner, _| json!([{ "k": inner }])),
        "type": "user", "uuid": 7, "content": { "type": "text", "text": "x" }, "text": ["a"],
    })
}

/// `record` with something nothing reads in every place a record says things:
/// the record, its message, each block of its content, its attachment.
fn noisy(mut record: Value) -> Value {
    let add = |fields: &mut Map<String, Value>, names: &[&str]| {
        for name in names {
            fields.insert((*name).to_owned(), unread());
        }
    };
    let Value::Object(fields) = &mut record else {
        return record;
    };
    add(
        fields,
        &[
            "toolUseResult",
            "wireToolInputs",
            "snapshot",
            "rendered",
            "serverClassifierContext",
            "requestId",
            "cwd",
            "session_id",
            "promptId",
            "sourceToolAssistantUUID",
            "origin",
            "leafUuid",
        ],
    );
    if let Some(Value::Object(message)) = fields.get_mut("message") {
        add(
            message,
            &[
                "usage",
                "model",
                "type",
                "stop_sequence",
                "stop_details",
                "diagnostics",
            ],
        );
        if let Some(Value::Array(blocks)) = message.get_mut("content") {
            for block in blocks {
                if let Value::Object(block) = block {
                    add(
                        block,
                        &[
                            "input",
                            "caller",
                            "name",
                            "signature",
                            "thinking",
                            "is_error",
                        ],
                    );
                }
            }
        }
    }
    if let Some(Value::Object(attachment)) = fields.get_mut("attachment") {
        add(
            attachment,
            &["hookName", "toolUseID", "addedNames", "names", "skillCount"],
        );
    }
    record
}

/// Which kind of record the projection holds.
fn variant(projected: &Projected) -> &'static str {
    match projected {
        Projected::Other => "other",
        Projected::Hook(_) => "hook",
        Projected::Enqueue(_) => "enqueue",
        Projected::Dequeue => "dequeue",
        Projected::PopAll(_) => "popAll",
        Projected::Remove(_) => "remove",
        Projected::Assistant(_) => "assistant",
        Projected::User(_) => "user",
        Projected::Clear(_) => "clear",
        Projected::Boundary(_) => "boundary",
    }
}

/// Records of every kind the projection holds, as the helpers write them.
fn made() -> Vec<Value> {
    let output = |text: &str| user("u1", Value::Null, json!(text));
    vec![
        hello(),
        output(&command("", "")),
        answer("a1", "u1"),
        assistant(
            "a2",
            "a1",
            json!("m2"),
            json!([tool_use(json!("t1"))]),
            Value::Null,
        ),
        refused("a3", json!({ "apiErrorStatus": 429 })),
        user(
            "u2",
            json!("a2"),
            json!([tool_result(json!("t1"), json!("out"))]),
        ),
        hook(json!(["context", "more"]), Some("h1")),
        attachment("at1", "u1"),
        queue("enqueue", json!("later")),
        queue("dequeue", Value::Null),
        queue("popAll", json!("later")),
        queue("remove", json!("later")),
        queue("elsewhere", json!("later")),
        duration("d1", "a1"),
        summary("s1", "a1"),
        output_of_clear(),
        interrupt(),
        json!({ "type": "mode", "mode": "normal", "sessionId": SESSION }),
    ]
}

fn output_of_clear() -> Value {
    output("u1")
}

/// Claude Code's own record of an interrupt.
pub(super) fn interrupt() -> Value {
    user(
        "i1",
        json!("a1"),
        json!([text(json!("[Request interrupted by user]"))]),
    )
}

#[test]
fn what_nothing_reads_of_a_record_changes_neither_how_it_waits_nor_the_reading() {
    let mut seen = BTreeSet::new();
    for (name, session, records) in fixtures() {
        for record in &records {
            let plain = projected(record, &session);
            assert_eq!(
                projected(&noisy(record.clone()), &session),
                plain,
                "{name}: {record}"
            );
            seen.insert(variant(&plain));
        }
        let noisy_records: Vec<Value> = records.iter().cloned().map(noisy).collect();
        assert_eq!(
            read_as(&session, &noisy_records),
            read_as(&session, &records),
            "{name}"
        );
    }
    for record in made() {
        let plain = projected(&record, SESSION);
        assert_eq!(
            projected(&noisy(record.clone()), SESSION),
            plain,
            "{record}"
        );
        seen.insert(variant(&plain));
    }
    // Every kind of record the projection holds was among them.
    let every = [
        "other",
        "hook",
        "enqueue",
        "dequeue",
        "popAll",
        "remove",
        "assistant",
        "user",
        "clear",
        "boundary",
    ];
    assert_eq!(
        seen,
        BTreeSet::from(every),
        "kinds of record the cases hold"
    );
    // And the noise is no small one.
    let size = |record: &Value| record.to_string().len();
    assert!(size(&noisy(answer("a1", "u1"))) > 20 * size(&answer("a1", "u1")));
}

#[test]
fn a_record_waits_as_little_as_its_reading_asks_of_it() {
    // What a record that says nothing of the turn waits as.
    for record in [
        json!({ "type": "mode", "mode": "normal" }),
        json!({ "type": "file-history-snapshot", "snapshot": unread() }),
        json!({ "type": "last-prompt", "lastPrompt": "x", "sessionId": SESSION }),
        json!([1, 2]),
        json!("a string"),
        json!(5),
        json!({ "type": ["attachment"] }),
        json!({}),
    ] {
        assert_eq!(projected(&record, SESSION), Projected::Other, "{record}");
    }
    // A hook's attachment that is not the conversation's own, and one with
    // nothing to say, wait as nothing; a system record that ends no turn too.
    assert_eq!(
        projected(&attachment("at1", "u1"), SESSION),
        Projected::Other
    );
    assert_eq!(
        projected(&hook(json!([1, {}]), Some("h")), SESSION),
        Projected::Other
    );
    assert_eq!(
        projected(
            &having(
                hook(json!(["x"]), Some("h")),
                json!({ "sessionId": "other" })
            ),
            SESSION
        ),
        Projected::Other
    );
    assert_eq!(
        projected(
            &having(duration("d1", "a1"), json!({ "durationMs": -1 })),
            SESSION
        ),
        Projected::Other
    );
    // What it holds of what it says: the uuid of the boundary, the text of a result.
    assert_eq!(
        projected(&duration("d1", "a1"), SESSION),
        Projected::Boundary(Some(Arc::from("d1")))
    );
    assert_eq!(
        projected(&queue("enqueue", json!(7)), SESSION),
        Projected::Enqueue(Ok("7".to_owned()))
    );
    assert_eq!(
        projected(&queue("remove", json!({ "toString": 1 })), SESSION),
        Projected::Remove(Err(
            "an object with a toString of its own cannot be made text".to_owned()
        ))
    );
    let Projected::User(result) = projected(
        &user(
            "u2",
            json!("a2"),
            json!([tool_result(json!("t1"), json!([{ "type": "image" }]))]),
        ),
        SESSION,
    ) else {
        panic!("a user's record");
    };
    assert_eq!(result.results[0].text, Ok(r#"{"type":"image"}"#.to_owned()));
}

#[test]
fn what_replay_fails_on_fails_the_look_after_every_line_is_read_and_in_the_order_it_met_it() {
    let mut stage = Stage::new();
    let append_raw = |stage: &Stage, text: &str| {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(stage.file())
            .unwrap();
        std::io::Write::write_all(&mut file, text.as_bytes()).unwrap();
    };
    // Each record is one replay fails on: a result with no id, a text V8
    // cannot make of an object, an assistant's message with no id.
    let no_id = user(
        "u2",
        json!("u1"),
        json!([tool_result(json!({ "a": 1 }), json!("x"))]),
    );
    let no_text = user(
        "u2",
        json!("u1"),
        json!([tool_result(
            json!("t1"),
            json!([{ "type": "text", "text": { "toString": 1 } }])
        )]),
    );
    let no_message_id = in_message(answer("a1", "u1"), "id", None);
    let queue_bad = queue("enqueue", json!({ "toString": 1 }));
    let failures = [
        (no_id, "missing native claude tool result id at record 1"),
        (
            no_text,
            "an object with a toString of its own cannot be made text",
        ),
        (
            no_message_id,
            "missing native claude message id at record 1",
        ),
        (
            queue_bad,
            "an object with a toString of its own cannot be made text",
        ),
    ];
    for (record, said) in failures {
        stage.write(&[hello(), record]);
        assert_eq!(stage.look(), format!("unknown: unreadable: {said}"));
        // A line after it that cannot be read is the look's failure, for the
        // look reads every line before it replays any.
        append_raw(&stage, "{\"type\":\n");
        assert_eq!(
            stage.look(),
            "unknown: unreadable: malformed JSONL at record 2"
        );
        // And so is a line too deep for a value, in a part nobody reads.
        stage.write(&[hello(), failures_record()]);
        let deep = format!(
            "{{\"toolUseResult\":{}}}\n",
            "[".repeat(300) + &"]".repeat(300)
        );
        append_raw(&stage, &deep);
        assert!(
            stage
                .look()
                .starts_with("unknown: unreadable: JSON this build cannot hold at record 2"),
            "{said}"
        );
    }
}

/// A record replay fails on, as the line after it is read before it is replayed.
fn failures_record() -> Value {
    user(
        "u2",
        json!("u1"),
        json!([tool_result(json!({ "a": 1 }), json!("x"))]),
    )
}
