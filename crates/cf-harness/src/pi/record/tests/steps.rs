//! What a session's records say, where the goldens' scenarios never reach: ids
//! and times of kinds a record should not hold and JavaScript reads anyway,
//! tool calls keyed by what JavaScript keys a `Set` by, a turn that names no
//! end, what a refusal's quota outlives, and a record that is `null`.

use super::*;

/// A made Pi 1.5.0 session, keeping the field order of the benchmark's cut
/// message but none of its words. `content` is a JSON array of our own.
fn length_stop(content: &str) -> Stage {
    let mut stage = Stage::new();
    stage.write_text(&format!(
        concat!(
            "{{\"type\":\"session\",\"version\":3,\"id\":\"hazy-ridge\",\"timestamp\":\"2026-08-24T18:00:00.703Z\"}}\n",
            "{{\"type\":\"message\",\"id\":\"u1\",\"parentId\":null,\"timestamp\":\"2026-08-24T18:00:01.000Z\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"text\",\"text\":\"Inspect the parser\"}}]}}}}\n",
            "{{\"type\":\"message\",\"id\":\"a1\",\"parentId\":\"u1\",\"timestamp\":\"2026-08-24T18:00:02.000Z\",\"message\":{{",
            "\"role\":\"assistant\",\"content\":{content},\"api\":\"made-api\",\"provider\":\"made-provider\",\"model\":\"made-model\",",
            "\"usage\":{{\"input\":1,\"output\":2,\"cacheRead\":0,\"cacheWrite\":0,\"reasoning\":2,\"totalTokens\":3,\"cost\":{{\"input\":0,\"output\":0,\"cacheRead\":0,\"cacheWrite\":0,\"total\":0,\"totalFromProvider\":0}}}},",
            "\"stopReason\":\"length\",\"timestamp\":1790000002000,\"responseId\":\"made-response\",\"rawStopReason\":\"length\",\"durationMs\":1000,\"thinkingLevel\":\"high\"}}}}\n"
        ),
        content = content
    ));
    stage
}

fn assert_length_failure(mut stage: Stage) {
    let before_end = stage.at(1_000);
    assert!(
        before_end.in_flight && !before_end.failed,
        "Pi may still continue"
    );
    let record = stage.at(120_000);
    assert!(!record.in_flight, "the cut turn must be over: {record:?}");
    assert_eq!(record.settlement, Settlement::Settled);
    assert!(record.failed);
    assert!(record.quota.is_none());
    assert_eq!(record.failure, None);
    assert_eq!(
        record.failure_kind.as_deref(),
        Some("the member's reply was cut at the model's output limit; nothing was taken as its result")
    );
    assert!(
        !record.items.last().unwrap().complete,
        "a cut reply is not a result"
    );
}

#[test]
fn a_cut_tool_call_is_followed_by_pis_error_result_and_continuation() {
    // Pi 1.5.0 declines a truncated tool call, records its error result and
    // asks the model again. That cut message is not the agent's end.
    let cut = || {
        message(
            "a1",
            json!({"role":"assistant", "stopReason":"length",
        "content":[{"type":"toolCall", "id":"c1", "name":"read", "arguments":{}}]}),
        )
    };
    let mut stage = Stage::with(&[
        header(),
        user("u1", "go"),
        cut(),
        result("r1", &json!("c1")),
    ]);
    let waiting = stage.at(120_000);
    assert!(waiting.in_flight && !waiting.failed);
    assert_eq!(waiting.settlement, Settlement::InFlight);
    stage.write(&[
        header(),
        user("u1", "go"),
        cut(),
        result("r1", &json!("c1")),
        assistant("a2", "stop", "Inspection complete."),
    ]);
    let done = stage.at(120_000);
    assert_eq!(done.settlement, Settlement::Settled);
    assert!(!done.failed && done.failure_kind.is_none());
}

#[test]
fn a_length_stop_after_only_thinking_ends_the_turn_as_a_failure() {
    assert_length_failure(length_stop(
        r#"[{"type":"thinking","thinking":"Let me inspect the parser.","thinkingSignature":"made"}]"#,
    ));
}

#[test]
fn a_length_stop_after_some_text_ends_the_turn_as_a_failure() {
    assert_length_failure(length_stop(
        r#"[{"type":"thinking","thinking":"First check the parser.","thinkingSignature":"made"},{"type":"text","text":"The parser appears to"}]"#,
    ));
}

#[test]
fn a_deferred_response_ends_the_turn_as_a_failure_without_a_result() {
    // Pi 1.5.0's agent loop ends after a deferred response with no tool calls:
    // the provider returned a handle, not an answer to this task.
    let mut stage = Stage::with(&[
        header(),
        user("u1", "Inspect the parser"),
        message(
            "a1",
            json!({
                "role": "assistant", "content": [], "stopReason": "deferred",
                "deferred": { "provider": "made-provider", "modelId": "made-model",
                    "api": "made-api", "id": "made-handle" }
            }),
        ),
    ]);
    let record = stage.at(120_000);
    assert!(
        !record.in_flight,
        "the deferred turn must be over: {record:?}"
    );
    assert_eq!(record.settlement, Settlement::Settled);
    assert!(record.failed && record.quota.is_none());
    assert_eq!(record.failure, None);
    assert_eq!(
        record.failure_kind.as_deref(),
        Some("the provider deferred the member's reply; no result was returned")
    );
    assert!(!record.items.last().unwrap().complete);
}

#[test]
fn a_later_message_clears_the_cut_failure_and_pending_is_not_an_end() {
    let cut = || assistant("a1", "length", "partial");
    for next in [
        assistant("a2", "stop", "complete"),
        user("u2", "try another way"),
    ] {
        let mut stage = Stage::with(&[header(), user("u1", "go"), cut(), next]);
        let record = stage.at(120_000);
        assert!(!record.failed && record.failure_kind.is_none());
    }
    let mut stage = Stage::with(&[
        header(),
        user("u1", "go"),
        cut(),
        failing("a2", &json!("provider error"), &json!(0)),
    ]);
    let record = stage.at(120_000);
    assert!(record.failed && record.failure_kind.is_none());
    assert_eq!(record.failure.as_deref(), Some("provider error"));
    let mut pending = Stage::with(&[header(), user("u1", "go"), assistant("a1", "pending", "")]);
    assert!(pending.at(120_000).in_flight);
}

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

/// An assistant's step that said `text` and called the tool `call`.
fn saying_and_calling(id: &str, text: &str, call: &Value) -> Value {
    message(
        id,
        json!({ "role": "assistant", "stopReason": "toolUse",
            "content": [{ "type": "text", "text": text },
                        { "type": "toolCall", "id": call, "name": "read" }] }),
    )
}

#[test]
fn the_steps_of_a_turn_that_stopped_for_a_tool_are_never_complete_and_the_step_that_stopped_is() {
    // The owner's decision of 2026-10-15: a result is what the member wrote after its
    // last tool call. Pi's steps that said something and called a tool stop for it
    // (`toolUse`): they are progress, and only the step that stopped is complete.
    let mut stage = Stage::with(&[
        header(),
        user("u1", "Review the commit"),
        saying_and_calling("a1", "Still reviewing", &json!("c1")),
        result("t1", &json!("c1")),
        saying_and_calling("a2", "Now the tests", &json!("c2")),
        result("t2", &json!("c2")),
        assistant("a3", "stop", "PASS: no findings."),
    ]);
    let record = stage.at(121_000);

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

#[test]
fn a_step_that_names_no_end_keeps_the_turn_open_without_failing_it() {
    let mut stage = Stage::with(&[
        header(),
        user("u1", "go"),
        assistant("a1", "error", "provider down"),
        calling("a2", &json!("c1")),
    ]);
    // Tool use ends nothing: even with a quiet file, the turn is still open.
    let record = stage.at(1_000_000);
    assert!(!record.failed && record.in_flight);
    assert_eq!(record.settlement, Settlement::InFlight);
    // A step with no stop reason is no end either.
    stage.write(&[
        header(),
        user("u1", "go"),
        assistant("a1", "error", "provider down"),
        message("a2", json!({ "role": "assistant", "content": [] })),
    ]);
    let record = stage.at(1_000_000);
    assert!(!record.failed && record.in_flight);
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
fn a_refusal_names_its_quota_and_a_genuine_user_turn_or_the_next_step_clears_it() {
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
    assert!(!first.failed && first.in_flight);
    // The same object, look after look, until the transcript changes it.
    let again = stage.at(500);
    assert!(Arc::ptr_eq(
        first.quota.as_ref().unwrap(),
        again.quota.as_ref().unwrap()
    ));
    // A tool's result opens no turn: the refusal stays in view.
    stage.write(&[
        header(),
        user("u1", "go"),
        refused(),
        result("t1", &json!("c1")),
    ]);
    assert_eq!(stage.at(0).quota.as_deref(), Some(&quota));
    // A user message with words takes the refusal out of view.
    stage.write(&[header(), user("u1", "go"), refused(), user("u2", "again")]);
    let record = stage.at(0);
    assert!(!record.failed && record.in_flight);
    assert_eq!(record.quota, None);
    // A message of white space is no turn.
    stage.write(&[header(), user("u1", "go"), refused(), user("u2", "  ")]);
    assert_eq!(stage.at(0).quota.as_deref(), Some(&quota));
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
fn pi_reads_a_refusal_of_the_accounts_billing_or_login_as_the_account_unavailable_and_no_quota() {
    // MUS-23: OpenRouter's spent credit (a 402) and a login Pi could not refresh (a 401) have
    // no reset, as a 429's has: the account is unavailable until its human fixes it.
    for (error, said) in [
        (
            "402: {\"message\":\"This request requires more credits, or fewer max_tokens.\"}",
            "its billing was refused: 402:",
        ),
        (
            "OAuth refresh failed for openai-codex: OpenAI Codex token refresh failed (401)",
            "its login was refused: OAuth refresh failed",
        ),
    ] {
        let mut stage = Stage::with(&[
            header(),
            failing("a1", &json!(error), &json!(1_790_000_000_000_i64)),
        ]);
        let record = stage.at(0);
        assert!(!record.failed && record.in_flight, "{error}");
        let Some(Quota::Unavailable { at, because }) = record.quota.as_deref() else {
            panic!("{error}: {:?}", record.quota);
        };
        assert_eq!(at.as_deref(), Some("2026-09-21T14:13:20.000Z"), "{error}");
        assert!(because.starts_with(said), "{because}");
        assert_eq!(record.quota.as_deref().unwrap().resets_at(), None);
    }
    // A 429 is still a limit that resets.
    let mut stage = Stage::with(&[
        header(),
        failing(
            "a1",
            &json!("429: slow down"),
            &json!(1_790_000_000_000_i64),
        ),
    ]);
    assert!(matches!(
        stage.at(0).quota.as_deref(),
        Some(Quota::Exhausted { .. })
    ));
}

/// What Pi's record makes of a failed step's words: a limit that resets, an account that
/// refused (billing or login), a refusal for a moment, or the turn's own failure.
fn read_pi_failure(error: &str) -> &'static str {
    let mut stage = Stage::with(&[
        header(),
        failing("a1", &json!(error), &json!(1_790_000_000_000_i64)),
    ]);
    let record = stage.at(0);
    match record.quota.as_deref() {
        Some(Quota::Exhausted { .. }) => "resets",
        Some(Quota::Busy { .. }) => "moment",
        Some(Quota::Unavailable { because, .. }) if because.starts_with("its billing") => "billing",
        Some(Quota::Unavailable { .. }) => "login",
        // Pi tries its first error again itself (T-1958): no hold, no failure yet.
        _ => "none",
    }
}

/// The words Pi 1.6.1 records for each provider's refusals (read from its published
/// `@earendil-works/pi-ai` and `pi-coding-agent`, 2026-11-26): the provider's own body behind its
/// status, and for an OAuth login the refresh's own sentence.
#[test]
fn pi_reads_each_providers_refusal_for_what_it_is() {
    for (error, read) in [
        // A limit that resets is never an account's refusal, whatever else its words say.
        (
            r#"429 {"type":"error","error":{"type":"rate_limit_error","message":"This request would exceed your account's rate limit. Please try again later."},"request_id":"req_011"}"#,
            "resets",
        ),
        (
            r#"400 {"type":"error","error":{"type":"invalid_request_error","message":"You're out of extra usage. Add more at claude.ai/settings/usage and keep going."}}"#,
            "resets",
        ),
        (
            r#"openai-codex API error (429): {"error":{"type":"usage_limit_reached","message":"The usage limit has been reached","plan_type":"plus","resets_in_seconds":3600}}"#,
            "resets",
        ),
        (
            "openai-codex API error (429): {\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\"}}\nCheck your ChatGPT usage: https://chatgpt.com/settings/usage",
            "resets",
        ),
        (
            "OpenAI API error (429): Rate limit reached for gpt-5 on tokens per min (TPM): Limit 30000, Used 29000. Please try again in 2s. You can increase your rate limit by adding a payment method at https://platform.openai.com/account/billing.",
            "resets",
        ),
        (
            r#"429 {"error":{"code":429,"message":"You exceeded your current quota, please check your plan and billing details.","status":"RESOURCE_EXHAUSTED"}}"#,
            "resets",
        ),
        // An account's refusal that no wait cures.
        (
            r#"400 {"type":"error","error":{"type":"invalid_request_error","message":"Your credit balance is too low to access the Anthropic API."}}"#,
            "billing",
        ),
        (
            r#"OpenAI API error (429): {"error":{"code":"insufficient_quota","message":"You exceeded your current quota"}}"#,
            "billing",
        ),
        (
            r#"401 {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
            "login",
        ),
        (
            "OAuth refresh failed for openai-codex: OpenAI Codex token refresh failed (401): {\"error\":\"refresh_token_reused\"}",
            "login",
        ),
        (
            "OAuth refresh failed for anthropic: Anthropic token refresh request failed. url=https://platform.claude.com/v1/oauth/token; details=Error: HTTP request failed. status=401; url=https://platform.claude.com/v1/oauth/token; body={\"error\":\"invalid_grant\"}; stack=Error: HTTP request failed",
            "login",
        ),
        // A refresh refused for a moment is no login refusal: a server's error is tried again,
        // and a network's is Pi's own to try again.
        (
            "OAuth refresh failed for anthropic: Anthropic token refresh request failed. url=https://platform.claude.com/v1/oauth/token; details=Error: HTTP request failed. status=503; url=https://platform.claude.com/v1/oauth/token; body=Service Unavailable",
            "moment",
        ),
        (
            "OAuth refresh failed for openai-codex: OpenAI Codex token refresh failed (502): Bad Gateway",
            "moment",
        ),
        (
            "OAuth refresh failed for anthropic: Anthropic token refresh request failed. url=https://platform.claude.com/v1/oauth/token; details=TypeError: fetch failed; cause=Error: getaddrinfo ENOTFOUND platform.claude.com",
            "none",
        ),
        (
            "OAuth refresh failed for openai-codex: OpenAI Codex token refresh error: fetch failed",
            "none",
        ),
        (
            "Authentication failed for \"anthropic\". Credentials may have expired or network is unavailable. Run '/login anthropic' to re-authenticate.",
            "none",
        ),
        (
            r#"529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            "moment",
        ),
        // An access token that ran out is renewed by Pi: no hold, Pi tries it again.
        (
            r#"401 {"type":"error","error":{"type":"authentication_error","message":"OAuth token has expired. Please obtain a new token or refresh your existing token."}}"#,
            "none",
        ),
        (
            r#"openai-codex API error (401): {"error":{"message":"Your authentication token has expired. Please try signing in again.","code":"token_expired"}}"#,
            "none",
        ),
    ] {
        assert_eq!(read_pi_failure(error), read, "{error}");
    }
}

#[test]
fn an_expired_token_that_fails_a_turn_again_is_a_failed_turn_and_no_hold() {
    let expired = json!(
        r#"401 {"type":"error","error":{"type":"authentication_error","message":"OAuth token has expired. Please obtain a new token or refresh your existing token."}}"#
    );
    let mut stage = Stage::with(&[
        header(),
        user("u1", "go"),
        failing("a1", &expired, &json!(1_790_000_000_000_i64)),
        user("u2", "again"),
        failing("a2", &expired, &json!(1_790_000_001_000_i64)),
    ]);
    // Settled: the record has been quiet for longer than Pi's own retry takes.
    let record = stage.at(600_000);
    assert!(record.failed, "the turn failed, for the engine's tries");
    assert!(record.quota.is_none(), "{:?}", record.quota);
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
    // `[object Object]`, and the empty text, and a number that is no refusal's status (a
    // 500 is a server's error: a refusal for a moment, T-1112).
    for error in [json!({}), json!(null), json!(""), json!(200), json!(true)] {
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
    assert!(!record.failed && record.in_flight && record.quota.is_none());
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
            Reading::Unknown(reason) | Reading::Missing(reason) => Err(reason.clone()),
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
    // A JSON `null` is no time, as an absent one is.
    assert_eq!(quota_at(Some(json!(true))), ok("1970-01-01T00:00:00.001Z"));
    assert_eq!(quota_at(Some(json!([5]))), ok("1970-01-01T00:00:00.005Z"));
    // `NaN` is no time at all: none is said.
    for at in [
        None,
        Some(json!(null)),
        Some(json!("soon")),
        Some(json!({})),
    ] {
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

#[test]
fn a_failed_step_keeps_pis_words_for_it_and_the_next_message_drops_them() {
    let failed = || failing("a1", &json!("Model gpt-0-nonesuch not found"), &json!(0));
    let mut stage = Stage::with(&[header(), user("u1", "go"), failed()]);
    let record = stage.at(120_000);
    assert!(record.failed);
    assert_eq!(
        record.failure.as_deref(),
        Some("Model gpt-0-nonesuch not found")
    );
    // `provider error` is Node's word for none: Pi said nothing, so neither does the record.
    let mut silent = Stage::with(&[
        header(),
        message(
            "a1",
            json!({ "role": "assistant", "stopReason": "error", "content": [] }),
        ),
    ]);
    let record = silent.at(120_000);
    assert!(record.failed);
    assert_eq!(record.failure, None);
    stage.write(&[header(), user("u1", "go"), failed(), user("u2", "again")]);
    assert_eq!(stage.at(0).failure, None);
}

#[test]
fn a_failed_step_is_a_turn_that_ended_once_nothing_follows_it_for_the_quiet_spell() {
    // A window the extension left no evidence of (a conversation followed with
    // `/resume`, a launch folder swept, a write that failed) must not read at work for
    // good after a failed turn (T-781): Pi writes a new step for each retry.
    let failed = || failing("a1", &json!("Connection error."), &json!(0));
    let mut stage = Stage::with(&[header(), user("u1", "go"), failed()]);
    // Just written: Pi may be about to retry, so it is at work.
    let record = stage.at(1_000);
    assert!(!record.failed && record.in_flight);
    assert_eq!(record.settlement, Settlement::InFlight);
    // Past two retries' worth of quiet it is over, with Pi's words for it kept.
    let record = stage.at(120_000);
    assert!(record.failed && !record.in_flight);
    assert_eq!(record.settlement, Settlement::Settled);
    assert_eq!(record.failure.as_deref(), Some("Connection error."));
    // A retry that went through is no failed turn.
    stage.write(&[
        header(),
        user("u1", "go"),
        failed(),
        assistant("a2", "stop", "done"),
    ]);
    let record = stage.at(120_000);
    assert!(!record.failed && !record.in_flight);
    assert_eq!(record.settlement, Settlement::Settled);
}

#[test]
fn a_record_whose_newline_a_short_write_dropped_keeps_its_place_and_its_time() {
    // A message with no time of its own is at its place among the records; the
    // next append, the newline before it lost, ran on from it (T-246).
    let untimed = |id: &str, text: &str| {
        let mut record = user(id, text);
        record.as_object_mut().unwrap().remove("timestamp");
        record
    };
    let mut stage = Stage::new();
    stage.write_text(&format!("{}\n{}", header(), untimed("u1", "hello")));
    let before = stage.at(0).items;
    assert_eq!(before.len(), 1);
    stage.write_text(&format!(
        "{}\n{}{}\n",
        header(),
        untimed("u1", "hello"),
        untimed("u2", "again")
    ));
    let after = stage.at(0);
    assert_eq!(after.items.len(), 2);
    assert_eq!(after.items[0], before[0], "the same id, at the same place");
    assert_eq!(after.items[1].at, Some(json!(2)));
    assert!(after.lines.lost.is_empty());
    // Each item with its record's place, the header's at 0.
    assert_eq!(after.lines.places, Some(vec![1, 2]));
}

/// The step Pi writes of its own when a run fails or is aborted before the model's
/// first token (`handleRunFailure`, Pi 1.4.0): an empty text, no usage, and how
/// the run ended.
fn of_its_own(id: &str, stop: &str) -> Value {
    message(
        id,
        json!({ "role": "assistant", "stopReason": stop, "content": [{ "type": "text", "text": "" }],
            "usage": { "input": 0, "output": 0, "totalTokens": 0 },
            "errorMessage": "Request was aborted" }),
    )
}

#[test]
fn a_step_pi_wrote_with_no_model_behind_it_is_synthetic_and_one_the_model_began_is_not() {
    // T-530: the check that a window got a turn through took a step Pi writes of its
    // own, at an Escape before the model's first token, for an answer: it is over,
    // not failed, and dated when Pi wrote it.
    for stop in ["aborted", "error"] {
        let record = Stage::with(&[header(), user("u1", "go"), of_its_own("a1", stop)]).at(0);
        let step = record.items.last().unwrap();
        assert_eq!(
            (step.role, step.synthetic),
            (Role::Assistant, true),
            "{stop}"
        );
    }
    // A provider's abort keeps what had come: nothing, or the model's first words.
    let aborted = |content: Value| {
        let step = message(
            "a1",
            json!({ "role": "assistant", "stopReason": "aborted", "content": content }),
        );
        let record = Stage::with(&[header(), user("u1", "go"), step]).at(0);
        record.items.last().unwrap().synthetic
    };
    assert!(aborted(json!([])));
    assert!(!aborted(json!([{ "type": "text", "text": "Reading the" }])));
    assert!(!aborted(
        json!([{ "type": "thinking", "thinking": "The user wants" }])
    ));
    assert!(!aborted(
        json!([{ "type": "toolCall", "id": "c1", "name": "read" }])
    ));
    // A step that stopped is the model's, whatever it says.
    let record = Stage::with(&[header(), user("u1", "go"), assistant("a1", "stop", "")]).at(0);
    assert!(!record.items.last().unwrap().synthetic);
}

#[test]
fn a_providers_refusal_for_a_moment_is_busy_and_no_quota() {
    // T-1112: Anthropic's 529 as Pi's Anthropic provider words it (its status, then the
    // body), an OpenAI-shaped 503, words of a model at capacity; a 400 stays the turn's
    // failure, and Pi's own "provider error" (no message) is none.
    for error in [
        r#"529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        "OpenAI API error (503): Service Unavailable",
        "Provider returned error: model is at capacity",
        "The servers are currently busy, please try again",
        "520 Cloudflare: web server returned an unknown error",
    ] {
        let mut stage = Stage::with(&[
            header(),
            failing("a1", &json!(error), &json!(1_790_000_000_000_i64)),
        ]);
        let record = stage.at(0);
        assert!(!record.failed && record.in_flight, "{error}");
        assert_eq!(
            record.quota.as_deref(),
            Some(&Quota::Busy {
                at: Some("2026-09-21T14:13:20.000Z".to_owned()),
                because: error.to_owned(),
            }),
            "{error}"
        );
    }
    let mut stage = Stage::with(&[
        header(),
        failing(
            "a1",
            &json!(r#"400 {"type":"error","error":{"type":"invalid_request_error"}}"#),
            &json!(1_790_000_000_000_i64),
        ),
    ]);
    let record = stage.at(0);
    assert!(!record.failed && record.in_flight && record.quota.is_none());
}

#[test]
fn a_second_distinct_refusal_after_a_retry_is_in_view_and_the_first_is_not() {
    // T-1904's replay: the first 401, the retry's user item, the second 401.
    let refused = |id: &str| failing(id, &json!("401: Authentication failed"), &json!(null));
    let mut stage = Stage::with(&[header(), user("u1", "work"), refused("a1")]);
    let first = stage.at(0);
    assert!(!first.failed && first.in_flight && first.quota.is_some());
    assert!(stage.at(120_000).failed);
    stage.write(&[
        header(),
        user("u1", "work"),
        refused("a1"),
        user("u2", "retry"),
    ]);
    let retried = stage.at(0);
    assert!(retried.in_flight && !retried.failed);
    assert_eq!(retried.quota, None);
    stage.write(&[
        header(),
        user("u1", "work"),
        refused("a1"),
        user("u2", "retry"),
        refused("a2"),
    ]);
    let second = stage.at(0);
    assert!(!second.failed && second.in_flight && second.quota.is_some());
    assert!(stage.at(120_000).failed);
}

#[test]
fn a_retry_on_another_branch_than_the_one_pi_is_on_takes_no_refusal_out_of_view() {
    // T-1932: the file holds the prompt, the refused step and the retry's prompt; Pi was
    // put back on the refused step and saved a label or a branch summary there, so the path
    // to the leaf does not hold the retry.
    let with_parent = |mut record: Value, parent: Option<&str>| {
        record["parentId"] = parent.map_or(Value::Null, |parent| json!(parent));
        record
    };
    let refused = || {
        with_parent(
            failing("a1", &json!("401: Authentication failed"), &json!(null)),
            Some("u1"),
        )
    };
    let base = || {
        vec![
            header(),
            with_parent(user("u1", "work"), None),
            refused(),
            with_parent(user("u2", "retry"), Some("a1")),
        ]
    };
    let label = |parent: &str| {
        json!({ "type": "label", "id": "l1", "parentId": parent, "targetId": parent,
            "label": "prior result", "timestamp": "2026-08-24T18:00:03.000Z" })
    };
    let summary = json!({ "type": "branch_summary", "id": "s1", "parentId": "a1", "fromId": "a1",
        "summary": "older branch", "timestamp": "2026-08-24T18:00:03.000Z" });
    let state = |rows: Vec<Value>| {
        let mut stage = Stage::with(&rows);
        let record = stage.at(0);
        (
            record.quota.is_some(),
            record.failed,
            stage.at(120_000).failed,
        )
    };
    // The leaf is the retry's label: the retry is on the path, and the refusal is out of view.
    assert_eq!(
        state([base(), vec![label("u2")]].concat()),
        (false, false, false)
    );
    // The leaf is the refused step's label or summary: the retry is on another branch.
    assert_eq!(
        state([base(), vec![label("a1")]].concat()),
        (true, false, true)
    );
    assert_eq!(state([base(), vec![summary]].concat()), (true, false, true));
    // And with no label at all, the retry is the leaf: it clears.
    assert_eq!(state(base()), (false, false, false));
}
