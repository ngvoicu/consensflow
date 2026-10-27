//! An interrupt: a user's record, a list of one text block that says one of
//! Claude Code's two markers and nothing else. It need not name the message it
//! interrupted: Claude Code 2.1.292 names none when the interrupt comes while
//! a Stop hook runs (Node's reader asked for one, and read that record as the
//! user's next turn). And the turn the daemon stopped before Claude wrote a
//! word of it, which Claude writes no record of: the reading is as it was,
//! and only the daemon's own press says the turn stopped
//! (`claude::stopped`). Both are read on the records of live runs of
//! `npm run live:stops`, Claude Code 2.1.292.

use super::*;
use crate::shared::record::reading::Role;

const MARKER: &str = "[Request interrupted by user]";

/// The error result Claude writes of a call the user stopped.
const REJECTED: &str =
    "The user doesn't want to proceed with this tool use. The tool use was rejected.";

/// The records of a live run (`cargo xtask scrub-transcript` made them), for
/// the session of these tests.
fn live(name: &str) -> Vec<Value> {
    let text = match name {
        "stopped-before-a-word" => {
            include_str!("../../../../tests/fixtures/claude/stopped-before-a-word.jsonl")
        }
        "stopped-in-the-stop-hook" => {
            include_str!("../../../../tests/fixtures/claude/stopped-in-the-stop-hook.jsonl")
        }
        other => panic!("no fixture {other}"),
    };
    text.replace("$SESSION", SESSION)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// What one look at the records of a live run reads.
fn read(name: &str) -> Record {
    looked_at(&live(name))
}

/// What one look at a transcript of `records` reads.
fn looked_at(records: &[Value]) -> Record {
    let mut stage = Stage::new();
    stage.write(records);
    match &*stage.reader.look(&Options::default(), 0) {
        Reading::Known(record) => record.clone(),
        Reading::Unknown(reason) | Reading::Missing(reason) => panic!("unknown: {reason}"),
    }
}

/// A user's turn after the answer, naming `m1` as the message interrupted,
/// with `fields` set, whose content is `content`.
fn interrupt(fields: Value, content: Value) -> Value {
    let record = having(
        user("u2", json!("a1"), content),
        json!({ "interruptedMessageId": "m1" }),
    );
    having(record, fields)
}

/// The marker as the one text block of a user's turn.
fn marked() -> Value {
    json!([text(json!(MARKER))])
}

#[test]
fn only_a_marker_alone_in_a_list_on_a_user_record_ends_the_turn_whether_or_not_it_names_the_message(
) {
    let (settled, waiting) = (Settlement::Settled, Settlement::InFlight);
    let tool_use = json!([text(json!("[Request interrupted by user for tool use]"))]);
    let named = |fields: Value| interrupt(fields, marked());
    let said = |content: Value| interrupt(json!({}), content);
    // Node's answer for each record after the answer was an ordinary turn of
    // the user's, or none, unless it was the interrupt naming its message.
    // The reading here departs from it where the cases say so.
    let cases = [
        ("the marker", named(json!({})), settled),
        ("the marker for tool use", said(tool_use), settled),
        (
            "no interrupted message (Node: the user's turn)",
            lacking(named(json!({})), "interruptedMessageId"),
            settled,
        ),
        (
            "an interrupted message that is empty (Node: the user's turn)",
            named(json!({ "interruptedMessageId": "" })),
            settled,
        ),
        (
            "an interrupted message that is a number (Node: the user's turn)",
            named(json!({ "interruptedMessageId": 5 })),
            settled,
        ),
        (
            "an interrupted message that is null (Node: the user's turn)",
            named(json!({ "interruptedMessageId": null })),
            settled,
        ),
        (
            "the marker and a line break",
            said(json!([text(json!(format!("{MARKER}\n")))])),
            waiting,
        ),
        (
            "the marker in lower case",
            said(json!([text(json!(MARKER.to_lowercase()))])),
            waiting,
        ),
        (
            "another text",
            said(json!([text(json!("Stop it"))])),
            waiting,
        ),
        (
            "two blocks",
            said(json!([text(json!(MARKER)), text(json!(MARKER))])),
            waiting,
        ),
        ("no block", said(json!([])), waiting),
        ("content that is text", said(json!(MARKER)), waiting),
        (
            "a block of another type",
            said(json!([{ "type": "image", "text": MARKER }])),
            waiting,
        ),
        (
            "a block with no type",
            said(json!([{ "text": MARKER }])),
            waiting,
        ),
        (
            "a block whose text is a list",
            said(json!([text(json!([MARKER]))])),
            waiting,
        ),
        (
            "a message that is no user's",
            having(
                named(json!({})),
                json!({ "message": { "role": "assistant", "content": marked() } }),
            ),
            waiting,
        ),
        (
            "a message with no role",
            having(
                named(json!({})),
                json!({ "message": { "content": marked() } }),
            ),
            waiting,
        ),
        ("no message", lacking(named(json!({})), "message"), waiting),
    ];
    for (name, record, settlement) in cases {
        assert_eq!(
            settlement_of(&[hello(), answer("a1", "u1"), record]),
            settlement,
            "{name}"
        );
    }
}

#[test]
fn a_turn_the_daemon_interrupted_ends_on_the_marker_whether_the_window_had_written_none_of_it_a_part_or_a_call(
) {
    // The window of the paused worker, stopped by the Escape the daemon pressed:
    // the marker is the last record and no answer follows it. It names no message
    // (Claude Code 2.1.292 names none), and the window had written nothing of the
    // turn, a part of its answer, or a call: Claude closes that one with the
    // result of a tool the user stopped (an error), as the transcripts of its
    // real runs show, and then writes the marker for tool use.
    let started = || user("u2", json!("a1"), json!("Parser"));
    let part = || {
        assistant(
            "a2",
            "u2",
            json!("m2"),
            json!([text(json!("Reading the grammar"))]),
            Value::Null,
        )
    };
    let calling = || {
        assistant(
            "a2",
            "u2",
            json!("m2"),
            json!([tool_use(json!("t1"))]),
            json!("tool_use"),
        )
    };
    let refused = || {
        user(
            "u3",
            json!("a2"),
            json!([having(
                tool_result(json!("t1"), json!(REJECTED)),
                json!({ "is_error": true })
            )]),
        )
    };
    let stopped = |parent: &str| user("u4", json!(parent), json!([text(json!(MARKER))]));
    let for_tool_use = |parent: &str| {
        user(
            "u4",
            json!(parent),
            json!([text(json!("[Request interrupted by user for tool use]"))]),
        )
    };
    let cases = [
        ("no word of the answer", vec![stopped("u2")], vec![]),
        ("a part of the answer", vec![part(), stopped("a2")], vec![]),
        (
            "a call it had begun",
            vec![calling(), refused(), for_tool_use("u3")],
            vec![("t1", REJECTED)],
        ),
    ];
    for (name, after, tools) in cases {
        let mut records = vec![hello(), answer("a1", "u1"), started()];
        records.extend(after);
        let record = looked_at(&records);
        assert_eq!(record.settlement, Settlement::Settled, "{name}");
        // The error result is read as the tool's, and closes the call: the marker
        // after it clears what is open, so only the item tells it was read.
        let read: Vec<(&str, &str)> = record
            .items
            .iter()
            .filter(|item| item.role == Role::Tool)
            .map(|item| (&*item.id, &*item.text))
            .collect();
        assert_eq!(read, tools, "{name}");
    }
}

#[test]
fn an_interrupt_in_a_stop_hook_that_names_no_message_ends_the_turn_and_its_answer_is_no_result() {
    // Escape while a Stop hook that ignores signals ran, after the answer
    // `DONE` was written: Claude's record of it names no message.
    let record = read("stopped-in-the-stop-hook");
    assert_eq!(record.settlement, Settlement::Settled);
    assert!(!record.in_flight);
    let items: Vec<(Role, &str, bool)> = record
        .items
        .iter()
        .map(|item| (item.role, &*item.text, item.complete))
        .collect();
    assert_eq!(
        items,
        [
            (
                Role::User,
                "[ConsensFlow m-1 · T-1 · task from @chief]\nReply with exactly one line: DONE",
                true
            ),
            (Role::Assistant, "DONE", false),
            (Role::User, MARKER, true),
        ],
        "the answer is not complete: the interrupt cut the turn, and a result is a complete answer"
    );
}

#[test]
fn a_turn_stopped_before_claude_wrote_a_word_reads_as_it_was_for_the_record_says_nothing_of_the_stop(
) {
    // The daemon paused the task a moment after its brief and pressed Escape;
    // Claude stopped in the hooks of the prompt, wrote no record of it, and
    // put the brief back in its input box. The transcript ends on the brief
    // and what Claude kept beside it: no interrupt, no word of an answer.
    let record = read("stopped-before-a-word");
    assert_eq!(record.settlement, Settlement::InFlight);
    assert!(record.in_flight);
    let items: Vec<(Role, &str)> = record
        .items
        .iter()
        .map(|item| (item.role, &*item.text))
        .collect();
    assert_eq!(
        items,
        [(
            Role::User,
            "[ConsensFlow m-1 · T-1 · task from @chief]\nRun exactly this one shell command, then stop: node -e \"setTimeout(() => {}, 300000)\" cf-stops-live-long-command"
        )],
        "only the daemon's press says this turn stopped (`claude::stopped`)"
    );
}

#[test]
fn an_interrupt_while_a_call_has_no_result_ends_the_turn_and_closes_the_call() {
    // Escape while the advisor answered (a server tool, live on Claude Code
    // 2.1.296, a pause of the task): the call never gets a result, and the
    // interrupt that follows names its message. An ordinary tool's call is
    // closed by the result Claude writes for it; a server tool's is not, and
    // a reading that waited for it read the window at work for good, so the
    // task's next words never went in.
    let thinking = assistant("a1", "u1", json!("m1"), json!([]), Value::Null);
    for call in [
        json!({ "type": "server_tool_use", "id": "t1", "name": "advisor" }),
        tool_use(json!("t1")),
    ] {
        let calling = assistant("a2", "a1", json!("m1"), json!([call]), Value::Null);
        let stop = interrupt(json!({}), marked());
        assert_eq!(
            settlement_of(&[hello(), thinking.clone(), calling.clone(), stop.clone()]),
            Settlement::Settled,
            "{call}"
        );
        // The next prompt begins a turn of its own, which its answer ends.
        let next = user("u3", json!("u2"), json!("continue"));
        let answered = assistant(
            "a3",
            "u3",
            json!("m2"),
            json!([text(json!("Done"))]),
            json!("end_turn"),
        );
        assert_eq!(
            settlement_of(&[
                hello(),
                thinking.clone(),
                calling,
                stop,
                next,
                answered,
                duration("d1", "a3")
            ]),
            Settlement::Settled,
            "{call}"
        );
    }
}
