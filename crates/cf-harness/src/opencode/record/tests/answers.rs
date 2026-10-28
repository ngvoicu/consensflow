//! What a session's messages and parts say, where the goldens' scenarios
//! never reach: tools open and ended, a question asked, a refusal for quota,
//! a reply streaming or cut at its length, and a part of another session.

use super::*;
use crate::shared::record::reading::{Item, Role};

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

/// A reply to the user's message that finished as `finish`, saying `said` and, when it
/// stopped for a tool, calling it: its rows and its events, from the event `seq`.
fn reply(staged: &Staged, id: &str, seq: i64, finish: &str, said: &str, tool: Option<&str>) {
    let (text, call) = (format!("{id}-text"), format!("{id}-call"));
    let time = json!({ "created": seq, "completed": seq + 4 });
    staged.message(
        id,
        seq,
        &json!({ "role": "assistant", "time": time, "finish": finish }),
    );
    staged.part(&text, id, seq + 1, &json!({ "type": "text", "text": said }));
    staged.event(seq, "message.updated.1", &info(id, None));
    staged.event(seq + 1, "message.part.updated.1", &part_of(&text, id));
    if let Some(tool) = tool {
        staged.part(
            &call,
            id,
            seq + 2,
            &json!({
                "type": "tool", "tool": tool, "callID": call,
                "state": { "status": "completed", "output": "out", "time": { "end": seq + 2 } },
            }),
        );
        staged.event(seq + 2, "message.part.updated.1", &part_of(&call, id));
    }
    staged.event(seq + 3, "message.updated.1", &info(id, Some(&time)));
}

#[test]
fn the_replies_of_a_turn_that_stopped_for_a_tool_are_never_complete_and_the_reply_that_stopped_is()
{
    // OpenCode writes a message for each step of a turn: the owner's decision of 2026-10-15
    // (a result is what the member wrote after its last tool call) reads the final answer
    // out of the reply that stopped, the replies before it stopped for a tool.
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
    staged.event(1, "message.updated.1", &info("m1", None));
    staged.event(2, "message.part.updated.1", &part_of("p1", "m1"));
    reply(
        &staged,
        "m2",
        10,
        "tool-calls",
        "Still reviewing",
        Some("bash"),
    );
    reply(
        &staged,
        "m3",
        20,
        "tool-calls",
        "Now the tests",
        Some("bash"),
    );
    reply(&staged, "m4", 30, "stop", "PASS: no findings.", None);

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
        ],
        "the replies before the last stopped for a tool, and the last stopped"
    );
    let words: Vec<&str> = record.items.iter().map(|item| &*item.text).collect();
    assert_eq!(
        (words[1], words[3], words[5]),
        ("Still reviewing", "Now the tests", "PASS: no findings.")
    );
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
    // A failure with no message is said as its error's JSON, which names no reset. A 402 is
    // the account's billing, no limit that resets (MUS-23): unavailable.
    let staged = Staged::new();
    staged.conversation(&json!({
        "role": "assistant",
        "time": { "created": 3, "completed": 1_790_000_000_000_i64 },
        "error": { "name": "APIError", "data": { "statusCode": 402 } },
    }));
    assert_eq!(
        read_once(&staged),
        format!(
            r#"{{"items":{},"inFlight":false,"asking":false,"failed":true,"quota":{{"state":"unavailable","at":"2026-09-21T14:13:20.000Z","resetsAt":null,"because":"its billing was refused: {{\"name\":\"APIError\",\"data\":{{\"statusCode\":402}}}}"}},"settlement":"settled"}}"#,
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

#[test]
fn anthropics_400_for_spent_extra_usage_is_a_refusal_for_quota_that_names_no_reset() {
    // Anthropic's spent extra usage is a 400 that says so: a refusal for quota
    // that names no reset (the words of Pi's sessions on this Mac; T-88).
    let staged = Staged::new();
    staged.conversation(&json!({
        "role": "assistant",
        "time": { "created": 3, "completed": 1_790_000_000_000_i64 },
        "finish": "stop",
        "error": {
            "name": "APIError",
            "data": {
                "message": "You're out of extra usage. Add more at claude.ai/settings/usage and keep going.",
                "statusCode": 400,
            },
        },
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
fn a_failed_reply_keeps_the_words_of_its_error() {
    let failure_of = |reply: &Value| {
        let staged = Staged::new();
        staged.conversation(reply);
        match &*look(&mut staged.reader()) {
            Reading::Known(record) => (record.failed, record.failure.clone()),
            Reading::Unknown(reason) | Reading::Missing(reason) => panic!("unknown: {reason}"),
        }
    };
    let said = failure_of(&json!({
        "role": "assistant",
        "time": { "created": 3, "completed": 9 },
        "error": { "name": "ProviderModelNotFoundError", "data": { "message": "Model not found: nonesuch/gpt-0" } },
    }));
    assert_eq!(
        said,
        (true, Some("Model not found: nonesuch/gpt-0".to_owned()))
    );
    // No message: what it is, its name and its response's status, and never the
    // response's body or headers an SDK's error keeps beside them.
    let named = failure_of(&json!({
        "role": "assistant",
        "time": { "created": 3, "completed": 9 },
        "error": {
            "name": "APIError",
            "data": {
                "statusCode": 500,
                "isRetryable": false,
                "responseBody": "{\"error\":{\"message\":\"upstream body\"}}",
                "responseHeaders": { "set-cookie": "session=abc123def456", "x-request-id": "req_1" },
            },
        },
    }));
    assert_eq!(named, (true, Some("APIError (status 500)".to_owned())));
    let bare = failure_of(&json!({
        "role": "assistant",
        "time": { "created": 3, "completed": 9 },
        "error": { "name": "UnknownError" },
    }));
    assert_eq!(bare, (true, Some("UnknownError".to_owned())));
    let fine = failure_of(&json!({
        "role": "assistant",
        "time": { "created": 3, "completed": 9 },
        "finish": "stop",
    }));
    assert_eq!(fine, (false, None));
}

#[test]
fn the_summary_a_compaction_writes_is_marked_so_and_the_answer_before_it_is_not() {
    // OpenCode 1.19.3 writes a compaction's summary as an assistant's message of its own,
    // `summary: true`, its mode and agent `compaction`, which stops as any reply does
    // (Calliope on 0eec91d2: it was read as the member's words, and could be joined into a
    // final answer).
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
    staged.event(1, "message.updated.1", &info("m1", None));
    staged.event(2, "message.part.updated.1", &part_of("p1", "m1"));
    reply(&staged, "m2", 10, "stop", "PASS: no findings.", None);
    let time = json!({ "created": 20, "completed": 24 });
    staged.message(
        "m3",
        20,
        &json!({
            "role": "assistant", "time": time, "finish": "stop",
            "summary": true, "mode": "compaction", "agent": "compaction",
        }),
    );
    staged.part(
        "m3-text",
        "m3",
        21,
        &json!({ "type": "text", "text": "Summary of the conversation so far" }),
    );
    staged.event(20, "message.updated.1", &info("m3", None));
    staged.event(21, "message.part.updated.1", &part_of("m3-text", "m3"));
    staged.event(23, "message.updated.1", &info("m3", Some(&time)));

    let reading = look(&mut staged.reader());
    let Reading::Known(record) = &*reading else {
        panic!("a reading");
    };
    let shape: Vec<(&str, bool)> = record
        .items
        .iter()
        .filter(|item| item.role == Role::Assistant)
        .map(|item| (&*item.text, item.compaction))
        .collect();
    assert_eq!(
        shape,
        [
            ("PASS: no findings.", false),
            ("Summary of the conversation so far", true),
        ]
    );
}

/// The last item a look reads of a reply to the user's message, `reply` its message's
/// data and `parts` the data of its parts.
fn reply_read(reply: &Value, parts: &[Value]) -> Item {
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
    staged.event(1, "message.updated.1", &info("m1", None));
    staged.event(2, "message.part.updated.1", &part_of("p1", "m1"));
    staged.message("m2", 3, reply);
    staged.event(3, "message.updated.1", &info("m2", None));
    let mut n = 4;
    for part in parts {
        let id = format!("p{n}");
        staged.part(&id, "m2", n, part);
        staged.event(n, "message.part.updated.1", &part_of(&id, "m2"));
        n += 1;
    }
    if let Some(time) = reply
        .get("time")
        .filter(|time| time.get("completed").is_some())
    {
        staged.event(n, "message.updated.1", &info("m2", Some(time)));
    }
    match &*look(&mut staged.reader()) {
        Reading::Known(record) => record.items.last().unwrap().clone(),
        reading => panic!("{reading:?}"),
    }
}

#[test]
fn a_reply_with_nothing_of_the_models_in_it_is_synthetic_and_one_the_model_began_is_not() {
    // T-530: OpenCode makes the assistant's message before the model replies, and keeps
    // it when the request fails or is aborted before a token (seen on this Mac's
    // store: rows of no part, `MessageAbortedError` and `APIError` rows of none, an
    // `UnknownError` row of its `step-start` alone). No turn got through any of them.
    let begun = json!({ "role": "assistant", "time": { "created": 3 } });
    let failed = |name: &str| {
        json!({ "role": "assistant", "time": { "created": 3, "completed": 4 },
            "error": { "name": name, "data": { "message": "failed" } } })
    };
    let step = json!({ "type": "step-start" });
    for (reply, parts) in [
        (begun.clone(), vec![]),
        (begun.clone(), vec![step.clone()]),
        (failed("MessageAbortedError"), vec![]),
        (failed("APIError"), vec![]),
        (failed("UnknownError"), vec![step.clone()]),
        (begun.clone(), vec![json!({ "type": "text", "text": " " })]),
    ] {
        let item = reply_read(&reply, &parts);
        assert_eq!(
            (item.role, item.synthetic),
            (Role::Assistant, true),
            "{reply} {parts:?}"
        );
    }
    for part in [
        json!({ "type": "text", "text": "Reading the" }),
        json!({ "type": "reasoning", "text": "The user wants" }),
        json!({ "type": "tool", "tool": "read", "callID": "c1", "state": { "status": "running" } }),
    ] {
        let item = reply_read(&begun, &[step.clone(), part.clone()]);
        assert!(!item.synthetic, "{part}");
    }
}
