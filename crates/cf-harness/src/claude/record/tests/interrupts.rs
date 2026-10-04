//! An interrupt: a user's record that names the message it cut short and says
//! one of Claude Code's two markers, and nothing else.

use super::*;

const MARKER: &str = "[Request interrupted by user]";

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
fn only_a_marker_alone_on_a_record_that_names_the_interrupted_message_ends_the_turn() {
    let (settled, waiting) = (Settlement::Settled, Settlement::InFlight);
    let tool_use = json!([text(json!("[Request interrupted by user for tool use]"))]);
    let named = |fields: Value| interrupt(fields, marked());
    let said = |content: Value| interrupt(json!({}), content);
    // Node's answer for each record after the answer: an ordinary turn of the
    // user's, or none, unless it is the interrupt.
    let cases = [
        ("the marker", named(json!({})), settled),
        ("the marker for tool use", said(tool_use), settled),
        (
            "no interrupted message",
            lacking(named(json!({})), "interruptedMessageId"),
            waiting,
        ),
        (
            "an interrupted message that is empty",
            named(json!({ "interruptedMessageId": "" })),
            waiting,
        ),
        (
            "an interrupted message that is a number",
            named(json!({ "interruptedMessageId": 5 })),
            waiting,
        ),
        (
            "an interrupted message that is null",
            named(json!({ "interruptedMessageId": null })),
            waiting,
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
