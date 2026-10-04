//! What ends a turn: the boundary records that follow an answer, each at the
//! edge of each of its guards, and the stop reasons that make a message an
//! answer; and what a prompt takes back of the turn before it.

use super::*;

const HELLO_AND_HI: &str = r#"[["u1","user","Hello",true,0],["m1","assistant","Hi","#;

/// A reading of the answer `m1` and the turn's end, the answer complete or
/// not as the end settled it.
fn ended(settles: bool) -> String {
    if settles {
        settled(&format!("{HELLO_AND_HI}true,1]]"))
    } else {
        in_flight(&format!("{HELLO_AND_HI}false,1]]"))
    }
}

/// What a look at the user's turn and its answer reads, then `boundary`.
fn then(boundary: Value) -> String {
    once(&[hello(), answer("a1", "u1"), boundary])
}

/// A field of a record, as the JSON text says it, or left out.
fn set(record: Value, field: &str, value: Option<&str>) -> Value {
    match value {
        Some(text) => having(
            record,
            json!({ field: serde_json::from_str::<Value>(text).unwrap() }),
        ),
        None => lacking(record, field),
    }
}

#[test]
fn a_turn_duration_ends_the_turn_only_when_each_of_its_guards_holds() {
    // Node's answer for each value of each field of an otherwise good record:
    // `Number.isFinite`, `Number.isSafeInteger`, `>= 0`, `count === undefined || count === 0`.
    let guards: &[(&str, Option<&str>, bool)] = &[
        ("durationMs", Some("0"), true),
        ("durationMs", Some("-0"), true),
        ("durationMs", Some("1.5"), true),
        ("durationMs", Some("1e-7"), true),
        ("durationMs", Some("-1"), false),
        ("durationMs", Some("-1e-7"), false),
        ("durationMs", Some(r#""5""#), false),
        ("durationMs", Some("null"), false),
        ("durationMs", Some("true"), false),
        ("durationMs", None, false),
        ("messageCount", Some("0"), true),
        ("messageCount", Some("-0"), true),
        ("messageCount", Some("1e2"), true),
        ("messageCount", Some("100.0"), true),
        ("messageCount", Some("9007199254740991"), true),
        ("messageCount", Some("9007199254740991.0"), true),
        ("messageCount", Some("9007199254740992"), false),
        ("messageCount", Some("9007199254740993"), false),
        ("messageCount", Some("1e300"), false),
        ("messageCount", Some("1.5"), false),
        ("messageCount", Some("-1"), false),
        ("messageCount", Some("-9007199254740991"), false),
        ("messageCount", Some(r#""2""#), false),
        ("messageCount", Some("null"), false),
        ("messageCount", None, false),
        ("pendingBackgroundAgentCount", Some("0"), true),
        ("pendingBackgroundAgentCount", Some("0.0"), true),
        ("pendingBackgroundAgentCount", Some("-0"), true),
        ("pendingBackgroundAgentCount", Some("0e5"), true),
        ("pendingBackgroundAgentCount", None, true),
        ("pendingBackgroundAgentCount", Some("1"), false),
        ("pendingBackgroundAgentCount", Some("-1"), false),
        ("pendingBackgroundAgentCount", Some("1e-7"), false),
        ("pendingBackgroundAgentCount", Some(r#""0""#), false),
        ("pendingBackgroundAgentCount", Some("null"), false),
        ("pendingBackgroundAgentCount", Some("false"), false),
        ("pendingWorkflowCount", Some("0"), true),
        ("pendingWorkflowCount", Some("-0"), true),
        ("pendingWorkflowCount", None, true),
        ("pendingWorkflowCount", Some("1"), false),
        ("pendingWorkflowCount", Some(r#""0""#), false),
        ("pendingWorkflowCount", Some("null"), false),
        ("isSidechain", Some("false"), true),
        ("isSidechain", Some("true"), false),
        ("isSidechain", Some("null"), false),
        ("isSidechain", None, false),
        ("pendingWorkflowCount", Some("[]"), false),
        ("pendingWorkflowCount", Some("{}"), false),
        ("durationMs", Some("[]"), false),
        ("messageCount", Some("[2]"), false),
        ("messageCount", Some("{}"), false),
    ];
    for &(field, value, settles) in guards {
        assert_eq!(
            then(set(duration("d1", "a1"), field, value)),
            ended(settles),
            "{field} {value:?}"
        );
    }
}

#[test]
fn a_message_ends_its_turn_by_stopping_as_end_turn_or_stop_sequence_and_no_other_way() {
    let stopping = |stop: Option<Value>| {
        let said = assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([text(json!("Hi"))]),
            json!("end_turn"),
        );
        in_message(said, "stop_reason", stop)
    };
    // Node's answer for each stop reason, whichever boundary record follows.
    let stops = [
        (Some(json!("end_turn")), true),
        (Some(json!("stop_sequence")), true),
        (Some(json!("max_tokens")), false),
        (Some(json!("tool_use")), false),
        (Some(json!("pause_turn")), false),
        (Some(json!("refusal")), false),
        (Some(json!("END_TURN")), false),
        (Some(json!("")), false),
        (Some(Value::Null), false),
        (Some(json!(5)), false),
        (Some(json!(["end_turn"])), false),
        (None, false),
    ];
    for (stop, settles) in stops {
        for boundary in [duration("d1", "a1"), summary("d1", "a1")] {
            assert_eq!(
                once(&[hello(), stopping(stop.clone()), boundary]),
                ended(settles),
                "{stop:?}"
            );
        }
    }
    // A later fragment of the message that did not stop takes nothing back; another message does.
    let said = |uuid: &str, id: &str, content: &str| {
        assistant(
            uuid,
            "a1",
            json!(id),
            json!([text(json!(content))]),
            Value::Null,
        )
    };
    let sequence = || stopping(Some(json!("stop_sequence")));
    assert_eq!(
        once(&[
            hello(),
            sequence(),
            said("a2", "m1", "More"),
            duration("d1", "a2")
        ]),
        settled(r#"[["u1","user","Hello",true,0],["m1","assistant","Hi\nMore",true,2]]"#)
    );
    assert_eq!(
        once(&[
            hello(),
            sequence(),
            said("a2", "m2", "More"),
            duration("d1", "a2")
        ]),
        in_flight(
            r#"[["u1","user","Hello",true,0],["m1","assistant","Hi",false,1],["m2","assistant","More",false,2]]"#
        )
    );
}

#[test]
fn a_prompt_takes_back_the_answer_and_the_calls_of_the_turn_before_it() {
    let again = || user("u2", json!("a1"), json!("Again"));
    let said = |content: Value, stop: Value, id: &str, uuid: &str| {
        assistant(uuid, "u2", json!(id), content, stop)
    };
    let working = || said(json!([text(json!("Working"))]), Value::Null, "m2", "a2");
    let both = r#"["u1","user","Hello",true,0],["m1","assistant","Hi",false,1],["u2","user","Again",true,2]"#;
    // Node's answer: a boundary after a prompt is not the end of the answer before it.
    let unended = in_flight(&format!(r#"[{both},["m2","assistant","Working",false,3]]"#));
    for boundary in [duration("d2", "a2"), summary("d2", "a2")] {
        let records = [hello(), answer("a1", "u1"), again(), working(), boundary];
        assert_eq!(once(&records), unended);
    }
    assert_eq!(
        once(&[hello(), answer("a1", "u1"), again(), duration("d2", "u2")]),
        in_flight(&format!("[{both}]"))
    );
    // The answer of the new turn is what its boundary ends.
    let done = said(json!([text(json!("Done"))]), json!("end_turn"), "m2", "a2");
    assert_eq!(
        once(&[
            hello(),
            answer("a1", "u1"),
            again(),
            done,
            duration("d2", "a2")
        ]),
        settled(&format!(r#"[{both},["m2","assistant","Done",true,3]]"#))
    );
    // A fragment of the message before the prompt is the message again, placed where it was said last.
    let more = said(json!([text(json!("More"))]), Value::Null, "m1", "a2");
    assert_eq!(
        once(&[
            hello(),
            answer("a1", "u1"),
            again(),
            more,
            duration("d2", "a2")
        ]),
        in_flight(
            r#"[["u1","user","Hello",true,0],["u2","user","Again",true,2],["m1","assistant","Hi\nMore",false,3]]"#
        )
    );
    // Its calls still open are taken back too.
    let calling = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([tool_use(json!("t1"))]),
        Value::Null,
    );
    let done = said(json!([text(json!("Done"))]), json!("end_turn"), "m2", "a2");
    assert_eq!(
        once(&[hello(), calling, again(), done, duration("d2", "a2")]),
        settled(
            r#"[["u1","user","Hello",true,0],["m1","assistant","",false,1],["u2","user","Again",true,2],["m2","assistant","Done",true,3]]"#
        )
    );
}

#[test]
fn a_duration_with_no_answer_or_of_another_subtype_ends_nothing() {
    let waiting = in_flight(r#"[["u1","user","Hello",true,0]]"#);
    assert_eq!(once(&[hello(), duration("d1", "u1")]), waiting);
    let other = having(duration("d1", "a1"), json!({ "subtype": "other" }));
    assert_eq!(then(other), ended(false));
}

#[test]
fn a_stop_hook_summary_ends_the_turn_when_it_did_not_prevent_continuation() {
    let summary_with =
        |field: &str, value: Option<&str>| then(set(summary("d1", "a1"), field, value));
    assert_eq!(then(summary("d1", "a1")), ended(true));
    for prevented in [
        Some("true"),
        Some("null"),
        Some("0"),
        Some(r#""false""#),
        None,
    ] {
        assert_eq!(
            summary_with("preventedContinuation", prevented),
            ended(false),
            "{prevented:?}"
        );
    }
    // Neither the conversation nor the session of the summary is looked at.
    assert_eq!(summary_with("isSidechain", Some("true")), ended(true));
    assert_eq!(summary_with("sessionId", Some(r#""other""#)), ended(true));
}
