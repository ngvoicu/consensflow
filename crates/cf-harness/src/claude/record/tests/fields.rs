//! Each field replay reads of a record, one row of `read` for each: the
//! reading of a transcript with the field as it is is not the reading with it
//! changed, so the projection holds it, and so does the line it is built from
//! (`keep`). A row is a scenario, the record of it that changes, and how.

use super::projection::{interrupt, projected};
use super::*;

const LATER: &str = "2026-09-20T11:30:00.000Z";

/// The records of a transcript.
type Scenario = fn() -> Vec<Value>;

/// What a record becomes when a field of it is changed.
type Change = Box<dyn Fn(Value) -> Value>;

/// A field, a scenario that says it, the place in the scenario of the record
/// that does, and how the record is changed.
type Row = (&'static str, Scenario, usize, Change);

/// The record with the field `name` said as `value`.
fn is(name: &'static str, value: Value) -> Change {
    Box::new(move |record| having(record, json!({ name: value.clone() })))
}

/// The record with the field `name` left out.
fn without(name: &'static str) -> Change {
    Box::new(move |record| lacking(record, name))
}

/// The record with the field `name` of its message said as `value`.
fn in_the_message(name: &'static str, value: Value) -> Change {
    Box::new(move |record| in_message(record, name, Some(value.clone())))
}

/// The record with the field `name` of its attachment said as `value`.
fn in_the_attachment(name: &'static str, value: Value) -> Change {
    Box::new(move |record| in_attachment(record, name, Some(value.clone())))
}

/// A fragment `uuid` of the message `m1`, saying `said`.
fn fragment(uuid: &str, said: &str) -> Value {
    assistant(
        uuid,
        "u1",
        json!("m1"),
        json!([text(json!(said))]),
        Value::Null,
    )
}

fn tool(id: &str) -> Value {
    json!({ "type": "tool_use", "id": id, "name": "Read" })
}

// The scenarios.

/// A turn that ends at its boundary.
fn ended() -> Vec<Value> {
    vec![hello(), answer("a1", "u1"), duration("d1", "a1")]
}

/// A turn that ends at the stop hooks' summary.
fn summarized() -> Vec<Value> {
    vec![hello(), answer("a1", "u1"), summary("s1", "a1")]
}

/// The answer `A`, the boundary `D` after it, then the user `U` that wrote it
/// and the attachments between: `U` is a late ancestor of `A`, and the turn is
/// settled, if `D` and the chain say so (`ancestry`).
fn late() -> Vec<Value> {
    let prompt = user("U", Value::Null, json!("Hello"));
    let (first, second) = (attachment("T1", "U"), attachment("T2", "T1"));
    vec![answer("A", "T2"), duration("D", "A"), prompt, first, second]
}

/// A `/clear` and its output.
fn cleared() -> Vec<Value> {
    vec![
        user("u1", Value::Null, json!(command("", ""))),
        output("u1"),
    ]
}

fn hooked() -> Vec<Value> {
    vec![hook(json!(["ctx"]), Some("h1"))]
}

fn fragments() -> Vec<Value> {
    vec![hello(), fragment("a1", "One"), fragment("a2", "Two")]
}

/// A call to the tool `t1`, its result, and the answer.
fn answered_call() -> Vec<Value> {
    let result = user(
        "u2",
        json!("a1"),
        json!([tool_result(json!("t1"), json!("out"))]),
    );
    let calling = assistant("a1", "u1", json!("m1"), json!([tool("t1")]), Value::Null);
    let done = assistant(
        "a2",
        "u2",
        json!("m1"),
        json!([text(json!("Done"))]),
        json!("end_turn"),
    );
    vec![hello(), calling, result, done, duration("d1", "a2")]
}

/// A turn whose message makes a call `call`, which nothing answers, then answers.
fn calling(call: Value) -> Vec<Value> {
    let calling = assistant("a1", "u1", json!("m1"), json!([call]), Value::Null);
    let done = assistant(
        "a2",
        "a1",
        json!("m1"),
        json!([text(json!("Done"))]),
        json!("end_turn"),
    );
    vec![hello(), calling, done, duration("d1", "a2")]
}

fn calling_a_tool() -> Vec<Value> {
    calling(tool("t1"))
}

fn calling_a_server_tool() -> Vec<Value> {
    calling(json!({ "type": "server_tool_use", "id": "t1" }))
}

/// A refusal that says when the limit resets, by its status.
fn refused_by_status() -> Vec<Value> {
    vec![hello(), refused("a1", json!({ "apiErrorStatus": 429 }))]
}

/// A refusal that says when the limit resets, by its error.
fn refused_by_error() -> Vec<Value> {
    vec![hello(), refused("a1", json!({ "error": "rate_limit" }))]
}

/// The turn ends, and Claude Code's own record of an interrupt follows.
fn interrupted() -> Vec<Value> {
    vec![hello(), answer("a1", "u1"), interrupt()]
}

/// A message the user queued while the turn was open.
fn queued_message() -> Vec<Value> {
    let queued = queue("enqueue", json!("m"));
    vec![hello(), queued, answer("a1", "u1"), duration("d1", "a1")]
}

/// A message queued, and taken back.
fn queued_and_removed() -> Vec<Value> {
    let (queued, removed) = (queue("enqueue", json!("m")), queue("remove", json!("m")));
    vec![
        hello(),
        queued,
        removed,
        answer("a1", "u1"),
        duration("d1", "a1"),
    ]
}

/// A prompt that is the queue's message, typed, and the answer to it: the
/// message is taken from the queue, or it is not, and the turn is settled, or
/// it is not.
fn prompted() -> Vec<Value> {
    let prompt = having(
        user("u2", json!("d1"), json!("Again")),
        json!({ "promptSource": "typed" }),
    );
    let answered = assistant(
        "a2",
        "u2",
        json!("m2"),
        json!([text(json!("Hi"))]),
        json!("end_turn"),
    );
    vec![
        hello(),
        queue("enqueue", json!("Again")),
        answer("a1", "u1"),
        duration("d1", "a1"),
        prompt,
        answered,
        duration("d2", "a2"),
    ]
}

/// Each field replay reads.
fn read() -> Vec<Row> {
    let progress = || is("type", json!("progress"));
    let other = || is("sessionId", json!("other"));
    let sidechain = || is("isSidechain", json!(true));
    let later = || is("timestamp", json!(LATER));
    let blocks = |blocks: Value| in_the_message("content", blocks);
    vec![
        ("type", ended, 2, progress()),
        (
            "type, of the attachments an answer is the child of",
            late,
            3,
            progress(),
        ),
        ("subtype", ended, 2, is("subtype", json!("other"))),
        ("timestamp, of an assistant's message", ended, 1, later()),
        ("timestamp, of a user's turn", ended, 0, later()),
        ("timestamp, of a tool's result", answered_call, 2, later()),
        ("timestamp, of a hook's context", hooked, 0, later()),
        (
            "timestamp, of a refusal for quota",
            refused_by_status,
            1,
            later(),
        ),
        (
            "uuid, of an assistant's fragment",
            fragments,
            2,
            is("uuid", json!("a1")),
        ),
        ("uuid, of a user's turn", ended, 0, is("uuid", json!("u9"))),
        (
            "uuid, of a hook's context",
            hooked,
            0,
            is("uuid", json!("h2")),
        ),
        (
            "uuid, of the boundary record",
            late,
            1,
            is("uuid", json!(7)),
        ),
        (
            "parentUuid, of a /clear's output",
            cleared,
            1,
            is("parentUuid", json!("u9")),
        ),
        (
            "parentUuid, of the boundary record",
            late,
            1,
            is("parentUuid", json!("X")),
        ),
        ("sessionId, of a hook's context", hooked, 0, other()),
        ("sessionId, of a /clear's output", cleared, 1, other()),
        ("sessionId, of the boundary record", late, 1, other()),
        ("isSidechain, of a hook's context", hooked, 0, sidechain()),
        ("isSidechain, of a /clear's output", cleared, 1, sidechain()),
        ("isSidechain, of a turn_duration", ended, 2, sidechain()),
        ("isSidechain, of the boundary record", late, 1, sidechain()),
        (
            "message, of an assistant's record",
            ended,
            1,
            without("message"),
        ),
        (
            "message.id",
            fragments,
            2,
            in_the_message("id", json!("m2")),
        ),
        ("message.content, as text", ended, 1, blocks(json!("Bye"))),
        (
            "message.content, of a user's turn",
            ended,
            0,
            blocks(json!([text(json!("Hello")), text(json!("Again"))])),
        ),
        (
            "message.stop_reason",
            ended,
            1,
            in_the_message("stop_reason", json!("max_tokens")),
        ),
        (
            "message.role",
            interrupted,
            2,
            in_the_message("role", json!("assistant")),
        ),
        (
            "block.type, of a text",
            ended,
            1,
            blocks(json!([{ "type": "image", "text": "Hi" }])),
        ),
        (
            "block.type, of a call",
            calling_a_tool,
            1,
            blocks(json!([{ "type": "other", "id": "t1" }])),
        ),
        (
            "block.type, of a server's call",
            calling_a_server_tool,
            1,
            blocks(json!([{ "type": "other", "id": "t1" }])),
        ),
        (
            "block.type, of a tool's result",
            answered_call,
            2,
            blocks(
                json!([{ "type": "advisor_tool_result", "tool_use_id": "t1", "content": "out" }]),
            ),
        ),
        ("block.text", ended, 1, blocks(json!([text(json!("Bye"))]))),
        (
            "block.id, of a call",
            answered_call,
            1,
            blocks(json!([tool("t2")])),
        ),
        (
            "block.tool_use_id",
            answered_call,
            2,
            blocks(json!([tool_result(json!("t2"), json!("out"))])),
        ),
        (
            "block.content, of a result",
            answered_call,
            2,
            blocks(json!([tool_result(json!("t1"), json!("other"))])),
        ),
        (
            "isApiErrorMessage",
            refused_by_status,
            1,
            is("isApiErrorMessage", json!(false)),
        ),
        (
            "apiErrorStatus",
            refused_by_status,
            1,
            is("apiErrorStatus", json!(500)),
        ),
        (
            "error",
            refused_by_error,
            1,
            is("error", json!("overloaded")),
        ),
        (
            "message.content, of an interrupt",
            interrupted,
            2,
            blocks(json!([text(json!("[Request interrupted]"))])),
        ),
        (
            "promptSource",
            prompted,
            4,
            is("promptSource", json!("queued")),
        ),
        ("isMeta", cleared, 1, is("isMeta", json!(true))),
        ("level", cleared, 1, is("level", json!("warn"))),
        (
            "content, of a /clear's output",
            cleared,
            1,
            is(
                "content",
                json!("<local-command-stdout>x</local-command-stdout>"),
            ),
        ),
        ("durationMs", ended, 2, is("durationMs", json!(-1))),
        ("messageCount", ended, 2, is("messageCount", json!(1.5))),
        (
            "pendingBackgroundAgentCount",
            ended,
            2,
            is("pendingBackgroundAgentCount", json!(1)),
        ),
        (
            "pendingWorkflowCount",
            ended,
            2,
            is("pendingWorkflowCount", json!(1)),
        ),
        (
            "preventedContinuation",
            summarized,
            2,
            is("preventedContinuation", json!(true)),
        ),
        (
            "operation",
            queued_message,
            1,
            is("operation", json!("elsewhere")),
        ),
        (
            "content, of a queue operation",
            queued_and_removed,
            2,
            is("content", json!("n")),
        ),
        (
            "attachment, of a hook's context",
            hooked,
            0,
            without("attachment"),
        ),
        (
            "attachment.type",
            hooked,
            0,
            in_the_attachment("type", json!("skill_listing")),
        ),
        (
            "attachment.hookEvent",
            hooked,
            0,
            in_the_attachment("hookEvent", json!("SessionStart")),
        ),
        (
            "attachment.content",
            hooked,
            0,
            in_the_attachment("content", json!(["other"])),
        ),
    ]
}

#[test]
fn each_field_replay_reads_of_a_record_changes_the_reading() {
    let rows = read();
    for (field, scenario, at, change) in &rows {
        let base = scenario();
        let mut changed = base.clone();
        changed[*at] = change(changed[*at].clone());
        assert_ne!(
            once(&base),
            once(&changed),
            "{field}: the reading does not change"
        );
        // The same holds of the records built as a line is: what the line
        // keeps is what the projection reads.
        for record in base.iter().chain(&changed) {
            projected(record, SESSION);
        }
    }
    // Fifty-odd fields, none twice.
    let names: std::collections::BTreeSet<_> = rows.iter().map(|(field, ..)| *field).collect();
    assert_eq!(names.len(), rows.len());
    assert!(rows.len() >= 50, "{}", rows.len());
}
