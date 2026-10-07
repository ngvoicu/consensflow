//! The ids a record must have to say anything, and the texts JavaScript
//! makes of what a record holds, where V8 threw and where it did not.

use super::*;

/// V8 threw a TypeError on `String()` of an object with a `toString` of its
/// own (`Cannot convert object to primitive value`): the look is unreadable
/// for these words.
const NOT_TEXT: &str =
    "unknown: unreadable: an object with a toString of its own cannot be made text";

const HELLO_AND_HI: &str = r#"[["u1","user","Hello",true,0],["m1","assistant","Hi",true,2]]"#;

#[test]
fn an_assistant_record_is_asked_for_its_ids_even_when_it_says_nothing() {
    let silent = assistant("a1", "u1", json!("m1"), json!([]), Value::Null);
    assert_eq!(
        once(&[hello(), lacking(silent, "uuid")]),
        "unknown: unreadable: missing native claude record id at record 1"
    );
    // The message's id first: a record that lacks both says the message's.
    let said = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([text(json!("Hi"))]),
        Value::Null,
    );
    for id in [None, Some(json!(5)), Some(json!(""))] {
        let record = lacking(in_message(said.clone(), "id", id), "uuid");
        assert_eq!(
            once(&[hello(), record]),
            "unknown: unreadable: missing native claude message id at record 1"
        );
    }
}

#[test]
fn a_hook_context_is_asked_for_its_id_only_when_it_says_something() {
    let only_hello = in_flight(r#"[["u1","user","Hello",true,0]]"#);
    for nothing in [json!([]), json!([5, null, {}])] {
        assert_eq!(once(&[hello(), hook(nothing, None)]), only_hello);
    }
    assert_eq!(
        once(&[hello(), hook(json!(["ctx"]), None)]),
        "unknown: unreadable: missing native claude hook context id at record 1"
    );
    assert_eq!(
        once(&[hello(), hook(json!(["a", 5, "b"]), Some("h1"))]),
        in_flight(r#"[["u1","user","Hello",true,0],["h1","custom","a\nb",true,1]]"#)
    );
}

#[test]
fn a_user_turn_of_javascript_s_white_space_alone_is_no_turn() {
    let nothing = r#"{"items":[],"inFlight":false,"asking":false,"failed":false,"quota":null,"settlement":"unknown"}"#;
    // Node's answer for each: `trim()` takes U+FEFF and not U+0085, U+200B or U+180E.
    let turns: &[(&str, bool)] = &[
        ("", false),
        (" \n\t", false),
        ("\u{a0}", false),
        ("\u{feff}", false),
        ("\u{2028}\u{2029}", false),
        ("\u{1680}", false),
        ("\u{3000}", false),
        ("\u{2000}\u{200a}", false),
        ("\t\u{b}\u{c}", false),
        ("\u{85}", true),
        ("\u{200b}", true),
        ("\u{180e}", true),
        ("\u{feff} x \u{feff}", true),
        ("x", true),
    ];
    let said = |text: &str| {
        in_flight(&format!(
            r#"[["u1","user",{},true,0]]"#,
            js::stringify(&json!(text))
        ))
    };
    for &(spaced, is_turn) in turns {
        let expected = if is_turn {
            said(spaced)
        } else {
            nothing.to_owned()
        };
        assert_eq!(
            once(&[user("u1", Value::Null, json!(spaced))]),
            expected,
            "{spaced:?}"
        );
    }
    // Of text blocks, joined a line each.
    for &(spaced, is_turn) in &turns[..6] {
        let blocks = json!([text(json!(spaced)), text(json!(spaced))]);
        let expected = if is_turn {
            said(&format!("{spaced}\n{spaced}"))
        } else {
            nothing.to_owned()
        };
        assert_eq!(
            once(&[user("u1", Value::Null, blocks)]),
            expected,
            "{spaced:?}"
        );
    }
}

#[test]
fn a_hook_context_that_is_not_the_conversation_s_own_adds_nothing() {
    let context = || hook(json!(["ctx"]), Some("h1"));
    assert_eq!(
        once(&[hello(), context()]),
        in_flight(r#"[["u1","user","Hello",true,0],["h1","custom","ctx",true,1]]"#)
    );
    // Node's answer for each: the user's turn alone. Another session, a
    // sidechain or none, another event or none, another kind of attachment,
    // content that is no list, and no attachment at all.
    let others = [
        having(context(), json!({ "sessionId": "other" })),
        lacking(context(), "sessionId"),
        having(context(), json!({ "isSidechain": true })),
        lacking(context(), "isSidechain"),
        having(context(), json!({ "isSidechain": null })),
        in_attachment(context(), "hookEvent", Some(json!("Stop"))),
        in_attachment(context(), "hookEvent", None),
        in_attachment(context(), "type", Some(json!("other"))),
        in_attachment(context(), "content", Some(json!("ctx"))),
        in_attachment(context(), "content", None),
        lacking(context(), "attachment"),
        having(
            context(),
            json!({ "attachment": "hook_additional_context" }),
        ),
    ];
    for other in others {
        assert_eq!(
            once(&[hello(), other.clone()]),
            in_flight(r#"[["u1","user","Hello",true,0]]"#),
            "{other}"
        );
    }
}

#[test]
fn a_user_record_an_interrupt_and_a_tool_result_need_their_ids() {
    assert_eq!(
        once(&[lacking(hello(), "uuid")]),
        "unknown: unreadable: missing native claude user id at record 0"
    );
    let interrupt = user(
        "u2",
        json!("a1"),
        json!([text(json!("[Request interrupted by user]"))]),
    );
    assert_eq!(
        once(&[hello(), answer("a1", "u1"), lacking(interrupt, "uuid")]),
        "unknown: unreadable: missing native claude user id at record 2"
    );
    let calling = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([tool_use(json!("t1"))]),
        Value::Null,
    );
    let result = |call: Value| user("u2", json!("a1"), json!([tool_result(call, json!("x"))]));
    assert_eq!(
        once(&[hello(), calling.clone(), result(json!(7))]),
        "unknown: unreadable: missing native claude tool result id at record 2"
    );
    // A call named by nothing, an empty text, 0 or null is no call: passed over.
    let nameless = user(
        "u2",
        json!("a1"),
        json!([
            tool_result(json!(""), json!("x")),
            tool_result(json!(0), json!("y")),
            tool_result(Value::Null, json!("z"))
        ]),
    );
    assert_eq!(
        once(&[hello(), calling, nameless]),
        in_flight(r#"[["u1","user","Hello",true,0],["m1","assistant","",false,1]]"#)
    );
}

#[test]
fn a_record_with_two_causes_of_failure_reports_the_one_javascript_met_first() {
    let calling = || {
        assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([tool_use(json!("t1"))]),
            Value::Null,
        )
    };
    let bad = || json!([text(json!({ "toString": null }))]);
    let user_of = |blocks: Value| user("u2", json!("a1"), blocks);
    // Node's answer for each: a tool result's text is made before its call's id is asked for.
    assert_eq!(
        once(&[
            hello(),
            calling(),
            user_of(json!([tool_result(json!(7), bad())]))
        ]),
        NOT_TEXT
    );
    let in_assistant = assistant(
        "a2",
        "u1",
        json!("m1"),
        json!([tool_result(json!(7), bad())]),
        Value::Null,
    );
    assert_eq!(once(&[hello(), in_assistant]), NOT_TEXT);
    // A call that is named by nothing is passed over, text and all.
    assert_eq!(
        once(&[
            hello(),
            calling(),
            user_of(json!([tool_result(Value::Null, bad())]))
        ]),
        in_flight(r#"[["u1","user","Hello",true,0],["m1","assistant","",false,1]]"#)
    );
    // An assistant's message id is asked for before the result's text is made.
    let no_message = in_message(
        assistant(
            "a2",
            "u1",
            json!("m1"),
            json!([tool_result(json!("t1"), bad())]),
            Value::Null,
        ),
        "id",
        None,
    );
    assert_eq!(
        once(&[hello(), lacking(no_message, "uuid")]),
        "unknown: unreadable: missing native claude message id at record 1"
    );
    // A user's tool results are read before the user's own id is asked for.
    let blocks =
        |call: Value, content: Value| json!([tool_result(call, content), text(json!("and more"))]);
    assert_eq!(
        once(&[
            hello(),
            calling(),
            lacking(user_of(blocks(json!("t1"), bad())), "uuid")
        ]),
        NOT_TEXT
    );
    let numbered = blocks(json!(7), json!("x"));
    assert_eq!(
        once(&[
            hello(),
            calling(),
            lacking(user_of(numbered.clone()), "uuid")
        ]),
        "unknown: unreadable: missing native claude tool result id at record 2"
    );
    let with_its_uuid = user_of(json!([tool_result(json!(7), json!("x"))]));
    assert_eq!(
        once(&[hello(), calling(), with_its_uuid]),
        "unknown: unreadable: missing native claude tool result id at record 2"
    );
}

#[test]
fn a_record_that_is_null_and_a_transcript_of_no_records_say_why_they_are_unreadable() {
    // Node: `Cannot read properties of null (reading 'uuid')`.
    assert_eq!(
        once(&[hello(), Value::Null]),
        "unknown: unreadable: record 1 is null, where an object was read"
    );
    assert_eq!(once(&[]), "unknown: unreadable: empty claude session s1");
}

#[test]
fn a_queued_message_is_the_string_javascript_makes_of_its_content() {
    let queued = |content: Value, named: Value| {
        once(&[
            hello(),
            queue("enqueue", content),
            answer("a1", "u1"),
            queue("remove", named),
            duration("d1", "a1"),
        ])
    };
    let hi = || settled(HELLO_AND_HI);
    let waiting = in_flight(HELLO_AND_HI);
    // A list is the text its items make, null as nothing, as `join` makes it.
    assert_eq!(queued(json!("1,2"), json!([1, [2]])), hi());
    assert_eq!(queued(json!("1,2"), json!([1, [3]])), waiting);
    assert_eq!(queued(json!("1,"), json!([1, null])), hi());
    // Any object is `[object Object]`, even one with a `valueOf` of its own.
    assert_eq!(queued(json!({ "valueOf": 1 }), json!({})), hi());
    assert_eq!(queued(json!("[object Object]"), json!({ "a": 1 })), hi());
    assert_eq!(queued(json!(true), json!("true")), hi());
    assert_eq!(queued(json!(1.5), json!("1.5")), hi());
    // Nothing and null are no text at all: the empty one.
    assert_eq!(queued(Value::Null, json!("")), hi());
    assert_eq!(queued(Value::Null, Value::Null), hi());
    assert_eq!(queued(json!(""), json!([])), hi());
}

#[test]
fn a_queue_operation_whose_content_is_an_object_with_a_to_string_fails_the_look() {
    // Node: TypeError, `Cannot convert object to primitive value`.
    for operation in ["enqueue", "popAll", "remove"] {
        for content in [json!({ "toString": 1 }), json!([[{ "toString": null }]])] {
            assert_eq!(
                once(&[hello(), queue(operation, content.clone())]),
                NOT_TEXT,
                "{operation} {content}"
            );
        }
    }
    // A `dequeue` has no content to make text of.
    assert_eq!(
        once(&[hello(), queue("dequeue", json!({ "toString": 1 }))]),
        in_flight(r#"[["u1","user","Hello",true,0]]"#)
    );
}

#[test]
fn a_tool_s_text_blocks_are_each_the_string_javascript_makes_of_their_text() {
    let calling = || {
        assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([tool_use(json!("t1"))]),
            Value::Null,
        )
    };
    let returned = |content: Value| {
        once(&[
            hello(),
            calling(),
            user(
                "u2",
                json!("a1"),
                json!([tool_result(json!("t1"), content)]),
            ),
        ])
    };
    let items = |tool: &str| {
        in_flight(&format!(
            r#"[["u1","user","Hello",true,0],["m1","assistant","",false,1],["t1","tool",{tool},true,2]]"#
        ))
    };
    assert_eq!(
        returned(json!([
            text(json!([1, 2])),
            text(Value::Null),
            { "type": "text" },
            text(json!(3.5)),
            text(json!({ "a": 1 }))
        ])),
        items(r#""1,2\n\n\n3.5\n[object Object]""#)
    );
    // Blocks of other kinds make `visibleText` of the whole list.
    assert_eq!(
        returned(json!([text(json!("a")), { "type": "image" }, "b", 7, null])),
        items(r#""a\n{\"type\":\"image\"}\nb\n7\nnull""#)
    );
    assert_eq!(returned(json!([])), items(r#""""#));
    assert_eq!(returned(json!({ "a": [1] })), items(r#""{\"a\":[1]}""#));
    assert_eq!(returned(json!(5)), items(r#""5""#));
    // Node: TypeError.
    assert_eq!(
        returned(json!([text(json!({ "toString": null }))])),
        NOT_TEXT
    );
}

/// A refusal of the API, 429, at `timestamp`, or at none.
fn refusal(timestamp: Option<Value>) -> Value {
    let record = refused("a1", json!({ "apiErrorStatus": 429 }));
    match timestamp {
        Some(timestamp) => having(record, json!({ "timestamp": timestamp })),
        None => lacking(record, "timestamp"),
    }
}

#[test]
fn a_refusal_s_time_is_the_date_javascript_parses_of_the_string_it_makes_of_it() {
    let said = |at: &str, quota: &str| {
        format!(
            r#"{{"items":[["u1","user","Hello",true,0],["m1","assistant","You've hit your limit. Resets in 2 hours.",false,{at}]],"inFlight":false,"asking":false,"failed":true,"quota":{quota},"settlement":"settled"}}"#
        )
    };
    let no_time = r#"{"state":"exhausted","at":null,"resetsAt":null}"#;
    let listed = json!(["2026-09-19T10:00:00.000Z"]);
    assert_eq!(
        once(&[hello(), refusal(Some(listed))]),
        said(
            r#"["2026-09-19T10:00:00.000Z"]"#,
            r#"{"state":"exhausted","at":"2026-09-19T10:00:00.000Z","resetsAt":"2026-09-19T12:00:00.000Z"}"#
        )
    );
    // No time, as a record's `timestamp ?? seq`: the record's place.
    assert_eq!(
        once(&[hello(), refusal(Some(Value::Null))]),
        said("1", no_time)
    );
    assert_eq!(once(&[hello(), refusal(None)]), said("1", no_time));
    // A number no date reads.
    assert_eq!(
        once(&[hello(), refusal(Some(json!(24)))]),
        said("24", no_time)
    );
    // A fraction after the minutes, and one past 24:00, no time to V8 either.
    for stamped in ["2026-09-19T12:00.1Z", "2026-09-19T24:00:00.0001Z"] {
        assert_eq!(
            once(&[hello(), refusal(Some(json!(stamped)))]),
            said(&format!("\"{stamped}\""), no_time),
            "{stamped}"
        );
    }
    // Node: TypeError.
    assert_eq!(
        once(&[hello(), refusal(Some(json!({ "toString": "x" })))]),
        NOT_TEXT
    );
}
