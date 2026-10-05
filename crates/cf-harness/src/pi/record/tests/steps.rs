//! What a session's records say, where the goldens' scenarios never reach: ids
//! and times of kinds a record should not hold and JavaScript reads anyway,
//! tool calls keyed by what JavaScript keys a `Set` by, a turn that names no
//! end, what a refusal's quota outlives, and a record that is `null`.

use super::*;

#[test]
fn a_record_that_is_null_fails_the_look_and_a_file_of_no_record_is_an_empty_session() {
    let mut stage = Stage::with(&[header(), Value::Null]);
    assert_eq!(
        reason(&stage.look(0, &Options::default())),
        "unreadable: record 1 is null, where an object was read"
    );
    stage.write_text("\n  \n");
    assert_eq!(
        reason(&stage.look(0, &Options::default())),
        "unreadable: empty pi session hazy-ridge"
    );
    // A session of its header alone is a record, and says nothing of a turn.
    stage.write(&[header()]);
    let record = stage.at(0);
    assert!(record.items.is_empty() && !record.in_flight);
    assert_eq!(record.settlement, Settlement::Unknown);
}

#[test]
fn a_step_that_names_no_end_keeps_the_turn_open_and_the_failure_as_it_was() {
    let mut stage = Stage::with(&[
        header(),
        user("u1", "go"),
        assistant("a1", "error", "provider down"),
        calling("a2", &json!("c1")),
    ]);
    // Tool use ends nothing and clears no failure: the file is quiet, the turn open.
    let record = stage.at(1_000_000);
    assert!(record.failed && record.in_flight);
    assert_eq!(record.settlement, Settlement::InFlight);
    // A step with no stop reason is no end either.
    stage.write(&[
        header(),
        user("u1", "go"),
        assistant("a1", "error", "provider down"),
        message("a2", json!({ "role": "assistant", "content": [] })),
    ]);
    let record = stage.at(1_000_000);
    assert!(record.failed && record.in_flight);
    // A user message clears the failure.
    stage.write(&[
        header(),
        assistant("a1", "error", "provider down"),
        user("u1", "again"),
    ]);
    let record = stage.at(0);
    assert!(!record.failed && record.in_flight);
}

#[test]
fn a_refusal_names_its_quota_and_only_the_next_assistant_step_clears_it() {
    let refused = || {
        failing(
            "a1",
            &json!("429: slow down. Resets in 2 days."),
            &json!(1_790_000_000_000_i64),
        )
    };
    let mut stage = Stage::with(&[header(), user("u1", "go"), refused()]);
    let first = stage.at(0);
    let quota = Quota::Exhausted {
        at: Some("2026-09-21T14:13:20.000Z".to_owned()),
        resets_at: Some("2026-09-23T14:13:20.000Z".to_owned()),
    };
    assert_eq!(first.quota.as_deref(), Some(&quota));
    assert!(first.failed);
    // The same object, look after look, until the transcript changes it.
    let again = stage.at(500);
    assert!(Arc::ptr_eq(
        first.quota.as_ref().unwrap(),
        again.quota.as_ref().unwrap()
    ));
    // A user message ends the failure but not the quota.
    stage.write(&[header(), user("u1", "go"), refused(), user("u2", "again")]);
    let record = stage.at(0);
    assert!(!record.failed && record.in_flight);
    assert_eq!(record.quota.as_deref(), Some(&quota));
    stage.write(&[
        header(),
        user("u1", "go"),
        refused(),
        user("u2", "again"),
        assistant("a2", "stop", "done"),
    ]);
    assert_eq!(stage.at(0).quota, None);
}

#[test]
fn a_failure_is_the_text_string_makes_of_it() {
    let quota_of = |error: &Value| {
        let mut stage = Stage::with(&[header(), failing("a1", error, &json!(0))]);
        stage.at(0).quota.is_some()
    };
    // `String(429)`, and a list of one text, read as the text they are.
    assert!(quota_of(&json!(429)));
    assert!(quota_of(&json!("OpenAI API error (429): x")));
    assert!(quota_of(&json!(["402: more credits"])));
    // `[object Object]`, and the empty text, and a number that is no status.
    for error in [json!({}), json!(null), json!(""), json!(500), json!(true)] {
        assert!(!quota_of(&error), "{error}");
    }
    // No message at all is `provider error`: a failure, no quota.
    let mut stage = Stage::with(&[
        header(),
        message(
            "a1",
            json!({ "role": "assistant", "stopReason": "error", "content": [] }),
        ),
    ]);
    let record = stage.at(0);
    assert!(record.failed && record.quota.is_none());
}

#[test]
fn the_time_of_a_refusal_is_what_number_makes_of_the_message_time() {
    let quota_at = |at: Option<Value>| {
        let mut fields = json!({ "role": "assistant", "stopReason": "error",
            "errorMessage": "429: x", "content": [] });
        if let Some(at) = at {
            fields["timestamp"] = at;
        }
        let mut stage = Stage::with(&[header(), message("a1", fields)]);
        match &*stage.look(0, &Options::default()) {
            Reading::Known(record) => {
                Ok(serde_json::to_value(record.quota.as_deref()).unwrap()["at"].clone())
            }
            Reading::Unknown(reason) => Err(reason.clone()),
        }
    };
    let ok = |at: &str| Ok(json!(at));
    assert_eq!(
        quota_at(Some(json!("1790000000000"))),
        ok("2026-09-21T14:13:20.000Z")
    );
    assert_eq!(
        quota_at(Some(json!(1_790_000_000_000.9))),
        ok("2026-09-21T14:13:20.000Z")
    );
    assert_eq!(quota_at(Some(json!(null))), ok("1970-01-01T00:00:00.000Z"));
    assert_eq!(quota_at(Some(json!(true))), ok("1970-01-01T00:00:00.001Z"));
    assert_eq!(quota_at(Some(json!([5]))), ok("1970-01-01T00:00:00.005Z"));
    // `NaN` is no time at all: none is said.
    for at in [None, Some(json!("soon")), Some(json!({}))] {
        assert_eq!(quota_at(at.clone()), Ok(json!(null)), "{at:?}");
    }
    // A time past what a date holds fails the look, where `toISOString` threw.
    assert_eq!(
        quota_at(Some(json!(9e15))),
        Err("unreadable: Invalid time value".to_owned())
    );
}

#[test]
fn an_item_is_at_the_time_of_its_record_else_of_its_message_else_at_its_place() {
    let at_of = |record: Value| {
        let mut stage = Stage::with(&[header(), record]);
        stage.at(0).items[0].at.clone().unwrap()
    };
    let with = |mut record: Value, at: Value| {
        record["message"]["timestamp"] = at;
        record
    };
    let without_a_time = || {
        let mut record = user("u1", "go");
        record.as_object_mut().unwrap().remove("timestamp");
        record
    };
    assert_eq!(at_of(user("u1", "go")), json!("2026-08-24T18:00:01.000Z"));
    // `??` passes over null as well as nothing.
    let mut nulled = user("u1", "go");
    nulled["timestamp"] = json!(null);
    assert_eq!(
        at_of(with(nulled, json!(1_787_594_400_933_i64))),
        json!(1_787_594_400_933_i64)
    );
    assert_eq!(at_of(with(without_a_time(), json!("then"))), json!("then"));
    // None of either: the place of the record among the session's, from 0.
    assert_eq!(at_of(without_a_time()), json!(1));
    assert_eq!(at_of(with(without_a_time(), json!(null))), json!(1));
}

#[test]
fn open_tool_calls_are_keyed_as_a_set_keys_them() {
    let open_after = |records: Vec<Value>| {
        let mut stage = Stage::with(&[vec![header(), user("u1", "go")], records].concat());
        // A quiet file: only an open call keeps the turn in flight.
        stage.at(1_000_000).in_flight
    };
    let stopped = || assistant("a9", "stop", "done");
    // A call answered under the same id is closed.
    assert!(!open_after(vec![
        calling("a1", &json!("c1")),
        result("t1", &json!("c1")),
        stopped(),
    ]));
    // The number 1 and the text "1" are two calls; 1 and 1.0 are one.
    assert!(open_after(vec![
        calling("a1", &json!(1)),
        result("t1", &json!("1")),
        stopped(),
    ]));
    assert!(!open_after(vec![
        calling("a1", &json!(1)),
        result("t1", &json!(1.0)),
        stopped(),
    ]));
    // An id JavaScript takes for false opens nothing.
    for falsy in [json!(0), json!(""), json!(null), json!(false)] {
        assert!(
            !open_after(vec![calling("a1", &falsy), stopped()]),
            "{falsy}"
        );
    }
    // Two objects are two keys, however alike: a call by one is never closed.
    assert!(open_after(vec![
        calling("a1", &json!({ "a": 1 })),
        result("t1", &json!({ "a": 1 })),
        stopped(),
    ]));
    // A user message closes every call the turn left open.
    assert!(!open_after(vec![
        calling("a1", &json!("c1")),
        user("u2", "next"),
        stopped(),
    ]));
}

#[test]
fn a_message_needs_an_id_whatever_its_role_and_a_custom_one_only_when_it_says_something() {
    let fails = |records: Vec<Value>| {
        let mut stage = Stage::with(&[vec![header()], records].concat());
        reason(&stage.look(0, &Options::default())).to_owned()
    };
    let mut system = message("s1", json!({ "role": "system", "content": "x" }));
    system.as_object_mut().unwrap().remove("id");
    assert_eq!(
        fails(vec![system]),
        "unreadable: missing native pi message id at record 1"
    );
    assert_eq!(
        fails(vec![user("", "go")]),
        "unreadable: missing native pi message id at record 1"
    );
    let custom = |content: Value| json!({ "type": "custom_message", "content": content });
    assert_eq!(
        fails(vec![custom(json!("a note"))]),
        "unreadable: missing native pi custom message id at record 1"
    );
    // No text, no item, and so no id to ask for.
    let mut stage = Stage::with(&[
        header(),
        custom(json!("")),
        custom(json!([{ "type": "image" }])),
        custom(json!(5)),
    ]);
    assert!(stage.at(0).items.is_empty());
    // A text is a string as it is, or the text blocks of a list.
    let mut named = custom(json!([
        { "type": "text", "text": "one" },
        { "type": "text", "text": "two" },
    ]));
    named["id"] = json!("n1");
    let mut stage = Stage::with(&[header(), named]);
    let item = stage.at(0).items[0].clone();
    assert_eq!((&*item.text, item.role), ("one\ntwo", Role::Custom));
}

#[test]
fn text_is_the_text_blocks_a_line_each_and_a_user_of_white_space_is_no_turn() {
    let blocks = json!([
        { "type": "text", "text": "one" },
        { "type": "thinking", "text": "hidden" },
        { "type": "text", "text": 5 },
        { "type": "text" },
        "two",
        null,
        { "type": "text", "text": "three" },
    ]);
    let mut stage = Stage::with(&[
        header(),
        message("u1", json!({ "role": "user", "content": blocks })),
        message(
            "a1",
            json!({ "role": "assistant", "stopReason": "stop", "content": "text, not a list" }),
        ),
    ]);
    let record = stage.at(0);
    assert_eq!(&*record.items[0].text, "one\nthree");
    assert_eq!(&*record.items[1].text, "");
    // What JavaScript trims is white space, U+FEFF and U+00A0 among it; U+0085 is not.
    for (text, is_a_turn) in [(" \u{A0}\u{FEFF}\n", false), ("\u{85}", true), ("", false)] {
        let mut stage = Stage::with(&[header(), user("u1", text)]);
        assert_eq!(!stage.at(0).items.is_empty(), is_a_turn, "{text:?}");
    }
}
