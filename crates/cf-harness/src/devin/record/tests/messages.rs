//! What the main chain's messages say, where the goldens' scenarios never
//! reach: chains and messages no reading holds, what V8 threw on, text parts,
//! requests keyed as JavaScript keys them, and the question tool's calls.

use super::*;

#[test]
fn a_chain_that_leads_back_to_itself_and_messages_no_reading_holds_fail_the_look() {
    let staged = Staged::new();
    staged.node(Some("n-1"), Some("n-2"), &user("u-1", None));
    staged.node(Some("n-2"), Some("n-1"), &reply("a-1", json!("x")));
    staged.head("n-2");
    assert_eq!(
        read_once(&staged),
        "unknown: unreadable: cyclic Devin main chain"
    );
    let identity = "unknown: unreadable: invalid Devin message identity";
    for (messages, read) in [
        (vec![json!({ "message_id": 5, "role": "user" })], identity),
        (vec![user("u-1", None), user("u-1", None)], identity),
        (vec![json!(5)], identity),
        (vec![json!("\"text\"")], identity),
        (
            vec![json!({ "message_id": "m", "role": "robot" })],
            "unknown: unreadable: unknown Devin message role",
        ),
    ] {
        let staged = Staged::new();
        staged.chain(&messages);
        assert_eq!(read_once(&staged), read, "{messages:?}");
    }
}

#[test]
fn what_v8_threw_on_fails_the_look_in_words_of_its_own() {
    let ask = json!({ "id": "c", "name": "ask_user_question" });
    let with_calls = |calls: Value| {
        let mut message = reply("a-1", json!("x"));
        message["tool_calls"] = calls;
        message
    };
    for (message, reason) in [
        (
            json!("null"),
            "Devin's message at row 1 is null, where an object was read",
        ),
        (
            with_calls(json!("x")),
            "Devin's message at row 1 holds tool calls that are no list",
        ),
        (
            with_calls(json!({ "find": 1 })),
            "Devin's message at row 1 holds tool calls that are no list",
        ),
        (
            with_calls(json!([null, ask])),
            "Devin's message at row 1 holds a tool call that is null",
        ),
        (
            reply("a-1", json!([null])),
            "a Devin message's content holds a part that is null",
        ),
        (
            reply(
                "a-1",
                json!([{ "type": "text", "text": { "toString": 1 } }]),
            ),
            "an object with a toString of its own cannot be made text",
        ),
        (
            reply(
                "a-1",
                json!([{ "type": "text", "text": [{ "toString": 1 }] }]),
            ),
            "an object with a toString of its own cannot be made text",
        ),
        (json!("{bad"), "Devin's message at row 1 is no JSON"),
    ] {
        let staged = Staged::new();
        staged.chain(&[message]);
        assert_eq!(read_once(&staged), format!("unknown: unreadable: {reason}"));
    }
    // `find` stops at the call it finds: a null after it is never read.
    let staged = Staged::new();
    staged.chain(&[with_calls(json!([ask, null]))]);
    assert_eq!(
        read_once(&staged),
        format!(
            r#"{{"items":[["a-1","assistant","x",false,"{AT}"]],"inFlight":true,"asking":true,"failed":false,"settlement":"unknown"}}"#
        )
    );
    // A streamed piece of text with a `toString` of its own.
    let staged = Staged::new();
    staged.chain(&[
        user("u-1", Some(json!("request-1"))),
        reply("a-1", json!("x")),
    ]);
    staged.wire(
        "launch-1",
        &[chunk(Some(json!({ "toString": 1 })), "stream-1")],
    );
    assert_eq!(
        read_once(&staged),
        "unknown: unreadable: an object with a toString of its own cannot be made text"
    );
}

#[test]
fn a_message_s_text_parts_are_joined_as_javascript_joins_them() {
    let staged = Staged::new();
    staged.chain(&[json!({
        "message_id": "m",
        "role": "custom",
        "content": [
            { "type": "text", "text": "a" },
            { "type": "image" },
            "x",
            { "type": "text", "text": 2 },
            { "type": "text" },
            { "type": "text", "text": null },
            { "type": "text", "text": [1, null, [2]] },
            { "type": "text", "text": {} },
        ],
    })]);
    assert_eq!(
        read_once(&staged),
        format!(
            r#"{{"items":[["m","custom","a21,,2[object Object]",true,"{AT}"]],"inFlight":false,"asking":false,"failed":false,"settlement":"unknown"}}"#
        )
    );
    let staged = Staged::new();
    staged.chain(&[json!({ "message_id": "m", "role": "system", "content": "ctx" })]);
    assert_eq!(
        read_once(&staged),
        format!(
            r#"{{"items":[["m","custom","ctx",true,"{AT}"]],"inFlight":false,"asking":false,"failed":false,"settlement":"unknown"}}"#
        )
    );
}

#[test]
fn requests_are_told_apart_as_javascript_keys_them() {
    // Two requests, each answered with a turn's end: the first reply is
    // complete only where its request is not the second's.
    for (client, first_complete) in [
        (json!({ "x": 1 }), true),
        (json!("r"), false),
        (Value::Null, true),
        (json!(0), false),
        (json!(""), false),
    ] {
        let staged = Staged::new();
        staged.chain(&[
            user("u-1", Some(client.clone())),
            stopped("a-1", "One."),
            user("u-2", Some(client.clone())),
            stopped("a-2", "Two."),
        ]);
        assert_eq!(
            read_once(&staged),
            format!(
                r#"{{"items":[["u-1","user","Review T-1",true,"{AT}"],["a-1","assistant","One.",{first_complete},"{AT}"],["u-2","user","Review T-1",true,"{AT}"],["a-2","assistant","Two.",true,"{AT}"]],"inFlight":false,"asking":false,"failed":false,"settlement":"settled"}}"#
            ),
            "{client}"
        );
    }
}

#[test]
fn only_the_assistant_s_question_call_asks() {
    for (role, asking) in [
        ("user", false),
        ("tool", false),
        ("system", false),
        ("assistant", true),
    ] {
        let staged = Staged::new();
        staged.chain(&[json!({
            "message_id": "m",
            "role": role,
            "content": "x",
            "tool_calls": [{ "id": "c", "name": "ask_user_question" }],
        })]);
        let reading = look(&mut staged.reader());
        let Reading::Known(record) = &*reading else {
            panic!("{role}: {reading:?}");
        };
        assert_eq!(record.asking, asking, "{role}");
    }
}

#[test]
fn a_session_with_no_head_asks_from_the_rows_below_none() {
    let asking = |asking: bool| {
        format!(
            r#"{{"items":[],"inFlight":false,"asking":{asking},"failed":false,"settlement":"unknown"}}"#
        )
    };
    let asked = |call: Value| {
        let mut message = reply("a-1", json!("Voi întreba:"));
        message["tool_calls"] = json!([call]);
        message
    };
    let tool = |id: &str, call: Option<&str>| {
        let mut message = json!({ "message_id": id, "role": "tool", "content": "cariere" });
        if let Some(call) = call {
            message["tool_call_id"] = json!(call);
        }
        message
    };
    let staged = Staged::new();
    staged.node(
        Some("n-1"),
        None,
        &asked(json!({ "id": "call-1", "name": "ask_user_question" })),
    );
    staged.node(Some("n-2"), Some("n-1"), &tool("t-1", Some("other")));
    assert_eq!(read_once(&staged), asking(true));
    staged.node(Some("n-3"), Some("n-2"), &tool("t-2", Some("call-1")));
    assert_eq!(read_once(&staged), asking(false));
    // A call of no id waits, as `undefined` is not `null`; a tool message of
    // no call's id answers it; a call whose id is null never waited.
    let staged = Staged::new();
    staged.node(
        Some("n-1"),
        None,
        &asked(json!({ "name": "ask_user_question" })),
    );
    assert_eq!(read_once(&staged), asking(true));
    staged.node(Some("n-2"), Some("n-1"), &tool("t-1", None));
    assert_eq!(read_once(&staged), asking(false));
    let staged = Staged::new();
    staged.node(
        Some("n-1"),
        None,
        &asked(json!({ "id": null, "name": "ask_user_question" })),
    );
    assert_eq!(read_once(&staged), asking(false));
}

#[test]
fn rows_below_no_head_that_lead_back_fail_the_look_where_node_never_answered() {
    let staged = Staged::new();
    // A root row of no node: its children are the roots, itself among them.
    staged.node(None, None, &user("u-1", None));
    assert_eq!(
        read_once(&staged),
        "unknown: unreadable: Devin's rows below the main chain's head lead back to row 1"
    );
}
