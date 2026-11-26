//! What a harness's refusal says about its quota, on the words harnesses really
//! wrote: Claude Code's limits, Pi's provider errors and OpenRouter's spent
//! credit, as this machine's records held them (2026-09/10). The sentences of
//! Node's quota suite, `describe('a quota refusal')`.

use cf_base::time::iso;
use serde_json::json;

use super::*;

/// The instant the refusals are read at: `Date.parse('2026-10-03T12:00:00.000Z')`.
fn at() -> f64 {
    instant("2026-10-03T12:00:00.000Z")
}

fn at_ms() -> i64 {
    parse("2026-10-03T12:00:00.000Z").unwrap()
}

#[test]
fn reads_claude_s_reset_at_a_time_of_day_in_the_zone_it_names_as_the_next_time_it_comes() {
    // 15:00 in Bucharest, summer time (UTC+3).
    assert_eq!(
        resets(
            "You've hit your session limit · resets 7:30pm (Europe/Bucharest)",
            at()
        )
        .as_deref(),
        Some("2026-10-03T16:30:00.000Z")
    );
    assert_eq!(
        resets(
            "You've hit your session limit · resets 1:30am (Europe/Bucharest)",
            at()
        )
        .as_deref(),
        Some("2026-10-03T22:30:00.000Z"),
        "past for today: tomorrow"
    );
    assert_eq!(
        resets(
            "You've hit your weekly limit · resets 11am (Europe/Bucharest)",
            at()
        )
        .as_deref(),
        Some("2026-10-04T08:00:00.000Z")
    );
}

#[test]
fn reads_claude_s_reset_on_a_date_across_a_change_of_the_zone_s_offset() {
    assert_eq!(
        resets(
            "You've hit your weekly limit · resets Sep 29 at 11am (Europe/Bucharest)",
            instant("2026-09-26T12:00:00.000Z")
        )
        .as_deref(),
        Some("2026-09-29T08:00:00.000Z")
    );
    // Summer time ends on October 25: November is UTC+2.
    assert_eq!(
        resets(
            "You've hit your weekly limit · resets Nov 2 at 9am (Europe/Bucharest)",
            at()
        )
        .as_deref(),
        Some("2026-11-02T07:00:00.000Z")
    );
}

#[test]
fn reads_a_span_in_any_of_the_ways_providers_write_one() {
    let hours = |n: i64| iso(at_ms() + n * 3_600_000);
    assert_eq!(
        resets(
            r#"429: {"type":"GoUsageLimitError","message":"5-hour usage limit reached. Resets in 3hr 4min. To continue using this model, upgrade."}"#,
            at()
        )
        .as_deref(),
        Some(iso(at_ms() + (3 * 60 + 4) * 60_000).as_str())
    );
    assert_eq!(
        resets("Weekly usage limit reached. Resets in 2 days.", at()),
        Some(hours(48))
    );
    assert_eq!(
        resets("You've hit your limit. Resets in 2 hours.", at()),
        Some(hours(2))
    );
    assert_eq!(
        resets("Rate limit … reset in 35 minutes", at()).as_deref(),
        Some(iso(at_ms() + 35 * 60_000).as_str())
    );
}

#[test]
fn names_no_reset_where_the_words_name_none_or_a_zone_nobody_knows() {
    assert_eq!(
        resets(
            "You're out of usage credits. Run /usage-credits to keep using Fable 5.1.",
            at()
        ),
        None
    );
    assert_eq!(resets("resets 7pm (Mars/Olympus_Mons)", at()), None);
    assert_eq!(
        exhausted_quota("limit", f64::NAN, &local()),
        Ok(Quota::Exhausted {
            at: None,
            resets_at: None
        })
    );
}

#[test]
fn tells_anthropic_s_spent_extra_usage_a_400_for_a_refusal_for_quota() {
    // Pi on an Anthropic subscription, its extra usage spent (this machine's Pi
    // sessions, 2026-10): the task failed where it was to wait.
    assert!(refused_for_quota(
        r#"400 {"type":"error","error":{"type":"invalid_request_error","message":"You're out of extra usage. Add more at claude.ai/settings/usage and keep going."}}"#
    ));
    assert!(!refused_for_quota(
        r#"400 {"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long"}}"#
    ));
}

#[test]
fn tells_a_provider_s_quota_refusal_a_429_or_spent_credit_from_its_other_errors() {
    for text in [
        r#"429: {"message":"Provider returned error","code":429}"#,
        r#"OpenAI API error (429): {"type":"GoUsageLimitError","message":"Weekly usage limit reached. Resets in 2 days."}"#,
        r#"OpenAI API error (429): {"code":"rate_limit_exceeded","type":"rate_limit_error"}"#,
    ] {
        assert!(refused_for_quota(text), "{text}");
    }
    for text in [
        "500: provider down",
        r#"400: {"type":"server_error","message":"Upstream request failed"}"#,
        "OAuth refresh failed for openai-codex: OpenAI Codex token refresh failed (401)",
        "Provider returned error",
        "Stream ended without finish_reason",
        // An account's billing is no usage limit: it has no reset (`account_refusal`).
        r#"402: {"message":"This request requires more credits, or fewer max_tokens."}"#,
    ] {
        assert!(!refused_for_quota(text), "{text}");
    }
    assert_eq!(
        [
            quota_status(js::to_number(Some(&json!(429))).unwrap()),
            quota_status(js::to_number(Some(&json!("402"))).unwrap()),
            quota_status(js::to_number(Some(&json!(503))).unwrap()),
        ],
        [true, false, false]
    );
}

#[test]
fn an_expired_access_token_is_no_refused_login_but_a_refused_refresh_a_bad_key_and_a_bare_401_are()
{
    // The harness renews the token by itself: the turn failed and can be tried again.
    for text in [
        r#"401 {"type":"error","error":{"type":"authentication_error","message":"OAuth token has expired. Please obtain a new token or refresh your existing token."},"request_id":"req_1"}"#,
        r#"401 {"error":{"message":"Your authentication token has expired. Please try signing in again.","type":"invalid_request_error","code":"token_expired"}}"#,
    ] {
        assert_eq!(account_refusal(text, None), None, "{text}");
        assert_eq!(account_refusal(text, Some(401.0)), None, "{text}");
    }
    // A refresh that was refused, a key or token that is invalid or revoked, a 401 that names
    // neither: the login is refused.
    for text in [
        "OAuth refresh failed for anthropic: Anthropic token refresh request failed. details=Error: HTTP request failed. status=400; body={\"error\":\"invalid_grant\"}; the access token has expired",
        r#"401 {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
        r#"401 {"type":"error","error":{"type":"authentication_error","message":"OAuth token has been revoked."}}"#,
        "401 Unauthorized",
        // The review's cases (T-2350): an old expired token beside a revoked or invalid key, a
        // refresh token that expired, words that tell the person to log in, a refresh token
        // named beside an expired access token.
        "401 Invalid API key: key revoked. Previous OAuth token has expired.",
        r#"401 {"error":{"code":"refresh_token_expired","message":"The access token has expired."}}"#,
        "401 OAuth token has expired. Use your refresh token to renew.",
        "401 OAuth token has expired. Run /login to sign in again.",
        r#"401 {"error":{"message":"Your authentication token has expired. Please try signing in again.","code":"invalid_session"}}"#,
        // Pi's published Meta error: the identity session cannot be renewed (pi-ai 1.6.1,
        // auth/oauth/meta.js), in Pi's outer wrapper and bare.
        "OAuth refresh failed for meta: Meta session expired (status 401). Run `/login meta` to sign in again.",
        "Meta session expired (status 403). Run `/login meta` to sign in again.",
    ] {
        assert_eq!(
            account_refusal(text, None),
            Some(Refusal::Credentials),
            "{text}"
        );
    }
}

#[test]
fn tells_a_refusal_no_wait_cures_from_a_limit_that_resets() {
    // The words real refusals carried: OpenRouter's spent credit through Pi (2026-09),
    // Muse's billing refusal (T-705, 2026-10-26), Anthropic's API credit, an OpenAI
    // account over its plan.
    for text in [
        r#"402: {"message":"This request requires more credits, or fewer max_tokens."}"#,
        "API error 402 [request_id=x]: Billing verification failed. Billing Issue. (billing_error)",
        r#"400 {"type":"error","error":{"type":"invalid_request_error","message":"Your credit balance is too low to access the Anthropic API."}}"#,
        r#"OpenAI API error (429): {"code":"insufficient_quota","type":"insufficient_quota"}"#,
    ] {
        assert_eq!(
            account_refusal(text, None),
            Some(Refusal::Billing),
            "{text}"
        );
    }
    assert_eq!(
        account_refusal("anything", Some(402.0)),
        Some(Refusal::Billing)
    );
    for text in [
        r#"401 {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
        "OAuth refresh failed for openai-codex: OpenAI Codex token refresh failed (401)",
    ] {
        assert_eq!(
            account_refusal(text, None),
            Some(Refusal::Credentials),
            "{text}"
        );
    }
    assert_eq!(
        account_refusal("anything", Some(401.0)),
        Some(Refusal::Credentials)
    );
    // A limit that resets, backoff, and a failure of the request itself are not.
    for text in [
        r#"429: {"message":"Provider returned error","code":429}"#,
        r#"OpenAI API error (429): {"code":"rate_limit_exceeded","type":"rate_limit_error"}"#,
        "500: provider down",
        r#"400 {"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long"}}"#,
        "Usage limit reached. Check /usage.",
    ] {
        assert_eq!(account_refusal(text, None), None, "{text}");
    }
    assert_eq!(account_refusal("rate limited", Some(429.0)), None);
}

#[test]
fn an_unavailable_account_has_no_reset_and_says_why() {
    let unavailable = Quota::Unavailable {
        at: Some("2026-10-27T05:00:00.000Z".to_owned()),
        because: "no allowance".to_owned(),
    };
    assert_eq!(unavailable.level(), Level::Exhausted);
    assert_eq!(unavailable.resets_at(), None);
    assert_eq!(unavailable.at(), Some("2026-10-27T05:00:00.000Z"));
    assert_eq!(
        serde_json::to_string(&unavailable).unwrap(),
        r#"{"state":"unavailable","at":"2026-10-27T05:00:00.000Z","resetsAt":null,"because":"no allowance"}"#
    );
}

#[test]
fn a_moved_model_is_held_as_an_unavailable_account_is_and_says_it_is_neither_a_quota_nor_an_account(
) {
    // MUS-24: held for the human's word, with no reset, and told apart from the account in what
    // it says (`moved`) and in what a release asks (`moved()`, never `unavailable()`).
    let moved = Quota::Moved {
        at: Some("2026-10-27T05:00:00.000Z".to_owned()),
        because: "its model moved".to_owned(),
    };
    assert_eq!(moved.level(), Level::Exhausted);
    assert_eq!(moved.resets_at(), None);
    assert_eq!(moved.at(), Some("2026-10-27T05:00:00.000Z"));
    assert!(moved.moved() && !moved.unavailable() && moved.held_for_the_human());
    assert_eq!(
        serde_json::to_string(&moved).unwrap(),
        r#"{"state":"moved","at":"2026-10-27T05:00:00.000Z","resetsAt":null,"because":"its model moved"}"#
    );
    let account = Quota::Unavailable {
        at: None,
        because: "billing".to_owned(),
    };
    assert!(!account.moved() && account.unavailable() && account.held_for_the_human());
}
