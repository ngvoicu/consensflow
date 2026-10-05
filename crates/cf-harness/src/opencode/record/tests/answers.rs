//! What a session's messages and parts say, where the goldens' scenarios
//! never reach: tools open and ended, a question asked, a refusal for quota,
//! a reply streaming or cut at its length, and a part of another session.

use super::*;

#[test]
fn a_reply_that_stopped_is_settled_and_one_streaming_or_cut_is_not_complete() {
    let staged = Staged::new();
    staged.stopped();
    assert_eq!(read_once(&staged), settled(&conversation_items(true, 9)));
    let staged = Staged::new();
    staged.conversation(&json!({ "role": "assistant", "time": { "created": 3 } }));
    assert_eq!(
        read_once(&staged),
        in_flight(&conversation_items(false, 3), false)
    );
    let staged = Staged::new();
    staged.conversation(&json!({
        "role": "assistant",
        "time": { "created": 3, "completed": 9 },
        "finish": "length",
    }));
    assert_eq!(read_once(&staged), settled(&conversation_items(false, 9)));
}

#[test]
fn a_tool_still_running_holds_the_turn_open_and_a_question_asks() {
    for (tool, call, asking) in [
        ("bash", Value::Null, false),
        ("question", json!("c1"), true),
    ] {
        let staged = Staged::new();
        staged.stopped();
        staged.part(
            "p3",
            "m2",
            5,
            &json!({ "type": "tool", "tool": tool, "callID": call, "state": { "status": "running" } }),
        );
        staged.event(6, "message.part.updated.1", &part_of("p3", "m2"));
        assert_eq!(
            read_once(&staged),
            in_flight(&conversation_items(true, 9), asking),
            "{tool}"
        );
    }
}

#[test]
fn a_tool_that_ended_says_its_output_else_its_error_at_its_end() {
    let staged = Staged::new();
    staged.stopped();
    staged.part(
        "p3",
        "m2",
        5,
        &json!({
            "type": "tool",
            "tool": "bash",
            "callID": "c1",
            "state": { "status": "completed", "output": ["a", { "text": "b" }], "time": { "end": 7 } },
        }),
    );
    staged.part(
        "p4",
        "m2",
        6,
        &json!({ "type": "tool", "tool": "bash", "callID": "c2", "state": { "status": "error", "error": null } }),
    );
    staged.event(6, "message.part.updated.1", &part_of("p3", "m2"));
    staged.event(7, "message.part.updated.1", &part_of("p4", "m2"));
    assert_eq!(
        read_once(&staged),
        settled(
            r#"[["m1","user","Review T-1",true,1],["m2","assistant","Done.",true,9],["p3","tool","a\nb",true,7],["p4","tool","",true,6]]"#
        )
    );
}

#[test]
fn a_refusal_for_quota_is_read_as_node_read_it() {
    let staged = Staged::new();
    staged.conversation(&json!({
        "role": "assistant",
        "time": { "created": 3, "completed": 1_790_000_000_000_i64 },
        "finish": "stop",
        "error": {
            "name": "APIError",
            "data": { "message": "Rate limited. Resets in 2 hours.", "statusCode": "429" },
        },
    }));
    assert_eq!(
        read_once(&staged),
        format!(
            r#"{{"items":{},"inFlight":false,"asking":false,"failed":true,"quota":{{"state":"exhausted","at":"2026-09-21T14:13:20.000Z","resetsAt":"2026-09-21T16:13:20.000Z"}},"settlement":"settled"}}"#,
            conversation_items(false, 1_790_000_000_000)
        )
    );
    // A failure with no message is said as its error's JSON, which names no reset.
    let staged = Staged::new();
    staged.conversation(&json!({
        "role": "assistant",
        "time": { "created": 3, "completed": 1_790_000_000_000_i64 },
        "error": { "name": "APIError", "data": { "statusCode": 402 } },
    }));
    assert_eq!(
        read_once(&staged),
        format!(
            r#"{{"items":{},"inFlight":false,"asking":false,"failed":true,"quota":{{"state":"exhausted","at":"2026-09-21T14:13:20.000Z","resetsAt":null}},"settlement":"settled"}}"#,
            conversation_items(false, 1_790_000_000_000)
        )
    );
    // Node: `Number()` of an object with a toString of its own threw.
    let staged = Staged::new();
    staged.conversation(&json!({
        "role": "assistant",
        "time": { "created": 3, "completed": 9 },
        "error": { "name": "APIError", "data": { "statusCode": { "toString": 1 } } },
    }));
    assert_eq!(
        read_once(&staged),
        "unknown: unreadable: an object with a toString of its own cannot be made text"
    );
}

#[test]
fn a_part_of_another_session_is_none_of_this_one() {
    let staged = Staged::new();
    staged.stopped();
    staged
        .store
        .execute(
            "insert into part values ('p9', 'm2', 'ses_other', 5, 5, ?)",
            [json!({ "type": "text", "text": "other" }).to_string()],
        )
        .unwrap();
    assert_eq!(read_once(&staged), settled(&conversation_items(true, 9)));
}

#[test]
fn an_item_is_placed_by_its_text_s_last_event_else_its_first_completion() {
    // A reply of no text, completed twice, a tool between: the first
    // completion places it, before the tool.
    let staged = Staged::new();
    staged.message(
        "m1",
        1,
        &json!({ "role": "user", "time": { "created": 1 } }),
    );
    staged.part(
        "p1",
        "m1",
        2,
        &json!({ "type": "text", "text": "Review T-1" }),
    );
    let completed = json!({ "created": 3, "completed": 9 });
    staged.message(
        "m2",
        3,
        &json!({ "role": "assistant", "time": completed, "finish": "stop" }),
    );
    let tool = json!({
        "type": "tool",
        "tool": "bash",
        "callID": "c1",
        "state": { "status": "completed", "output": "ok", "time": { "end": 6 } },
    });
    staged.part("p3", "m2", 4, &tool);
    staged.event(1, "message.updated.1", &info("m1", None));
    staged.event(2, "message.part.updated.1", &part_of("p1", "m1"));
    staged.event(3, "message.updated.1", &info("m2", None));
    staged.event(5, "message.updated.1", &info("m2", Some(&completed)));
    staged.event(6, "message.part.updated.1", &part_of("p3", "m2"));
    staged.event(7, "message.updated.1", &info("m2", Some(&completed)));
    assert_eq!(
        read_once(&staged),
        settled(
            r#"[["m1","user","Review T-1",true,1],["m2","assistant","",true,9],["p3","tool","ok",true,6]]"#
        )
    );
    // A text part updated again after a tool: its last event places the reply.
    let staged = Staged::new();
    staged.message(
        "m1",
        1,
        &json!({ "role": "user", "time": { "created": 1 } }),
    );
    staged.part(
        "p1",
        "m1",
        2,
        &json!({ "type": "text", "text": "Review T-1" }),
    );
    staged.message(
        "m2",
        3,
        &json!({ "role": "assistant", "time": completed, "finish": "stop" }),
    );
    staged.part("p2", "m2", 4, &json!({ "type": "text", "text": "Done." }));
    staged.part("p3", "m2", 5, &tool);
    staged.event(1, "message.updated.1", &info("m1", None));
    staged.event(2, "message.part.updated.1", &part_of("p1", "m1"));
    staged.event(3, "message.updated.1", &info("m2", Some(&completed)));
    staged.event(4, "message.part.updated.1", &part_of("p2", "m2"));
    staged.event(6, "message.part.updated.1", &part_of("p3", "m2"));
    staged.event(8, "message.part.updated.1", &part_of("p2", "m2"));
    assert_eq!(
        read_once(&staged),
        settled(
            r#"[["m1","user","Review T-1",true,1],["p3","tool","ok",true,6],["m2","assistant","Done.",true,9]]"#
        )
    );
}

#[test]
fn a_user_s_message_of_white_space_alone_opens_the_turn_and_is_no_item() {
    let staged = Staged::new();
    staged.message(
        "m1",
        1,
        &json!({ "role": "user", "time": { "created": 1 } }),
    );
    staged.part(
        "p1",
        "m1",
        2,
        &json!({ "type": "text", "text": " \u{a0}\n" }),
    );
    staged.event(1, "message.updated.1", &info("m1", None));
    staged.event(2, "message.part.updated.1", &part_of("p1", "m1"));
    assert_eq!(read_once(&staged), in_flight("[]", false));
}

#[test]
fn a_refusal_s_words_are_its_message_s_not_its_error_s() {
    let staged = Staged::new();
    staged.conversation(&json!({
        "role": "assistant",
        "time": { "created": 3, "completed": 1_790_000_000_000_i64 },
        "error": { "name": "resets in 1 hour", "data": { "message": "no reset", "statusCode": 429 } },
    }));
    assert_eq!(
        read_once(&staged),
        format!(
            r#"{{"items":{},"inFlight":false,"asking":false,"failed":true,"quota":{{"state":"exhausted","at":"2026-09-21T14:13:20.000Z","resetsAt":null}},"settlement":"settled"}}"#,
            conversation_items(false, 1_790_000_000_000)
        )
    );
}

#[test]
fn a_time_of_a_column_the_store_has_not_is_left_out_as_json_leaves_it() {
    let staged = Staged::new();
    staged
        .store
        .execute_batch(
            "drop table part;
             create table part (id text primary key, message_id text not null,
               session_id text not null, time_created integer not null, data text not null);",
        )
        .unwrap();
    let completed = json!({ "created": 3, "completed": 9 });
    staged.message(
        "m",
        3,
        &json!({ "role": "assistant", "time": completed, "finish": "stop" }),
    );
    staged
        .store
        .execute(
            "insert into part values ('p', 'm', ?, 4, ?)",
            (
                SESSION,
                json!({ "type": "tool", "tool": "bash", "callID": "c", "state": { "status": "completed", "output": "ok" } })
                    .to_string(),
            ),
        )
        .unwrap();
    staged.event(1, "message.updated.1", &info("m", Some(&completed)));
    staged.event(2, "message.part.updated.1", &part_of("p", "m"));
    // Node 26's `JSON.stringify` of its reading: the tool's `at` undefined.
    assert_eq!(
        serde_json::to_string(&*look(&mut staged.reader())).unwrap(),
        r#"{"items":[{"id":"m","role":"assistant","text":"","complete":true,"at":9},{"id":"p","role":"tool","text":"ok","complete":true}],"inFlight":false,"asking":false,"failed":false,"quota":null,"settlement":{"state":"settled"}}"#
    );
}
