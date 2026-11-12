//! A conversation the human took back with Devin's "Revert to step", as Devin 3000.12.43
//! writes it (T-1507, two live sessions of 2026-11-12, `E2-relist-try1` and `-try2`; the
//! same on main's `57fecf047`, its `C1-main-revert`). Reverted to step 1, every prompt is gone:
//! the session's `steps` line lists none, a line names the revert's target node and the first
//! message it removed, and `steps` lists none again; then the TUI's settings and mode lines, and
//! no work and no `cause`. The store's chain is Devin's own words alone (its fork then says
//! there is no conversation to fork). Devin itself is at rest. Reverted to step 2, the prompt
//! kept is listed under its own id and its turn is replayed, with event ids, ending in a bare
//! `turnClientMessageId`. The ids and texts are made up; the lines' fields are the recorded
//! ones.

use super::stamped::{both, record, said, settled, stamped};
use super::*;

/// The `steps` line of the session, listing the prompts begun by `messages`.
fn steps(messages: &[&str]) -> Value {
    let listed: Vec<Value> = messages
        .iter()
        .enumerate()
        .map(|(at, message)| {
            json!({ "stepId": format!("step-{at}"), "kind": "prompt", "userMessageId": message,
                "summary": "Review T-1", "revertTargetNodeId": 30 + 4 * at,
                "forkTargetNodeId": 32 + 4 * at })
        })
        .collect();
    json!({ "sessionId": SESSION, "steps": listed })
}

/// The TUI's own lines: its commands, its settings and its mode, and the session's title.
fn settings() -> Vec<Value> {
    vec![
        record(json!({ "sessionUpdate": "available_commands_update", "availableCommands": [] })),
        record(json!({ "sessionUpdate": "config_option_update", "configOptions": [] })),
        record(json!({ "sessionUpdate": "current_mode_update", "currentModeId": "bypass" })),
        record(json!({ "sessionUpdate": "session_info_update", "title": "A review" })),
    ]
}

/// A turn of `message` as 3000.12.43 writes it: the message said back live, its prompt listed
/// new, its thought and words in a stream, and its end.
fn turn(message: &str, listed: &[&str]) -> Vec<Value> {
    vec![
        record(json!({
            "sessionUpdate": "user_message_chunk",
            "content": { "type": "text", "text": "Review T-1" },
            "_meta": {
                "cognition.ai/clientMessageId": message,
                "cognition.ai/eventType": "user_message",
                "cognition.ai/message": "Review T-1",
                "cognition.ai/timestamp": "2026-11-12T00:10:00.000000+00:00",
            },
        })),
        steps(listed),
        stamped("agent_thought_chunk", Some("…"), Some("stream-0")),
        stamped("agent_message_chunk", Some("Done."), Some("stream-1")),
        json!({ "durationMs": 1200, "blockIndex": 0, "sessionId": SESSION }),
        record(json!({ "sessionUpdate": "usage_update", "used": 1000, "size": 200_000 })),
        steps(listed),
        json!({ "sessionId": SESSION, "turnClientMessageId": message, "turnRequestId": "model-1",
            "responseDimensions": {} }),
        json!({ "sessionId": SESSION, "cause": "complete", "stats": { "requestId": "model-1" } }),
    ]
}

/// Devin's "Revert to step `step`" as the wire says it: the steps left, the revert's line, the
/// steps again, the TUI's lines.
fn reverted(left: &[&str], removed: &str) -> Vec<Value> {
    let mut lines = vec![
        steps(left),
        json!({ "sessionId": SESSION, "revertTargetNodeId": 30, "firstRemovedUserMessageId": removed }),
        record(json!({ "sessionUpdate": "session_info_update", "title": "A review" })),
        steps(left),
    ];
    lines.extend(settings());
    lines
}

/// Devin's own words, which a chain begins with.
fn own(id: &str) -> Value {
    json!({ "message_id": id, "role": "system", "content": "Devin's own." })
}

#[test]
fn a_devin_conversation_reverted_to_its_first_step_is_at_rest() {
    let staged = Staged::new();
    let mut reader = staged.reader();
    staged.chain(&[
        own("p-1"),
        own("p-2"),
        user("u-1", Some(json!("request-1"))),
        stopped("a-1", "Done."),
        user("u-2", Some(json!("request-2"))),
        stopped("a-2", "Done."),
    ]);
    let mut wire = vec![steps(&[])];
    wire.extend(settings());
    wire.extend(turn("request-1", &["request-1"]));
    wire.extend(turn("request-2", &["request-1", "request-2"]));
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "two turns answered"), settled());

    // Reverted to step 1: no prompt is left, the chain is Devin's own words, and nothing works.
    wire.extend(reverted(&[], "request-1"));
    staged.wire("launch-1", &wire);
    staged.node(Some("n-r"), Some("n-2"), &own("p-3"));
    staged.head("n-r");
    let reading = both(&staged, &mut reader, "reverted to step 1");
    assert_eq!(
        reading.0, "settled",
        "a conversation Devin says has no prompt left, with no turn open, is at rest: {reading:?}"
    );
    assert!(!reading.1, "nothing is in flight");

    // The next message opens its turn as in any conversation, and its end settles it.
    let mut next = turn("request-3", &["request-3"]);
    let end = next.split_off(next.len() - 2);
    wire.extend(next);
    staged.wire("launch-1", &wire);
    staged.node(
        Some("n-u3"),
        Some("n-r"),
        &user("u-3", Some(json!("request-3"))),
    );
    staged.node(Some("n-a3"), Some("n-u3"), &stopped("a-3", "Done."));
    staged.head("n-a3");
    let reading = both(&staged, &mut reader, "the next turn at work");
    assert_eq!((reading.0.as_str(), reading.1), ("in-flight", true));
    wire.extend(end);
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "the next turn ended"), settled());
}

#[test]
fn a_devin_conversation_reverted_to_a_later_step_reads_as_before() {
    // Reverted to step 2, the first prompt is kept under its own id and its turn replayed: at
    // rest by its answer, as on main (the live control of `E2-relist-try2`).
    let staged = Staged::new();
    let mut reader = staged.reader();
    staged.chain(&[
        own("p-1"),
        own("p-2"),
        user("u-1", Some(json!("request-1"))),
        stopped("a-1", "Done."),
        user("u-2", Some(json!("request-2"))),
        stopped("a-2", "Done."),
    ]);
    let mut wire = vec![steps(&[])];
    wire.extend(settings());
    wire.extend(turn("request-1", &["request-1"]));
    wire.extend(turn("request-2", &["request-1", "request-2"]));
    wire.extend(reverted(&["request-1"], "request-2"));
    let mut replayed = stamped("agent_message_chunk", Some("Done."), None);
    replayed["update"]["_meta"]["cognition.ai/eventId"] = json!("34");
    wire.extend([
        replayed,
        json!({ "sessionId": SESSION, "turnClientMessageId": "request-1" }),
    ]);
    staged.wire("launch-1", &wire);
    staged.node(Some("n-r"), Some("n-4"), &own("p-3"));
    staged.head("n-r");
    assert_eq!(both(&staged, &mut reader, "reverted to step 2"), settled());
}

#[test]
fn a_new_devin_conversation_that_lists_no_prompt_yet_reads_as_before() {
    // A window's first `steps` line lists what it found, here none: no revert, and a chain of
    // Devin's own words says nothing of a turn, as before (T-1388).
    let staged = Staged::new();
    staged.chain(&[own("p-1"), own("p-2")]);
    let mut wire = vec![steps(&[])];
    wire.extend(settings());
    wire.push(steps(&[]));
    staged.wire("launch-1", &wire);
    assert_eq!(
        said(&read_once(&staged)),
        ("unknown".to_owned(), false, true)
    );
}

#[test]
fn a_devin_turn_open_when_its_steps_list_no_prompt_is_open_until_its_end() {
    // No Revert is begun inside a turn on any recording; were the log to list no prompt while a
    // turn waits on its user's answer to a permission dialog (open, not at work) with nothing of
    // the turn on the chain yet, the turn holds the window until its own end, as any turn does.
    let staged = Staged::new();
    let mut reader = staged.reader();
    staged.chain(&[own("p-1"), own("p-2")]);
    let mut wire = vec![steps(&[])];
    let mut opening = turn("request-1", &["request-1"]);
    opening.truncate(3);
    wire.extend(opening);
    wire.push(json!({ "sessionId": SESSION,
        "toolCall": { "toolCallId": "exec:0#call-1" },
        "options": [{ "optionId": "allow_once", "name": "Allow", "kind": "allow_once" }] }));
    wire.push(steps(&[]));
    staged.wire("launch-1", &wire);
    let reading = both(&staged, &mut reader, "a turn waiting, no prompt listed");
    assert_ne!(
        reading.0, "settled",
        "the open turn holds the window: {reading:?}"
    );
    wire.push(json!({ "sessionId": SESSION, "cause": "cancelled", "stats": {} }));
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "the turn ended").0, "settled");
}
