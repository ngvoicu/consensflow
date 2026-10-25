//! What the main chain's messages say, where the goldens' scenarios never
//! reach: chains and messages no reading holds, what V8 threw on, text parts,
//! requests keyed as JavaScript keys them, and the question tool's calls.

use super::*;
use crate::shared::record::reading::Role;

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
fn the_replies_of_a_request_before_its_last_are_never_complete_and_its_last_is_where_it_stopped() {
    // Devin stores a message for each step of a request (the owner's decision of
    // 2026-10-15: a result is what the member wrote after its last tool call): only the last
    // reply of a request can be its final answer, and it is one once Devin stored it as the
    // turn's end.
    let staged = Staged::new();
    let output = |id: &str, call: &str| json!({ "message_id": id, "role": "tool", "content": "ok", "tool_call_id": call });
    let calling = |id: &str, said: &str, call: &str| {
        let mut message = reply(id, json!(said));
        message["tool_calls"] = json!([{ "id": call, "name": "read" }]);
        message
    };
    staged.chain(&[
        user("u-1", Some(json!("request-1"))),
        calling("a-1", "Still reviewing", "c-1"),
        output("t-1", "c-1"),
        calling("a-2", "Now the tests", "c-2"),
        output("t-2", "c-2"),
        stopped("a-3", "PASS: no findings."),
    ]);
    let reading = look(&mut staged.reader());
    let Reading::Known(record) = &*reading else {
        panic!("a reading");
    };

    assert_eq!(record.settlement, Settlement::Settled);
    let shape: Vec<(Role, bool)> = record
        .items
        .iter()
        .map(|item| (item.role, item.complete))
        .collect();
    assert_eq!(
        shape,
        [
            (Role::User, true),
            (Role::Assistant, false),
            (Role::Tool, true),
            (Role::Assistant, false),
            (Role::Tool, true),
            (Role::Assistant, true),
        ]
    );
    let words: Vec<&str> = record.items.iter().map(|item| &*item.text).collect();
    assert_eq!(
        (words[1], words[3], words[5]),
        ("Still reviewing", "Now the tests", "PASS: no findings.")
    );
}

/// The texts of the items one look reads.
fn texts(staged: &Staged) -> Vec<String> {
    match &*look(&mut staged.reader()) {
        Reading::Known(record) => record
            .items
            .iter()
            .map(|item| item.text.to_string())
            .collect(),
        Reading::Unknown(reason) | Reading::Missing(reason) => panic!("unread: {reason}"),
    }
}

/// The member's note that its turn was cut, and the chief's handoff given again on the
/// conversation it was given to: what a resumed window is given first.
const MEMBER: &str = "[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nConsensFlow quit while you were working on T-1. Go on where you stopped.";
const CHIEF: &str =
    "[ConsensFlow m-7 · note from ConsensFlow]\nYou are the chief now of app, switched in from Claude Code.";

/// A resumed window's first message goes behind `/bypass` (`adapter::first_message`), in
/// its launch's prompt file, and Devin may store the command with the prompt it ran, or
/// the prompt alone: either way the record reads the prompt, its header first, the paste
/// the engine waits for (`is_paste_of`).
#[test]
fn a_launchs_first_message_behind_bypass_is_read_as_the_prompt_it_ran_however_devin_stored_it() {
    for note in [MEMBER, CHIEF] {
        for (stored, read) in [
            (format!("/bypass {note}"), note.to_owned()),
            (note.to_owned(), note.to_owned()),
            (format!("  /bypass\n{note}\n"), format!("{note}\n")),
        ] {
            let staged = Staged::new();
            staged.launched_behind_bypass("launch-1", SESSION, note);
            staged.chain(&[json!({ "message_id": "u-1", "role": "user", "content": stored })]);
            assert_eq!(texts(&staged), [read], "{stored:?}");
        }
    }
}

/// Astra's review of 2026-10-18: answer m-42 was on its way, and the human typed
/// `/bypass` before its header and a question of their own after it, which Devin stored
/// with the command. Read behind the command, it was the answer's paste. Only what
/// ConsensFlow gave a launch of the session behind `/bypass`, whole, is read behind it.
#[test]
fn a_humans_message_behind_bypass_is_read_as_it_was_stored() {
    let human = "/bypass [ConsensFlow m-42 · T-1 · answer from @chief]\nWhy is this pending?";
    // And one that begins with what ConsensFlow gave and says more.
    let more = format!("/bypass {MEMBER}\nAnd write YAML.");
    let staged = Staged::new();
    staged.launched_behind_bypass("launch-1", SESSION, MEMBER);
    staged.chain(&[
        json!({ "message_id": "u-1", "role": "user", "content": format!("/bypass {MEMBER}") }),
        stopped("a-1", "Going on."),
        json!({ "message_id": "u-2", "role": "user", "content": human }),
        stopped("a-2", "It waits."),
        json!({ "message_id": "u-3", "role": "user", "content": more }),
    ]);
    assert_eq!(
        texts(&staged),
        [MEMBER, "Going on.", human, "It waits.", &more]
    );
    // Nor the first message itself where no launch of the session was given it so: one
    // with no prompt file, or another session's.
    let stored = format!("/bypass {MEMBER}");
    let staged = Staged::new();
    staged.wire(
        "launch-1",
        &[json!({ "sessionId": SESSION, "update": { "sessionUpdate": "available_commands_update" } })],
    );
    staged.launched_behind_bypass("launch-2", "other-session", MEMBER);
    staged.chain(&[json!({ "message_id": "u-1", "role": "user", "content": stored })]);
    assert_eq!(texts(&staged), [stored]);
    // Only Devin's command, with a prompt after it, and only in a user's message.
    for (role, stored) in [
        ("user", "/bypassed it"),
        ("user", "/bypass"),
        ("user", "say /bypass now"),
        ("assistant", "/bypass [ConsensFlow m-2 · note]"),
    ] {
        let staged = Staged::new();
        staged.launched_behind_bypass("launch-1", SESSION, MEMBER);
        staged.chain(&[json!({ "message_id": "m", "role": role, "content": stored })]);
        assert_eq!(texts(&staged), [stored], "{role}: {stored}");
    }
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
