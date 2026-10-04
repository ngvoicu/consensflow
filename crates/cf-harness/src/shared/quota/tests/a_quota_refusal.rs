//! What a harness's refusal says about its quota, on the words harnesses
//! really wrote: Claude Code's limits, Pi's provider errors and OpenRouter's
//! spent credit, as this machine's records held them (2026-09/10). The
//! sentences of `tests/quota.test.mjs`, `describe('a quota refusal')`.

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
fn tells_a_provider_s_quota_refusal_a_429_or_spent_credit_from_its_other_errors() {
    for text in [
        r#"429: {"message":"Provider returned error","code":429}"#,
        r#"OpenAI API error (429): {"type":"GoUsageLimitError","message":"Weekly usage limit reached. Resets in 2 days."}"#,
        r#"OpenAI API error (429): {"code":"rate_limit_exceeded","type":"rate_limit_error"}"#,
        r#"402: {"message":"This request requires more credits, or fewer max_tokens."}"#,
    ] {
        assert!(refused_for_quota(text), "{text}");
    }
    for text in [
        "500: provider down",
        r#"400: {"type":"server_error","message":"Upstream request failed"}"#,
        "OAuth refresh failed for openai-codex: OpenAI Codex token refresh failed (401)",
        "Provider returned error",
        "Stream ended without finish_reason",
    ] {
        assert!(!refused_for_quota(text), "{text}");
    }
    assert_eq!(
        [
            quota_status(js::to_number(Some(&json!(429))).unwrap()),
            quota_status(js::to_number(Some(&json!("402"))).unwrap()),
            quota_status(js::to_number(Some(&json!(503))).unwrap()),
        ],
        [true, true, false]
    );
}
