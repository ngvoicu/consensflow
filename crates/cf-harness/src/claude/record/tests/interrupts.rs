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

/// The records of a live run (`tests/live/scrub-transcript.mjs` made them), for
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
    let mut stage = Stage::new();
    stage.write(&live(name));
    match &*stage.reader.look(&Options::default(), 0) {
        Reading::Known(record) => record.clone(),
        Reading::Unknown(reason) => panic!("unknown: {reason}"),
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
