//! A turn as Devin 3000.12.43 writes it on the wire (T-1417, three live runs of 2026-11-11
//! on swe-2): its work stamped as 3000.11 stamps it, its words and thoughts in a stream, and
//! no word of Devin's own on the turn, no `session_info_update` with a `statusEnum` at all.
//! The user's message is said back stamped, with its client message and no event id of its
//! own; a call is stamped in no stream; a command past ten seconds goes on in the background
//! while the turn goes on; a dialog that asks for permission is a line of its own with no
//! word that the turn waits; and a reopened window's replay gives each item an event id and
//! ends the turn it replays with a bare `turnClientMessageId`. The texts and ids are made up;
//! the lines' fields are the recorded ones.

use super::stamped::{at_work, both, record, running, settled, stamped};
use super::*;

/// The user's message as Devin 3000.12.43 says it back as its turn begins: stamped, with
/// its client message and no event id.
fn said_back(text: &str) -> Value {
    record(json!({
        "sessionUpdate": "user_message_chunk",
        "content": { "type": "text", "text": text },
        "_meta": {
            "cognition.ai/clientMessageId": "request-1",
            "cognition.ai/eventType": "user_message",
            "cognition.ai/message": text,
            "cognition.ai/messageSubIndex": 0,
            "cognition.ai/timestamp": "2026-11-10T23:06:20.221778+00:00",
        },
    }))
}

/// A call of the turn's shell, stamped and in no stream, as Devin 3000.12.43 writes it.
fn call() -> Value {
    record(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "exec:0#call-1",
        "title": "sleep 45",
        "kind": "execute",
        "_meta": {
            "cognition.ai/inferenceToolName": "exec",
            "cognition.ai/timestamp": "2026-11-10T23:06:24.000000+00:00",
        },
    }))
}

/// The call's command moved to the background past ten seconds: the turn goes on.
fn backgrounded() -> Value {
    record(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "exec:0#call-1",
        "status": "in_progress",
        "_meta": {
            "cognition.ai/inferenceToolName": "exec",
            "cognition.ai/background": true,
            "cognition.ai/backgroundShellId": "shell-1",
            "cognition.ai/backgroundCommand": "sleep 45",
        },
    }))
}

/// The background command's end, which Devin writes when it comes, after the turn's own.
fn exited() -> Value {
    record(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "exec:0#call-1",
        "status": "completed",
        "_meta": {
            "cognition.ai/inferenceToolName": "exec",
            "terminal_exit": { "terminal_id": "shell-1", "exit_code": 0, "signal": null },
        },
    }))
}

/// The dialog that asks the user to allow the call, a line of its own.
fn asked() -> Value {
    json!({
        "sessionId": SESSION,
        "toolCall": {
            "toolCallId": "exec:0#call-1",
            "_meta": { "cognition.ai/editableCommand": "sleep 60" },
        },
        "options": [
            { "optionId": "allow_once", "name": "Allow", "kind": "allow_once" },
            { "optionId": "switch_bypass", "name": "Yes, switch to bypass mode", "kind": "allow_always" },
            { "optionId": "reject_once", "name": "Reject", "kind": "reject_once" },
        ],
    })
}

/// The end of the turn as Devin 3000.12.43 says it: no word that it waits, the turn named,
/// and how it ended.
fn ended(cause: &str) -> Vec<Value> {
    vec![
        json!({ "sessionId": SESSION, "turnClientMessageId": "request-1", "turnRequestId": "model-2",
            "responseDimensions": {} }),
        json!({ "sessionId": SESSION, "cause": cause,
            "stats": { "requestId": "model-2", "ttftMs": 1800 } }),
    ]
}

/// Work replayed by a reopened window, stamped with its event id and in no stream.
fn replayed(kind: &str, text: &str, event: &str) -> Value {
    let mut said = stamped(kind, Some(text), None);
    said["update"]["_meta"]["cognition.ai/eventId"] = json!(event);
    said
}

/// What the session's start hook writes at a launch: a stamped call of a hook.
fn hook() -> Value {
    record(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "hook-1",
        "title": "SessionStart hook",
        "status": "completed",
        "_meta": {
            "cognition.ai/eventType": "hook_execution",
            "cognition.ai/phase": "session_start",
            "cognition.ai/durationMs": 3013,
            "cognition.ai/timestamp": "2026-11-10T23:06:10.000000+00:00",
        },
    }))
}

#[test]
fn a_devin_3000_12_turn_is_at_work_from_its_first_streamed_word_through_its_background_command_to_its_end(
) {
    let staged = Staged::new();
    let mut reader = staged.reader();
    staged.chain(&[user("u-1", Some(json!("request-1")))]);
    let mut wire = vec![hook(), said_back("Review T-1")];
    // It thinks, in a stream, and calls its shell.
    wire.extend([
        stamped("agent_thought_chunk", Some("…"), Some("stream-0")),
        call(),
    ]);
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "calling"), at_work());
    // Past ten seconds the command goes to the background, and the turn goes on.
    wire.extend([running("in_progress"), backgrounded()]);
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "backgrounded"), at_work());
    wire.push(stamped(
        "agent_message_chunk",
        Some("Done."),
        Some("stream-1"),
    ));
    staged.wire("launch-1", &wire);
    staged.node(Some("n-2"), Some("n-1"), &stopped("a-1", "Done."));
    staged.head("n-2");
    assert_eq!(both(&staged, &mut reader, "answered"), at_work());
    wire.extend(ended("complete"));
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "ended"), settled());
    // The background command's end comes after the turn's.
    wire.push(exited());
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "the command ended"), settled());
}

#[test]
fn the_turn_a_reopened_devin_3000_12_window_replays_is_no_work() {
    let staged = Staged::new();
    staged.chain(&[
        user("u-1", Some(json!("request-1"))),
        stopped("a-1", "Done."),
    ]);
    let mut reader = staged.reader();
    // Devin 3000.12.43's replay of a finished turn (run 3, window c): each item with its event
    // id and in no stream, the turn named with no cause, then the start hook.
    let mut user = said_back("Review T-1");
    user["update"]["_meta"]["cognition.ai/eventId"] = json!("3");
    let wire = vec![
        user,
        replayed("agent_thought_chunk", "…", "4"),
        replayed("agent_message_chunk", "Done.", "5"),
        json!({ "sessionId": SESSION, "turnClientMessageId": "request-1" }),
        hook(),
    ];
    staged.wire("launch-2", &wire);
    assert_eq!(both(&staged, &mut reader, "replayed"), settled());
}

#[test]
#[ignore = "red on main: Devin 3000.12.43 writes no working (T-1417)"]
fn a_devin_3000_12_turn_is_at_work_from_the_message_it_says_back_before_its_first_streamed_word() {
    // Devin 3000.11 said `working` before it said the message back; 3000.12.43 says nothing of
    // its own, and the turn's first streamed word came 1-6.6 s after the message in three live
    // runs (T-1417). Read by its stamps, the message said back is a replay's, and the window
    // reads at rest until the model's first word.
    let staged = Staged::new();
    let mut reader = staged.reader();
    staged.chain(&[user("u-1", Some(json!("request-1")))]);
    staged.wire("launch-1", &[hook(), said_back("Review T-1")]);
    assert_eq!(both(&staged, &mut reader, "said back"), at_work());
}

#[test]
#[ignore = "red on main: Devin 3000.12.43 writes no blocked before a permission dialog (T-1417)"]
fn a_devin_3000_12_turn_that_asks_its_user_to_allow_a_call_is_no_work_while_it_waits() {
    // Devin 3000.11 said `blocked` before its dialog, and the window read at rest while it
    // waited; 3000.12.43 writes the dialog as a line of its own with no word that it waits
    // (T-1417, run 3), and the turn's streamed thought before it reads at work for good.
    let staged = Staged::new();
    let mut reader = staged.reader();
    staged.chain(&[user("u-1", Some(json!("request-1")))]);
    let mut wire = vec![
        said_back("Review T-1"),
        stamped("agent_thought_chunk", Some("…"), Some("stream-0")),
        call(),
    ];
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "calling"), at_work());
    wire.push(asked());
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "asking"), settled());
}
