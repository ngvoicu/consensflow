//! An error of the API, and which of them are a refusal for quota: a 429, or
//! an error that says `rate_limit`, each of the two alone.

use super::*;

const LIMIT: &str = "You've hit your limit. Resets in 2 hours.";
const AT: &str = "2026-09-19T10:00:00.000Z";

/// The user's turn and the refused message, which says `LIMIT` at `AT`.
fn items(refused_at: &str) -> String {
    format!(r#"[["u1","user","Hello",true,0],["m1","assistant","{LIMIT}",false,{refused_at}]]"#)
}

/// A reading of a failure, its turn settled natively, holding the quota or none.
fn failed(items: &str, quota: bool) -> String {
    let quota = if quota {
        format!(r#"{{"state":"exhausted","at":"{AT}","resetsAt":"2026-09-19T12:00:00.000Z"}}"#)
    } else {
        "null".to_owned()
    };
    format!(
        r#"{{"items":{items},"inFlight":false,"asking":false,"failed":true,"quota":{quota},"settlement":"settled"}}"#
    )
}

/// A refusal with `fields`, written as JSON text.
fn with(fields: &str) -> Value {
    refused("a1", serde_json::from_str(fields).unwrap())
}

#[test]
fn a_status_of_429_and_an_error_that_is_rate_limit_are_each_a_refusal_for_quota() {
    // Node's answer for each: the number 429 alone, the text `rate_limit` alone.
    let cases: &[(&str, bool)] = &[
        (r#"{"apiErrorStatus":429}"#, true),
        (r#"{"apiErrorStatus":429.0}"#, true),
        (r#"{"error":"rate_limit"}"#, true),
        (r#"{"apiErrorStatus":429,"error":"rate_limit"}"#, true),
        (r#"{"apiErrorStatus":"429"}"#, false),
        (r#"{"apiErrorStatus":[429]}"#, false),
        (r#"{"apiErrorStatus":true}"#, false),
        (r#"{"apiErrorStatus":402}"#, false),
        (r#"{"apiErrorStatus":500,"error":"overloaded"}"#, false),
        (r#"{"error":"Rate_Limit"}"#, false),
        (r#"{"error":["rate_limit"]}"#, false),
        ("{}", false),
    ];
    for &(fields, quota) in cases {
        assert_eq!(
            once(&[hello(), with(fields)]),
            failed(&items(&format!("\"{AT}\"")), quota),
            "{fields}"
        );
    }
}

#[test]
fn a_record_that_is_not_an_api_error_is_none_whatever_its_status() {
    for fields in [
        r#"{"apiErrorStatus":429,"isApiErrorMessage":false}"#,
        r#"{"apiErrorStatus":429,"isApiErrorMessage":"true"}"#,
    ] {
        assert_eq!(
            once(&[hello(), with(fields)]),
            in_flight(&items(&format!("\"{AT}\""))),
            "{fields}"
        );
    }
}

#[test]
fn a_refusal_of_a_message_that_had_answered_takes_its_stop_hooks_back() {
    let answered = || {
        assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([text(json!("Hi"))]),
            json!("end_turn"),
        )
    };
    let refusal = |id: &str| {
        in_message(
            refused("a2", json!({ "apiErrorStatus": 429 })),
            "id",
            Some(json!(id)),
        )
    };
    let back = |id: &str| {
        assistant(
            "a3",
            "a2",
            json!(id),
            json!([text(json!("Back"))]),
            json!("end_turn"),
        )
    };
    let refused_text = format!("Hi\\n{LIMIT}");
    // Node's answer for each: the next message's answer settles the turn.
    assert_eq!(
        once(&[
            hello(),
            answered(),
            refusal("m1"),
            back("m2"),
            duration("d1", "a3")
        ]),
        settled(&format!(
            r#"[["u1","user","Hello",true,0],["m1","assistant","{refused_text}",false,"{AT}"],["m2","assistant","Back",true,3]]"#
        ))
    );
    assert_eq!(
        once(&[
            hello(),
            answered(),
            refusal("m1"),
            back("m1"),
            duration("d1", "a3")
        ]),
        settled(&format!(
            r#"[["u1","user","Hello",true,0],["m1","assistant","{refused_text}\nBack",true,3]]"#
        ))
    );
    assert_eq!(
        once(&[
            hello(),
            answered(),
            refusal("m2"),
            back("m3"),
            duration("d1", "a3")
        ]),
        settled(&format!(
            r#"[["u1","user","Hello",true,0],["m1","assistant","Hi",false,1],["m2","assistant","{LIMIT}",false,"{AT}"],["m3","assistant","Back",true,3]]"#
        ))
    );
}

#[test]
fn a_refusal_is_the_latest_assistant_record_s_to_say_and_a_prompt_does_not_take_it_back() {
    let said = |at: &str| items(&format!("\"{at}\""));
    let refusal = || refused("a2", json!({ "apiErrorStatus": 429 }));
    let answer = |uuid: &str, id: &str, content: &str, stop: Value| {
        assistant(uuid, "a1", json!(id), json!([text(json!(content))]), stop)
    };
    // A refusal that is a later fragment of the message that answered.
    let fragment = r#"[["u1","user","Hello",true,0],["m1","assistant","Hi\nYou've hit your limit. Resets in 2 hours.",false,"2026-09-19T10:00:00.000Z"]]"#;
    let answered = assistant(
        "a1",
        "u1",
        json!("m1"),
        json!([text(json!("Hi"))]),
        json!("end_turn"),
    );
    assert_eq!(
        once(&[hello(), answered.clone(), refusal()]),
        failed(fragment, true)
    );
    // The boundary after it ends nothing: the refusal had taken the answer back.
    assert_eq!(
        once(&[hello(), answered, refusal(), duration("d1", "a2")]),
        failed(fragment, true)
    );
    // The call a refusal's own message left open keeps its turn in flight; another message's does not.
    let calling = |id: &str| {
        assistant(
            "a0",
            "u1",
            json!(id),
            json!([tool_use(json!("t1"))]),
            Value::Null,
        )
    };
    let refused_items = said(AT);
    assert_eq!(
        once(&[hello(), calling("m1"), refusal()]),
        format!(
            r#"{{"items":{refused_items},"inFlight":true,"asking":false,"failed":true,"quota":{{"state":"exhausted","at":"{AT}","resetsAt":"2026-09-19T12:00:00.000Z"}},"settlement":"in-flight"}}"#
        )
    );
    let two = format!(
        r#"[["u1","user","Hello",true,0],["m0","assistant","",false,1],["m1","assistant","{LIMIT}",false,"{AT}"]]"#
    );
    assert_eq!(
        once(&[hello(), calling("m0"), refusal()]),
        failed(&two, true)
    );
    // A prompt after it leaves the quota, and no failure: the turn is not over.
    let prompted = format!(
        r#"{{"items":[["u1","user","Hello",true,0],["m1","assistant","{LIMIT}",false,"{AT}"],["u2","user","Try again",true,2]],"inFlight":true,"asking":false,"failed":false,"quota":{{"state":"exhausted","at":"{AT}","resetsAt":"2026-09-19T12:00:00.000Z"}},"settlement":"in-flight"}}"#
    );
    let first = refused("a1", json!({ "apiErrorStatus": 429 }));
    assert_eq!(
        once(&[
            hello(),
            first.clone(),
            user("u2", json!("a1"), json!("Try again"))
        ]),
        prompted
    );
    // An answer after it has the last word on quota and on failure.
    let back = format!(
        r#"{{"items":[["u1","user","Hello",true,0],["m1","assistant","{LIMIT}",false,"{AT}"],["m2","assistant","Back",true,2]],"inFlight":false,"asking":false,"failed":false,"quota":null,"settlement":"settled"}}"#
    );
    assert_eq!(
        once(&[
            hello(),
            first.clone(),
            answer("a2", "m2", "Back", json!("end_turn")),
            duration("d1", "a2")
        ]),
        back
    );
    // And so does a fragment of the refused message itself.
    let more = format!(
        r#"{{"items":[["u1","user","Hello",true,0],["m1","assistant","{LIMIT}\nMore",false,2]],"inFlight":true,"asking":false,"failed":false,"quota":null,"settlement":"in-flight"}}"#
    );
    assert_eq!(
        once(&[hello(), first, answer("a2", "m1", "More", Value::Null)]),
        more
    );
}

#[test]
fn anthropics_400_for_spent_extra_usage_is_a_refusal_for_quota_that_names_no_reset() {
    // Claude Code 2.1.306 writes Anthropic's 400 as an API error that is
    // `invalid_request`; the words are the ones Pi's sessions on this Mac hold
    // (the review of the quota wait, T-88).
    let spent = "You're out of extra usage. Add more at claude.ai/settings/usage and keep going.";
    let record = having(
        assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([text(json!(spent))]),
            Value::Null,
        ),
        json!({
            "isApiErrorMessage": true,
            "timestamp": AT,
            "apiErrorStatus": 400,
            "error": "invalid_request",
        }),
    );
    assert_eq!(
        once(&[hello(), record]),
        format!(
            r#"{{"items":[["u1","user","Hello",true,0],["m1","assistant","{spent}",false,"{AT}"]],"inFlight":false,"asking":false,"failed":true,"quota":{{"state":"exhausted","at":"{AT}","resetsAt":null}},"settlement":"settled"}}"#
        )
    );
    // Another 400 is no refusal for quota.
    let other = having(
        assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([text(json!("Prompt is too long"))]),
            Value::Null,
        ),
        json!({
            "isApiErrorMessage": true,
            "timestamp": AT,
            "apiErrorStatus": 400,
            "error": "invalid_request",
        }),
    );
    assert!(once(&[hello(), other]).contains(r#""failed":true,"quota":null"#));
}

/// What a failed turn's record says of why, after one look at `records`.
fn failure_of(records: &[Value]) -> Option<String> {
    let mut stage = Stage::new();
    stage.write(records);
    match &*stage.reader.look(&Options::default(), 0) {
        Reading::Known(record) => record.failure.clone(),
        Reading::Unknown(reason) | Reading::Missing(reason) => panic!("unknown: {reason}"),
    }
}

#[test]
fn a_refused_turn_keeps_the_words_the_api_refused_it_in_and_the_next_turn_drops_them() {
    // A 5xx of the API's own, in the words Claude Code writes it in: no quota.
    const OVERLOADED: &str = r#"API Error: 529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
    let refusal = having(
        assistant(
            "a1",
            "u1",
            json!("m1"),
            json!([text(json!(OVERLOADED))]),
            Value::Null,
        ),
        json!({ "isApiErrorMessage": true, "apiErrorStatus": 529, "timestamp": AT }),
    );
    assert_eq!(
        failure_of(&[hello(), refusal.clone()]).as_deref(),
        Some(OVERLOADED)
    );
    assert_eq!(
        failure_of(&[hello(), with(r#"{"apiErrorStatus":429}"#)]).as_deref(),
        Some(LIMIT)
    );
    let next = user("u2", json!("a1"), json!("Try again"));
    assert_eq!(failure_of(&[hello(), refusal, next]), None);
}
