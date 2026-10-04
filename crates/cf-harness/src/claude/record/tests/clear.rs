//! A `/clear`, and each thing that must hold for the output of its command to
//! end the turn: the output is the main conversation's own, it follows the
//! user's turn that is the command, and nothing is open.

use super::*;

/// The user's first turn, saying `content`.
fn typed(content: &str) -> Value {
    user("u1", Value::Null, json!(content))
}

#[test]
fn the_output_of_a_clear_ends_the_turn_when_every_gate_of_it_holds() {
    let clear = || typed(&command("", ""));
    let said = |records: Vec<Value>| settlement_of(&records);
    let (settled, waiting) = (Settlement::Settled, Settlement::InFlight);
    // Node's answer for each: the command's text, then the output's record.
    assert_eq!(said(vec![clear(), output("u1")]), settled);
    let texts = [
        // JavaScript's white space between the tags: a line break, spaces, a no-break space, a byte order mark, a line separator.
        (typed(&command("\n   ", "")), settled),
        (typed(&command("\u{a0}\u{feff}\u{2028}", "")), settled),
        // Not JavaScript's: U+0085.
        (typed(&command("\u{85}", "")), waiting),
        (typed(&command("", "now")), waiting),
        (typed(&format!("{}\n", command("", ""))), waiting),
        (typed(&format!("Please {}", command("", ""))), waiting),
        (
            typed(&command("", "").replace("/clear", "/compact")),
            waiting,
        ),
    ];
    for (turn, settlement) in texts {
        let name = turn["message"]["content"].clone();
        assert_eq!(said(vec![turn, output("u1")]), settlement, "{name}");
    }
    let outputs = [
        (
            having(output("u1"), json!({ "sessionId": "other" })),
            waiting,
        ),
        (
            having(output("u1"), json!({ "isSidechain": true })),
            waiting,
        ),
        (lacking(output("u1"), "isSidechain"), waiting),
        (having(output("u1"), json!({ "isMeta": true })), waiting),
        (lacking(output("u1"), "isMeta"), waiting),
        (having(output("u1"), json!({ "level": "warn" })), waiting),
        (
            having(
                output("u1"),
                json!({ "content": "<local-command-stdout>x</local-command-stdout>" }),
            ),
            waiting,
        ),
        (having(output("u1"), json!({ "subtype": "other" })), waiting),
        (output("u9"), waiting),
        (lacking(output("u1"), "parentUuid"), waiting),
    ];
    for (record, settlement) in outputs {
        assert_eq!(said(vec![clear(), record.clone()]), settlement, "{record}");
    }
}

#[test]
fn a_clear_follows_the_user_turn_that_is_its_command_and_nothing_else() {
    let clear = || typed(&command("", ""));
    let said = |records: Vec<Value>| settlement_of(&records);
    let waiting = Settlement::InFlight;
    let answering = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([text(json!("Hi"))]),
        Value::Null,
    );
    // The turn last pushed is an assistant's, a hook's context, or another user's.
    assert_eq!(said(vec![clear(), answering, output("u1")]), waiting);
    assert_eq!(
        said(vec![
            clear(),
            hook(json!(["ctx"]), Some("h1")),
            output("u1")
        ]),
        waiting
    );
    let then = user("u2", json!("u1"), json!("Then this"));
    assert_eq!(said(vec![clear(), then, output("u1")]), waiting);
}

#[test]
fn a_clear_that_waited_for_a_call_leaves_the_turn_open_when_the_call_is_closed() {
    let calling = assistant(
        "a2",
        "u2",
        json!("m1"),
        json!([tool_use(json!("t1"))]),
        Value::Null,
    );
    let first = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([text(json!("Hi"))]),
        Value::Null,
    );
    let records = [
        typed("Hello"),
        first,
        user("u2", json!("a1"), json!(command("", ""))),
        calling,
        output("u2"),
    ];
    let closing = user(
        "u3",
        json!("a2"),
        json!([tool_result(json!("t1"), json!("x"))]),
    );
    // Node's answer: the output was passed over, and the turn is still open.
    let said = r#"["u1","user","Hello",true,0],["u2","user","<command-name>/clear</command-name><command-message>clear</command-message><command-args></command-args>",true,2],["m1","assistant","Hi",false,3]"#;
    assert_eq!(
        looks(&[&records, &[closing]]),
        [
            in_flight(&format!("[{said}]")),
            in_flight(&format!(r#"[{said},["t1","tool","x",true,5]]"#))
        ]
    );
}

#[test]
fn a_clear_waits_while_the_stop_hooks_or_a_call_of_a_fragment_after_it_are_open() {
    // The user's turn is the last pushed item, as a fragment of a message
    // already pushed pushes none: its stop hooks, or the call it opens, are
    // then what keeps the output of the command from ending the turn.
    let after = |first_stop: Value, fragment: Value| {
        let first = assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([text(json!("Hi"))]),
            first_stop,
        );
        settlement_of(&[
            typed("Hello"),
            first,
            user("u2", json!("a1"), json!(command("", ""))),
            fragment,
            output("u2"),
        ])
    };
    let said = |content: Value, stop: Value| assistant("a2", "u2", json!("m1"), content, stop);
    let more = || json!([text(json!("More"))]);
    assert_eq!(
        after(json!("end_turn"), said(more(), json!("end_turn"))),
        Settlement::InFlight
    );
    assert_eq!(
        after(
            Value::Null,
            said(json!([tool_use(json!("t1"))]), Value::Null)
        ),
        Settlement::InFlight
    );
    assert_eq!(
        after(Value::Null, said(more(), Value::Null)),
        Settlement::Settled
    );
}
