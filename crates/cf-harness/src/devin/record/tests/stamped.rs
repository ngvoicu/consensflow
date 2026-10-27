//! A turn as Devin 3000.11 writes it on the wire (T-660, from T-656's reading of the live
//! suites `notes`, `wake-ups` and `stops`, 2026-10-26): every record of its work stamped
//! with its time, as the history a reopened window replays is stamped, and Devin's own word
//! on the turn, `session_info_update` with `statusEnum` `working` as it begins and `blocked`
//! before its end. Its words are said in a stream, which a replay names none of. Read by its
//! stamps alone, a turn at work was a replay, and the window read at rest through a minute's
//! command, while its record's head was the user's message or the call's answer. A look
//! that read each step before reads the same as a fresh one.

use super::*;

/// What a reading says of the turn: its settlement, whether it is in flight, and whether
/// its last item is complete.
fn said(reading: &str) -> (String, bool, bool) {
    let reading: Value = serde_json::from_str(reading).unwrap_or_else(|_| panic!("{reading}"));
    let last = reading["items"]
        .as_array()
        .and_then(|items| items.last())
        .map_or(Value::Null, |item| item[3].clone());
    (
        reading["settlement"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        reading["inFlight"] == true,
        last == true,
    )
}

fn at_work() -> (String, bool, bool) {
    ("in-flight".to_owned(), true, true)
}

fn settled() -> (String, bool, bool) {
    ("settled".to_owned(), false, true)
}

/// A record of the session's, `update` its update.
fn record(update: Value) -> Value {
    json!({ "sessionId": SESSION, "update": update })
}

/// Devin's word on the turn: `working` or `blocked`, stamped as Devin stamps it.
fn status(state: &str) -> Value {
    record(json!({
        "sessionUpdate": "session_info_update",
        "_meta": {
            "cognition.ai/timestamp": 1_792_976_967_738_i64,
            "cognition.ai/eventType": "status_update",
            "cognition.ai/statusEnum": state,
            "cognition.ai/userActionRequired": null,
        },
    }))
}

/// A record of work of the kind `kind`, stamped, in the stream `stream` where there is one.
fn stamped(kind: &str, text: Option<&str>, stream: Option<&str>) -> Value {
    let mut update = json!({
        "sessionUpdate": kind,
        "_meta": { "cognition.ai/timestamp": "2026-10-26T08:18:30.000000+00:00" },
    });
    if let Some(text) = text {
        update["content"] = json!({ "type": "text", "text": text });
    }
    if let Some(stream) = stream {
        update["_meta"]["cognition.ai/streamingMessageId"] = json!(stream);
    }
    record(update)
}

/// What the shell says of the call it runs, with no stamp: no work, whatever its status.
fn running(status: &str) -> Value {
    record(json!({ "sessionUpdate": "tool_call_update", "status": status }))
}

/// The end of the turn as Devin 3000.11 says it, `cause` how it ended.
fn ended(cause: &str) -> Vec<Value> {
    vec![
        status("blocked"),
        json!({ "sessionId": SESSION, "turnClientMessageId": "request-1", "turnRequestId": "model-2" }),
        json!({ "sessionId": SESSION, "cause": cause, "stats": { "requestId": "model-2" } }),
    ]
}

/// The step of the turn that calls the shell.
fn step() -> Value {
    json!({
        "message_id": "a-step",
        "role": "assistant",
        "content": "",
        "tool_calls": [{ "id": "call-1", "name": "exec" }],
        "metadata": { "request_id": "model-1", "finish_reason": "tool_calls" },
    })
}

/// A look that read every step before and a fresh one, which read the same: as [`said`].
fn both(staged: &Staged, reader: &mut Box<dyn Look + Send>, what: &str) -> (String, bool, bool) {
    let incremental = show(&look(reader));
    assert_eq!(
        incremental,
        read_once(staged),
        "{what}: a fresh look reads the same"
    );
    said(&incremental)
}

#[test]
fn a_turn_devin_stamps_is_at_work_from_its_word_that_it_works_through_its_call_to_its_end() {
    let staged = Staged::new();
    let mut wire = Vec::new();
    let mut reader = staged.reader();
    // The user's message is the head, and Devin says it works.
    staged.chain(&[user("u-1", Some(json!("request-1")))]);
    wire.extend([
        status("working"),
        stamped("user_message_chunk", Some("Review T-1"), None),
    ]);
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "begun"), at_work());
    // It thinks, and calls its shell: the call hangs below the head.
    wire.extend([
        stamped("agent_thought_chunk", Some("…"), Some("stream-0")),
        stamped("tool_call", None, None),
    ]);
    staged.wire("launch-1", &wire);
    staged.node(Some("n-2"), Some("n-1"), &step());
    assert_eq!(both(&staged, &mut reader, "calling"), at_work());
    // A minute of the command: the shell says it runs, and nothing else is said.
    wire.extend([running("in_progress"), running("in_progress")]);
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "the command runs"), at_work());
    // The call's answer is the head.
    wire.push(running("completed"));
    staged.wire("launch-1", &wire);
    staged.node(
        Some("n-3"),
        Some("n-2"),
        &json!({ "message_id": "t-1", "role": "tool", "content": "ok", "tool_call_id": "call-1" }),
    );
    staged.head("n-3");
    assert_eq!(both(&staged, &mut reader, "the call answered"), at_work());
    // The answer is stored as the turn's end, and the wire has not said the turn ended.
    wire.push(stamped(
        "agent_message_chunk",
        Some("Done."),
        Some("stream-1"),
    ));
    staged.wire("launch-1", &wire);
    staged.node(Some("n-4"), Some("n-3"), &stopped("a-1", "Done."));
    staged.head("n-4");
    assert_eq!(both(&staged, &mut reader, "answered"), at_work());
    // The turn's end.
    wire.extend(ended("complete"));
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "ended"), settled());
}

#[test]
fn a_streamed_word_of_a_turn_devin_stamps_is_work_with_no_word_of_its_own() {
    let staged = Staged::new();
    staged.chain(&[user("u-1", Some(json!("request-1")))]);
    let mut reader = staged.reader();
    staged.wire(
        "launch-1",
        &[stamped(
            "agent_message_chunk",
            Some("1 2 3"),
            Some("stream-1"),
        )],
    );
    assert_eq!(both(&staged, &mut reader, "streaming"), at_work());
}

#[test]
fn the_turn_a_reopened_window_replays_is_no_work_however_it_is_stamped() {
    let staged = Staged::new();
    staged.chain(&[
        user("u-1", Some(json!("request-1"))),
        stopped("a-1", "Done."),
    ]);
    let mut reader = staged.reader();
    // Devin 3000.11.205's replay (a probe of 2026-10-26): the turn's words and calls, each
    // stamped and in no stream, then the window's setup, and its word that it waits, with
    // no turn's end. A look in the middle of it reads no work.
    let mut wire = vec![
        stamped("user_message_chunk", Some("Review T-1"), None),
        stamped("agent_thought_chunk", Some("…"), None),
        stamped("tool_call", None, None),
        stamped("tool_call_update", None, None),
        stamped("agent_message_chunk", Some("Done."), None),
        record(json!({ "sessionUpdate": "session_info_update", "_meta": {} })),
    ];
    staged.wire("launch-2", &wire);
    assert_eq!(both(&staged, &mut reader, "replaying"), settled());
    wire.push(status("blocked"));
    staged.wire("launch-2", &wire);
    assert_eq!(both(&staged, &mut reader, "replayed"), settled());
}

#[test]
fn a_turn_an_escape_ended_is_over_though_the_command_it_left_goes_on_saying_it_runs() {
    let staged = Staged::new();
    staged.chain(&[user("u-1", Some(json!("request-1")))]);
    let mut reader = staged.reader();
    let mut wire = vec![
        status("working"),
        stamped("tool_call", None, None),
        running("in_progress"),
        stamped("agent_message_chunk", Some("Interrupted"), Some("stream-1")),
    ];
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "at work"), at_work());
    // The cancel ends the turn, its words kept with no end of theirs.
    staged.node(
        Some("n-2"),
        Some("n-1"),
        &reply("a-1", json!("Interrupted")),
    );
    staged.head("n-2");
    wire.extend(ended("cancelled"));
    staged.wire("launch-1", &wire);
    let over = ("settled".to_owned(), false, false);
    assert_eq!(both(&staged, &mut reader, "cancelled"), over);
    // The command the call began goes on saying it runs, stamped or not.
    wire.extend([
        running("in_progress"),
        stamped("tool_call_update", None, None),
    ]);
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "the command runs on"), over);
}

#[test]
fn a_turn_devin_says_waits_for_its_user_is_no_work_until_devin_says_it_works_again() {
    let staged = Staged::new();
    staged.chain(&[user("u-1", Some(json!("request-1")))]);
    let mut reader = staged.reader();
    let mut wire = vec![status("working"), stamped("tool_call", None, None)];
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "at work"), at_work());
    // It waits for its user, with no end of the turn: the record says what the turn is.
    wire.push(status("blocked"));
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "waiting"), settled());
    wire.push(status("working"));
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "at work again"), at_work());
}
