//! A turn Devin's async compactor compacted before its answer (T-652, the live board's
//! T-602 of 2026-10-26): a system message that sums the conversation up begins a chain
//! of its own, the turn's steps are copied below it with the ids Devin gave them, and
//! the user's message the turn answers is left outside the main chain. The answer is
//! stored as Devin 3000.11 stores one, with no `finish_reason`, in a model request of
//! its own (`metadata.request_id`); the wire names that request as the turn ends, in the
//! record that names the turn's message (`turnRequestId`) and in the end's statistics
//! (`stats.requestId`). Only that request ties the answer to its end, and only where all
//! three name it; a look that read the turn before the compaction reads the same as a
//! fresh one.

use super::*;

/// The model's request the answer is of.
const ANSWERED: &str = "model-2";

/// The turn before the compaction: the user's message, a step that calls a tool, and
/// the tool's answer, with the answer on its way on the wire.
fn before(staged: &Staged) {
    staged.node(Some("n-1"), None, &user("u-1", Some(json!("request-1"))));
    staged.node(Some("n-2"), Some("n-1"), &step());
    staged.node(Some("n-3"), Some("n-2"), &tool());
    staged.head("n-3");
    staged.wire("launch-1", &[chunk(Some(json!("Done.")), "stream-1")]);
}

/// A step of the turn: the assistant's call of its shell, in a model request of its own.
fn step() -> Value {
    json!({
        "message_id": "a-step",
        "role": "assistant",
        "content": "",
        "tool_calls": [{ "id": "call-1", "name": "exec" }],
        "metadata": { "request_id": "model-1", "finish_reason": null },
    })
}

fn tool() -> Value {
    json!({ "message_id": "t-step", "role": "tool", "content": "ok", "tool_call_id": "call-1" })
}

/// The answer `text` as Devin 3000.11 stores one, in the model request `request`.
fn answer(id: &str, text: &str, request: &str) -> Value {
    json!({
        "message_id": id,
        "role": "assistant",
        "content": text,
        "metadata": { "request_id": request, "finish_reason": null },
    })
}

/// The compaction, and the answer below the copied steps, the main chain's head.
fn compacted(staged: &Staged, answered: &Value) {
    let summary = json!({
        "message_id": "s-1",
        "role": "system",
        "content": "The conversation so far.",
        "metadata": { "extensions": { "devin-rs/summary": { "source": "async_file_compactor", "kept_turns": 2 } } },
    });
    staged.node(Some("c-1"), None, &summary);
    staged.node(Some("c-2"), Some("c-1"), &step());
    staged.node(Some("c-3"), Some("c-2"), &tool());
    staged.node(Some("c-4"), Some("c-3"), answered);
    staged.head("c-4");
}

/// The end of the turn as Devin 3000.11 says it: the record that names its message
/// and, where `turn`, its model request, then the cause, with `stats` its request.
fn ended(cause: &str, turn: Option<&str>, stats: Option<&str>) -> Vec<Value> {
    let mut named = json!({ "sessionId": SESSION, "turnClientMessageId": "request-1" });
    if let Some(turn) = turn {
        named["turnRequestId"] = json!(turn);
    }
    let mut end = json!({ "sessionId": SESSION, "cause": cause, "stats": {} });
    if let Some(stats) = stats {
        end["stats"]["requestId"] = json!(stats);
    }
    vec![named, end]
}

/// The wire of the turn: the answer streamed, then `then`.
fn wired(staged: &Staged, then: &[Value]) {
    let mut events = vec![chunk(Some(json!("Done.")), "stream-1")];
    events.extend_from_slice(then);
    staged.wire("launch-1", &events);
}

/// What a reading says of the turn: its settlement, whether it is in flight, and
/// whether its last item is complete.
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

/// What a look that read the turn before the compaction reads once `after` wrote the
/// rest, and what a fresh look reads: each as [`said`] says it, the same.
fn read_after(after: impl Fn(&Staged)) -> (String, bool, bool) {
    let staged = Staged::new();
    before(&staged);
    let mut reader = staged.reader();
    assert_eq!(
        said(&show(&look(&mut reader))),
        ("in-flight".to_owned(), true, true),
        "the turn at work before the compaction"
    );
    after(&staged);
    let incremental = said(&show(&look(&mut reader)));
    let fresh = said(&read_once(&staged));
    assert_eq!(
        incremental, fresh,
        "a look that read the turn before the compaction"
    );
    fresh
}

fn settled() -> (String, bool, bool) {
    ("settled".to_owned(), false, true)
}

fn unknown() -> (String, bool, bool) {
    ("unknown".to_owned(), true, false)
}

#[test]
fn an_answer_a_compaction_cut_from_its_users_message_is_complete_by_the_model_request_the_wire_ended(
) {
    assert_eq!(
        read_after(|staged| {
            compacted(staged, &answer("a-final", "Done.", ANSWERED));
            wired(staged, &ended("complete", Some(ANSWERED), Some(ANSWERED)));
        }),
        settled()
    );
}

#[test]
fn the_end_of_a_compacted_turn_that_names_no_model_request_or_two_proves_nothing() {
    for (turn, stats) in [
        (None, Some(ANSWERED)),
        (Some(ANSWERED), None),
        (None, None),
        (Some(ANSWERED), Some("model-9")),
        (Some("model-9"), Some(ANSWERED)),
        (Some(""), Some("")),
    ] {
        assert_eq!(
            read_after(|staged| {
                compacted(staged, &answer("a-final", "Done.", ANSWERED));
                wired(staged, &ended("complete", turn, stats));
            }),
            unknown(),
            "the turn's request {turn:?}, the end's {stats:?}"
        );
    }
}

#[test]
fn a_compacted_answer_of_another_model_request_is_not_completed_by_the_same_text() {
    // The wire ended `model-9` with the same words: the stored answer is of `model-2`.
    assert_eq!(
        read_after(|staged| {
            compacted(staged, &answer("a-final", "Done.", ANSWERED));
            wired(staged, &ended("complete", Some("model-9"), Some("model-9")));
        }),
        unknown()
    );
    // Nor is an answer with no request of its own.
    assert_eq!(
        read_after(|staged| {
            compacted(staged, &reply("a-final", json!("Done.")));
            wired(staged, &ended("complete", Some(ANSWERED), Some(ANSWERED)));
        }),
        unknown()
    );
    // Nor one whose words are not the ones the wire streamed.
    assert_eq!(
        read_after(|staged| {
            compacted(staged, &answer("a-final", "Done, and more.", ANSWERED));
            wired(staged, &ended("complete", Some(ANSWERED), Some(ANSWERED)));
        }),
        unknown()
    );
}

#[test]
fn work_after_a_compacted_turn_ended_keeps_it_in_flight() {
    // The agent thinks again on the wire.
    let thought = json!({
        "sessionId": SESSION,
        "update": { "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "…" } },
    });
    let mut then = ended("complete", Some(ANSWERED), Some(ANSWERED));
    then.push(thought);
    assert_eq!(
        read_after(|staged| {
            compacted(staged, &answer("a-final", "Done.", ANSWERED));
            wired(staged, &then);
        }),
        ("in-flight".to_owned(), true, true)
    );
    // The store goes on below the answer, in a model request the wire never ended.
    assert_eq!(
        read_after(|staged| {
            compacted(staged, &answer("a-final", "Done.", ANSWERED));
            staged.node(
                Some("c-5"),
                Some("c-4"),
                &answer("a-next", "More", "model-3"),
            );
            staged.head("c-5");
            wired(staged, &ended("complete", Some(ANSWERED), Some(ANSWERED)));
        }),
        unknown()
    );
}

#[test]
fn a_compacted_turn_an_escape_cancelled_is_over_by_the_model_request_the_wire_ended() {
    // What a stop's look reads: the turn is over, its answer not whole.
    assert_eq!(
        read_after(|staged| {
            compacted(staged, &answer("a-final", "Do", ANSWERED));
            wired(staged, &ended("cancelled", Some(ANSWERED), Some(ANSWERED)));
        }),
        ("settled".to_owned(), false, false)
    );
}
