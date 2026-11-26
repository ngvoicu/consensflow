//! A Devin turn that ends in a named error is a failed turn whatever it said before: thoughts
//! and tool calls with no answer text yet bind the request's ids and the error end as much as
//! a turn that had begun its answer (T-2075's blocker 3: N35 never retried a Devin turn that
//! failed before its first assistant word).

use cf_harness::records::Reading;
use serde_json::{json, Value};

use super::fixtures::*;

const SESSION: &str = "review-session";

/// Whether the record reads the turn failed, after `before` said first (the wire's lines
/// before the turn's ids and its end).
fn failed_after(before: &[Value]) -> (bool, Option<String>) {
    failed_on(before, false)
}

/// The same, where the turn's ids and its end are one line (what the rig's stand-in writes).
fn failed_on(before: &[Value], one_line: bool) -> (bool, Option<String>) {
    let (failed, said, _) = look_after(before, one_line);
    (failed, said)
}

/// What the look reads: failed, why, and whether the window is at rest.
fn look_after(before: &[Value], one_line: bool) -> (bool, Option<String>, bool) {
    let home = Home::new();
    let store = home.store();
    store.begins(SESSION);
    store.writes(
        SESSION,
        "1",
        None,
        &json!({"message_id":"u","role":"user","content":"brief","metadata":{"extensions":{"chisel/client-message-id":"req"}}}),
    );
    let window = window(&home.adapter(), &Request::resumed(SESSION));
    home.selects(SESSION);
    let mut wire: Vec<Value> = before.to_vec();
    if one_line {
        wire.push(json!({"sessionId":SESSION,"turnClientMessageId":"req","cause":"error","errorMessage":"The agent crashed: I/O error"}));
    } else {
        wire.push(
            json!({"sessionId":SESSION,"turnClientMessageId":"req","turnRequestId":"model-req"}),
        );
        wire.push(json!({"sessionId":SESSION,"cause":"error","errorMessage":"The agent crashed: I/O error","stats":{"requestId":"model-req"}}));
    }
    home.appends(
        &wire
            .iter()
            .map(|event| format!("{event}\n"))
            .collect::<String>(),
    );
    let observed = cf_harness::testing::finished(window.observe()).expect("a look");
    let Some(Reading::Known(record)) = observed.reading.as_deref() else {
        panic!("the record is unreadable");
    };
    (record.failed, record.failure.clone(), observed.settled)
}

fn thought() -> Value {
    json!({"sessionId":SESSION,"update":{"sessionUpdate":"agent_thought_chunk","_meta":{"cognition.ai/streamingMessageId":"thought"},"content":{"type":"text","text":"thinking"}}})
}

fn answer_text() -> Value {
    json!({"sessionId":SESSION,"update":{"sessionUpdate":"agent_message_chunk","_meta":{"cognition.ai/streamingMessageId":"words"},"content":{"type":"text","text":"Looking"}}})
}

fn tool() -> Value {
    json!({"sessionId":SESSION,"update":{"sessionUpdate":"tool_call","toolCallId":"call-1","title":"Read","_meta":{"cognition.ai/streamingMessageId":"tool"}}})
}

#[test]
fn a_named_error_after_answer_text_is_a_failed_turn() {
    let (failed, _) = failed_after(&[thought(), answer_text()]);
    assert!(failed);
}

#[test]
fn a_named_error_after_thoughts_alone_is_a_failed_turn() {
    let (failed, said) = failed_after(&[thought()]);
    assert!(failed, "a turn of thoughts that ended in an error failed");
    assert!(said.is_some_and(|said| said.contains("I/O error")));
}

#[test]
fn a_named_error_after_thoughts_and_a_tool_call_is_a_failed_turn() {
    let (failed, _) = failed_after(&[thought(), tool()]);
    assert!(failed);
}

fn said_back() -> Value {
    json!({"sessionId":SESSION,"update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"brief"},"_meta":{"cognition.ai/clientMessageId":"req","cognition.ai/messageSubIndex":0,"cognition.ai/eventType":"user_message","cognition.ai/message":"brief"}}})
}

#[test]
fn a_named_error_on_one_line_after_the_message_said_back_is_a_failed_turn() {
    let (failed, said) = failed_on(&[said_back()], true);
    assert!(failed, "the turn the message opened ended in an error");
    assert!(said.is_some_and(|said| said.contains("I/O error")));
}

#[test]
fn a_window_whose_turn_ended_in_an_error_reads_at_rest_to_take_the_retry() {
    for one_line in [false, true] {
        let before = if one_line {
            vec![said_back()]
        } else {
            vec![thought()]
        };
        let (failed, _, settled) = look_after(&before, one_line);
        assert!(failed);
        assert!(
            settled,
            "the window is at rest after the turn's error end (one line: {one_line})"
        );
    }
}

fn status(state: &str) -> Value {
    json!({"sessionId":SESSION,"update":{"sessionUpdate":"session_info_update","_meta":{"cognition.ai/timestamp":1_790_000_000_000_i64,"cognition.ai/eventType":"status_update","cognition.ai/statusEnum":state,"cognition.ai/userActionRequired":null}}})
}

fn said_back_stamped() -> Value {
    json!({"sessionId":SESSION,"update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"brief"},"_meta":{"cognition.ai/clientMessageId":"req","cognition.ai/messageSubIndex":0}}})
}

#[test]
fn a_stamped_wire_with_working_and_blocked_around_the_error_reads_failed_and_at_rest() {
    let (failed, said, settled) = look_after(
        &[status("working"), said_back_stamped(), status("blocked")],
        true,
    );
    assert!(failed);
    assert!(said.is_some_and(|said| said.contains("I/O error")));
    assert!(settled, "the window is at rest after its error end");
}
