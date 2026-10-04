//! The tool calls that keep a turn open, and the items of a reading: each
//! message and result once, in the order of the record that last said it.

use super::*;

#[test]
fn a_tool_use_id_that_is_an_object_is_a_call_nothing_can_close() {
    let in_one_message = |call: Value, closing: Option<Value>| {
        let mut records = vec![
            hello(),
            assistant(
                "a1",
                "u1",
                json!("m1"),
                json!([tool_use(call)]),
                Value::Null,
            ),
            assistant(
                "a2",
                "a1",
                json!("m1"),
                json!([text(json!("Done"))]),
                json!("end_turn"),
            ),
        ];
        records.extend(closing);
        records.push(duration("d1", "a2"));
        once(&records)
    };
    let done = r#"[["u1","user","Hello",true,0],["m1","assistant","Done",true,2]"#;
    // Each object is its own: the call stays open, and no result names it.
    for call in [json!({ "a": 1 }), json!([])] {
        assert_eq!(in_one_message(call, None), in_flight(&format!("{done}]")));
    }
    let result = user(
        "u2",
        json!("a2"),
        json!([tool_result(json!({ "a": 1 }), json!("x"))]),
    );
    assert_eq!(
        in_one_message(json!({ "a": 1 }), Some(result)),
        "unknown: unreadable: missing native claude tool result id at record 3"
    );
    // A call is closed by the result that names it as the text it is, and by nothing else.
    let result = |call: &str| {
        Some(user(
            "u2",
            json!("a2"),
            json!([tool_result(json!(call), json!("x"))]),
        ))
    };
    let with_tool = format!(r#"{done},["1","tool","x",true,3]]"#);
    assert_eq!(in_one_message(json!(1), result("1")), in_flight(&with_tool));
    assert_eq!(in_one_message(json!("1"), result("1")), settled(&with_tool));
    // A call with no name opens nothing.
    for nameless in [json!(0), json!(""), Value::Null, json!(false)] {
        assert_eq!(in_one_message(nameless, None), settled(&format!("{done}]")));
    }
}

#[test]
fn a_server_tool_use_is_a_call_and_an_advisor_result_in_an_assistant_record_closes_it() {
    let server = || json!({ "type": "server_tool_use", "id": "s7" });
    let advice =
        || json!({ "type": "advisor_tool_result", "tool_use_id": "s7", "content": "advice" });
    let ending = |content: Value| {
        once(&[
            hello(),
            assistant("a1", "u1", json!("m1"), json!([server()]), Value::Null),
            assistant("a2", "a1", json!("m1"), content, json!("end_turn")),
            duration("d1", "a2"),
        ])
    };
    let done = r#"["u1","user","Hello",true,0],["m1","assistant","Done",true,2]"#;
    assert_eq!(
        ending(json!([text(json!("Done"))])),
        in_flight(&format!("[{done}]"))
    );
    assert_eq!(
        ending(json!([advice(), text(json!("Done"))])),
        settled(&format!(r#"[{done},["s7","tool","advice",true,2]]"#))
    );
    // In a user's record it is no result of a tool.
    let calling = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([tool_use(json!("t1"))]),
        Value::Null,
    );
    let in_user = user(
        "u2",
        json!("a1"),
        json!([{ "type": "advisor_tool_result", "tool_use_id": "t1", "content": "advice" }]),
    );
    assert_eq!(
        once(&[hello(), calling, in_user]),
        in_flight(r#"[["u1","user","Hello",true,0],["m1","assistant","",false,1]]"#)
    );
}

#[test]
fn a_tool_s_result_said_again_is_the_same_item_with_the_later_text_and_place() {
    let calling = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([tool_use(json!("t1"))]),
        Value::Null,
    );
    let result = |uuid: &str, parent: &str, text: &str| {
        user(
            uuid,
            json!(parent),
            json!([tool_result(json!("t1"), json!(text))]),
        )
    };
    assert_eq!(
        once(&[
            hello(),
            calling,
            result("u2", "a1", "first"),
            user("u3", json!("u2"), json!("More")),
            result("u4", "u3", "second"),
        ]),
        in_flight(
            r#"[["u1","user","Hello",true,0],["m1","assistant","",false,1],["u3","user","More",true,3],["t1","tool","second",true,4]]"#
        )
    );
}

#[test]
fn items_of_one_record_are_ordered_by_id_as_locale_compare_orders_them() {
    let blocks = |ids: &[(&str, &str)]| {
        let mut blocks: Vec<Value> = ids
            .iter()
            .map(|(id, content)| tool_result(json!(id), json!(content)))
            .collect();
        blocks.push(text(json!("Hi")));
        Value::from(blocks)
    };
    // By code point `B_tool` comes first; by `localeCompare` a letter's lower
    // case comes before its upper case, and both after `a`.
    let ordered = assistant(
        "a1",
        "u1",
        json!("a_msg"),
        blocks(&[("B_tool", "B"), ("b_tool", "b"), ("a_tool", "a")]),
        Value::Null,
    );
    assert_eq!(
        once(&[hello(), ordered]),
        in_flight(
            r#"[["u1","user","Hello",true,0],["a_msg","assistant","Hi",false,1],["a_tool","tool","a",true,1],["b_tool","tool","b",true,1],["B_tool","tool","B",true,1]]"#
        )
    );
    // Items of the one id keep the order they were pushed in.
    let same = assistant(
        "a1",
        "u1",
        json!("same"),
        blocks(&[("same", "tool")]),
        Value::Null,
    );
    assert_eq!(
        once(&[hello(), same]),
        in_flight(
            r#"[["u1","user","Hello",true,0],["same","assistant","Hi",false,1],["same","tool","tool",true,1]]"#
        )
    );
}

#[test]
fn a_record_that_says_nothing_of_its_message_moves_it_all_the_same() {
    let at = |time: &str| json!({ "timestamp": format!("2026-09-19T10:00:{time}.000Z") });
    let first = having(
        assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([text(json!("Hi"))]),
            Value::Null,
        ),
        at("00"),
    );
    let later = |uuid: &str, content: Value| {
        having(
            assistant(uuid, "u2", json!("m1"), content, Value::Null),
            at("09"),
        )
    };
    let said = |message: &str| {
        in_flight(&format!(
            r#"[["u1","user","Hello",true,0],["u2","user","Two",true,2],["m1","assistant","{message}",false,"2026-09-19T10:00:09.000Z"]]"#
        ))
    };
    // Node's answer: the message is where, and when, its latest record is,
    // though that record had no text, or one of its own uuid had none.
    let cases = [
        ("a3", json!([tool_use(json!("t1"))]), "Hi"),
        ("a3", json!([]), "Hi"),
        ("a3", json!("plain"), "Hi\\nplain"),
        ("a1", json!([text(json!("Hi again"))]), "Hi again"),
        ("a1", json!([]), "Hi"),
    ];
    for (uuid, content, message) in cases {
        assert_eq!(
            once(&[
                hello(),
                first.clone(),
                user("u2", json!("a1"), json!("Two")),
                later(uuid, content.clone())
            ]),
            said(message),
            "{uuid} {content}"
        );
    }
}

#[test]
fn an_item_said_again_goes_where_the_later_record_was() {
    let said = |uuid: &str, parent: &str, content: &str| {
        assistant(
            uuid,
            parent,
            json!("m1"),
            json!([text(json!(content))]),
            Value::Null,
        )
    };
    assert_eq!(
        once(&[
            hello(),
            said("a1", "u1", "One"),
            user("u2", json!("a1"), json!("Two")),
            said("a3", "u2", "Three"),
            queue("enqueue", json!("x")),
        ]),
        in_flight(
            r#"[["u1","user","Hello",true,0],["u2","user","Two",true,2],["m1","assistant","One\nThree",false,3]]"#
        )
    );
}
