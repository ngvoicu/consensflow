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
fn a_devin_3000_12_turn_is_at_work_from_the_message_it_says_back_before_its_first_streamed_word() {
    // Devin 3000.11 said `working` before it said the message back; 3000.12.43 says nothing of
    // its own, and the turn's first streamed word came 1-6.6 s after the message in three live
    // runs (T-1417). Read by its stamps alone, the message said back is a replay's, and the
    // window read at rest until the model's first word: the message said back live opens the
    // turn (T-1423).
    let staged = Staged::new();
    let mut reader = staged.reader();
    staged.chain(&[user("u-1", Some(json!("request-1")))]);
    staged.wire("launch-1", &[hook(), said_back("Review T-1")]);
    assert_eq!(both(&staged, &mut reader, "said back"), at_work());
}

#[test]
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
    // Allowed, the call goes on (its update, T-1417 run 3), and so does the turn's work.
    wire.push(running_call("in_progress"));
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "allowed"), at_work());
    wire.extend(ended("complete"));
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "ended"), settled());
}

/// What the shell says of the turn's call, by its id, with no stamp.
fn running_call(status: &str) -> Value {
    record(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "exec:0#call-1",
        "status": status,
        "_meta": { "cognition.ai/inferenceToolName": "exec" },
    }))
}

/// The call cancelled by the user's interrupt, as Devin 3000.12.43 says it (T-1423).
fn interrupted() -> Value {
    record(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "exec:0#call-1",
        "status": "failed",
        "content": [{ "type": "content",
            "content": { "type": "text", "text": "Canceled due to user interrupt" } }],
        "_meta": { "cognition.ai/inferenceToolName": "exec", "cognition.ai/canceled": true },
    }))
}

#[test]
fn a_permission_dialog_left_with_escape_ends_the_devin_3000_12_turn() {
    // T-1423 run 4: Escape over the dialog failed the call and cancelled the turn in 0.1 s.
    let staged = Staged::new();
    let mut reader = staged.reader();
    staged.chain(&[user("u-1", Some(json!("request-1")))]);
    let mut wire = vec![said_back("Review T-1"), call(), asked()];
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "asking"), settled());
    wire.push(interrupted());
    wire.push(json!({ "sessionId": SESSION, "requestId": "permission-1" }));
    wire.extend(ended("cancelled"));
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "escaped"), settled());
}

#[test]
fn a_devin_3000_12_turn_the_app_stops_ends_at_its_cancel_whatever_it_was_doing() {
    // T-1423 run 5, the product's Stop over each of three working turns.
    let thinking = vec![
        said_back("Review T-1"),
        stamped("agent_thought_chunk", Some("…"), Some("stream-0")),
    ];
    let mut over_thoughts = thinking.clone();
    over_thoughts.extend([
        json!({ "sessionId": SESSION, "turnClientMessageId": "request-1" }),
        json!({ "cause": "cancelled", "stats": { "requestId": "model-2" }, "sessionId": SESSION }),
        json!({ "durationMs": 4886, "blockIndex": 0, "sessionId": SESSION }),
    ]);
    let mut over_a_command = vec![said_back("Review T-1"), call(), running_call("in_progress")];
    over_a_command.push(interrupted());
    over_a_command.extend(ended("cancelled"));
    // The command's own end, after the turn's.
    over_a_command.push(exited());
    let mut in_the_background = vec![said_back("Review T-1"), call(), backgrounded()];
    in_the_background.extend([interrupted(), running_call("in_progress")]);
    in_the_background.extend(ended("cancelled"));
    for (what, wire) in [
        ("over its thoughts", over_thoughts),
        ("over a command", over_a_command),
        ("over a command in the background", in_the_background),
    ] {
        let staged = Staged::new();
        let mut reader = staged.reader();
        staged.chain(&[user("u-1", Some(json!("request-1")))]);
        staged.wire("launch-1", &thinking);
        assert_eq!(both(&staged, &mut reader, what), at_work(), "{what}");
        staged.wire("launch-1", &wire);
        assert_eq!(both(&staged, &mut reader, what), settled(), "{what}");
    }
}

#[test]
fn a_message_typed_into_a_devin_3000_12_turn_at_work_is_taken_by_it_and_ended_with_it() {
    // T-1417 run 2, T-1423 run 4: a message pasted while the turn works is said back into the
    // turn, and the one cause ends both.
    let staged = Staged::new();
    let mut reader = staged.reader();
    staged.chain(&[user("u-1", Some(json!("request-1")))]);
    let mut wire = vec![
        said_back("Review T-1"),
        stamped("agent_thought_chunk", Some("…"), Some("stream-0")),
        said_back("And T-2"),
    ];
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "both taken"), at_work());
    wire.push(json!({ "sessionId": SESSION, "turnClientMessageId": "request-1" }));
    wire.extend(ended("complete"));
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "ended"), settled());
}

#[test]
fn a_devin_turn_ends_at_a_cause_this_reader_has_no_name_for() {
    // A cause is the turn's end, whatever its name: a word a later Devin adds holds no window at
    // work for good.
    let staged = Staged::new();
    let mut reader = staged.reader();
    staged.chain(&[user("u-1", Some(json!("request-1")))]);
    let mut wire = vec![
        said_back("Review T-1"),
        stamped("agent_thought_chunk", Some("…"), Some("stream-0")),
    ];
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "thinking"), at_work());
    wire.extend(ended("refused"));
    staged.wire("launch-1", &wire);
    assert_eq!(both(&staged, &mut reader, "ended"), settled());
}

#[test]
fn a_message_devin_3000_11_says_back_opens_no_turn_live_or_replayed() {
    // Devin 3000.11 says its messages back with neither an event type nor an event id (127 of
    // 127 in the kit's logs): its turn opens at `working`, and a reopened window's replay of the
    // same line is no work.
    let staged = Staged::new();
    staged.chain(&[
        user("u-1", Some(json!("request-1"))),
        stopped("a-1", "Done."),
    ]);
    let mut reader = staged.reader();
    let old = record(json!({
        "sessionUpdate": "user_message_chunk",
        "content": { "type": "text", "text": "[ConsensFlow m-1 · T-1 · task from @chief]" },
        "_meta": {
            "cognition.ai/clientMessageId": "request-1",
            "cognition.ai/messageSubIndex": 0,
            "cognition.ai/timestamp": 1_792_976_967_738_i64,
        },
    }));
    staged.wire("launch-2", &[old]);
    assert_eq!(both(&staged, &mut reader, "said back"), settled());
}

#[test]
fn the_deliveries_a_devin_turn_took_in_are_its_own_and_none_once_it_ended() {
    // What the window tells the engine has begun (`Observed::begun`): the messages said back
    // into the turn open now, by their headers; none of a turn that ended, and none of a
    // message with no header of ConsensFlow's.
    let mut turns = super::super::wires::Turns::default();
    let said = |turns: &mut super::super::wires::Turns, lines: &[Value]| {
        for line in lines {
            turns.follow(line);
        }
        turns.said_back(SESSION)
    };
    assert_eq!(said(&mut turns, &[]), Vec::<i64>::new());
    assert_eq!(
        said(
            &mut turns,
            &[said_back("[ConsensFlow m-7 · T-1 · task from @chief]\nGo")]
        ),
        vec![7]
    );
    assert_eq!(
        said(
            &mut turns,
            &[
                said_back("[ConsensFlow m-9 · T-1 · note from @chief]\nAnd this"),
                said_back("typed by the human"),
            ]
        ),
        vec![7, 9]
    );
    assert_eq!(said(&mut turns, &ended("complete")), Vec::<i64>::new());
    assert!(!turns.open(SESSION));
    // The next turn took in its own message alone.
    assert_eq!(
        said(
            &mut turns,
            &[said_back(
                "[ConsensFlow m-12 · T-2 · task from @chief]\nNext"
            )]
        ),
        vec![12]
    );
}

#[test]
fn a_message_an_older_devin_says_back_is_its_turns_where_a_turn_opens_around_it_and_a_replays_is_none(
) {
    // Devin 3000.11 says each message back with no event type and no event id, live or replayed
    // (the kit's 34 logs of 2026-10-21 and 2026-11-05, T-1423): just after its `working` where it
    // is taken in live, before the turn's first streamed word in some older logs, and with no
    // turn after it, ended by a bare `turnClientMessageId`, where a reopened window replays it.
    // The message is the turn's where a turn opens around it before the next end; a replay's is
    // dropped at that end, and the next turn does not take it.
    let old = |text: &str| {
        json!({
            "sessionId": SESSION,
            "update": {
                "sessionUpdate": "user_message_chunk",
                "content": { "type": "text", "text": text },
                "_meta": {
                    "cognition.ai/clientMessageId": "request-1",
                    "cognition.ai/timestamp": "2026-11-05T17:01:53.911552+00:00",
                    "cognition.ai/messageSubIndex": 0,
                },
            },
        })
    };
    let working = json!({
        "sessionId": SESSION,
        "update": {
            "sessionUpdate": "session_info_update",
            "_meta": { "cognition.ai/statusEnum": "working" },
        },
    });
    let streamed = stamped("agent_thought_chunk", Some("Reading"), Some("stream-1"));
    let replayed_word = stamped("agent_message_chunk", Some("Done."), None);
    let bare_end = json!({ "sessionId": SESSION, "turnClientMessageId": "request-1" });
    let follow = |turns: &mut super::super::wires::Turns, lines: &[Value]| {
        for line in lines {
            turns.follow(line);
        }
        turns.said_back(SESSION)
    };

    // Live, after `working`: the turn's own until its end.
    let mut turns = super::super::wires::Turns::default();
    let taken = follow(
        &mut turns,
        &[
            working.clone(),
            old("[ConsensFlow m-3 · T-1 · task from @chief]\nGo"),
        ],
    );
    assert_eq!(taken, vec![3], "said back after working");
    assert_eq!(follow(&mut turns, &ended("complete")), Vec::<i64>::new());

    // Live, before the turn's first streamed word (no `working`): the turn's once it streams.
    let mut turns = super::super::wires::Turns::default();
    let said = old("[ConsensFlow m-4 · T-1 · task from @chief]\nGo");
    assert_eq!(
        follow(&mut turns, &[said]),
        Vec::<i64>::new(),
        "no turn open yet"
    );
    assert!(
        !turns.open(SESSION),
        "an older Devin's said-back opens nothing"
    );
    assert_eq!(
        follow(&mut turns, std::slice::from_ref(&streamed)),
        vec![4],
        "its first word"
    );

    // Replayed by a reopened window: no turn, a bare end; the next turn takes its own alone.
    let mut turns = super::super::wires::Turns::default();
    let replay = [
        old("[ConsensFlow m-1 · T-1 · task from @chief]\nGo"),
        replayed_word,
        bare_end,
    ];
    assert_eq!(follow(&mut turns, &replay), Vec::<i64>::new(), "a replay");
    assert!(!turns.open(SESSION), "a replay opens no turn");
    let next = [
        working,
        old("[ConsensFlow m-5 · T-1 · note from ConsensFlow]\nCut"),
    ];
    assert_eq!(follow(&mut turns, &next), vec![5], "the next turn");
}
