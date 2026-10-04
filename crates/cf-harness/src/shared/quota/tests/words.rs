//! The words of a refusal that no table row reads: where JavaScript's `\d`,
//! `\s`, `\b` and `/i` differ from the regex crate's, the carry of a day, an
//! hour or a minute, the edges of what a date holds, and the zones Node and
//! jiff take differently. Each answer is Node 26's (`TZ` set to
//! `America/Los_Angeles`, the instant `2026-10-03T12:00:00.000Z` unless said).

use serde_json::json;

use super::*;

const AT: &str = "2026-10-03T12:00:00.000Z";

/// Each text's reset, read at `AT`.
fn assert_resets(cases: &[(String, Option<&str>)]) {
    for (text, expected) in cases {
        assert_eq!(resets(text, instant(AT)).as_deref(), *expected, "{text:?}");
    }
}

/// A span of two days, `character` for the space after `resets`.
fn between(character: char) -> String {
    format!("resets{character}in 2 days")
}

#[test]
fn a_span_is_summed_by_its_units_and_wins_over_a_time() {
    assert_resets(&[
        (
            "resets in 1 week AND 2 days".into(),
            Some("2026-10-12T12:00:00.000Z"),
        ),
        ("resets in 3hr4min".into(), Some("2026-10-03T15:04:00.000Z")),
        (
            "resets in 1 week,2 days".into(),
            Some("2026-10-12T12:00:00.000Z"),
        ),
        (
            "resets in 1 weekand 2 days".into(),
            Some("2026-10-12T12:00:00.000Z"),
        ),
        ("resets in 5 and".into(), None),
        ("resets in 1.5 hours".into(), None),
        (
            "resets in 2 hours, or resets 7pm (UTC)".into(),
            Some("2026-10-03T14:00:00.000Z"),
        ),
        (
            "resets in 5 parsecs, resets 7pm (UTC)".into(),
            Some("2026-10-03T19:00:00.000Z"),
        ),
    ]);
}

#[test]
fn a_span_that_reaches_past_what_a_date_holds_fails_where_node_threw() {
    for text in [
        format!("resets in {} days", "9".repeat(30)),
        format!("resets in {} days", "9".repeat(400)),
        "resets in 999999999999 weeks".to_owned(),
    ] {
        assert!(
            exhausted_quota(&text, instant(AT), &local()).is_err(),
            "{text}"
        );
    }
    // A span to the last instant, and one millisecond past it.
    let span = |at_ms: f64| exhausted_quota("resets in 1 second", at_ms, &local());
    assert!(span(8.64e15 - 1000.0).is_ok());
    assert!(span(8.64e15 - 999.0).is_err());
    // A time that is past what a date holds is no instant at all.
    for at_ms in [8.64e15 + 2.0, -8.64e15 - 2.0] {
        assert!(
            exhausted_quota("limit", at_ms, &local()).is_err(),
            "{at_ms}"
        );
    }
}

#[test]
fn a_time_of_day_and_a_date_carry_and_cut_as_node_cut_them() {
    let on = |text: &str, when: &str, expected: Option<&str>| {
        assert_eq!(
            resets(text, instant(when)).as_deref(),
            expected,
            "{text:?} at {when}"
        );
    };
    on("resets 0am (UTC)", AT, Some("2026-10-04T00:00:00.000Z"));
    on("resets 12am (UTC)", AT, Some("2026-10-04T00:00:00.000Z"));
    on("resets 13pm (UTC)", AT, Some("2026-10-03T13:00:00.000Z"));
    on("resets 24am (UTC)", AT, Some("2026-10-04T00:00:00.000Z"));
    on("resets 99pm (UTC)", AT, Some("2026-10-03T15:00:00.000Z"));
    on("resets 1:99pm (UTC)", AT, Some("2026-10-03T14:39:00.000Z"));
    on("resets 1:5pm (UTC)", AT, None);
    on(
        "resets Feb 31 at 9am (UTC)",
        AT,
        Some("2027-03-03T09:00:00.000Z"),
    );
    on(
        "resets Mar 0 at 9am (UTC)",
        AT,
        Some("2027-02-28T09:00:00.000Z"),
    );
    on(
        "resets Mar 99 at 9am (UTC)",
        AT,
        Some("2027-06-07T09:00:00.000Z"),
    );
    // A time of day that is now is tomorrow's; one millisecond sooner it is today's.
    on("resets 12pm (UTC)", AT, Some("2026-10-04T12:00:00.000Z"));
    on(
        "resets 12pm (UTC)",
        "2026-10-03T11:59:59.999Z",
        Some("2026-10-03T12:00:00.000Z"),
    );
    // A date gone by a day and more is next year's; a day exactly is not.
    on(
        "resets Oct 2 at 11am (UTC)",
        AT,
        Some("2027-10-02T11:00:00.000Z"),
    );
    on(
        "resets Oct 2 at 12pm (UTC)",
        AT,
        Some("2026-10-02T12:00:00.000Z"),
    );
    on(
        "resets Oct 2 at 12pm (UTC)",
        "2026-10-03T11:59:59.999Z",
        Some("2026-10-02T12:00:00.000Z"),
    );
    on(
        "resets Oct 2 at 12pm (UTC)",
        "2026-10-03T12:00:00.001Z",
        Some("2027-10-02T12:00:00.000Z"),
    );
}

#[test]
fn a_fraction_of_a_millisecond_is_cut_toward_zero() {
    let quota = |at_ms: f64| {
        serde_json::to_value(exhausted_quota("resets in 2 days", at_ms, &local()).unwrap()).unwrap()
    };
    assert_eq!(
        quota(instant(AT) + 0.75),
        json!({ "state": "exhausted", "at": "2026-10-03T12:00:00.000Z", "resetsAt": "2026-10-05T12:00:00.000Z" })
    );
    assert_eq!(
        quota(-1.5),
        json!({ "state": "exhausted", "at": "1969-12-31T23:59:59.999Z", "resetsAt": "1970-01-02T23:59:59.998Z" })
    );
}

#[test]
fn white_space_is_javascripts_and_letters_digits_and_boundaries_are_ascii() {
    // Each is a span in Node but the two it does not take for white space.
    let span = Some("2026-10-05T12:00:00.000Z");
    assert_resets(&[
        (between('\u{FEFF}'), span),
        (between('\u{A0}'), span),
        (between('\u{3000}'), span),
        (between('\u{2028}'), span),
        (between('\u{85}'), None),
        (between('\u{200B}'), None),
        // ſ and the Kelvin sign fold to ASCII letters in Unicode, not in JavaScript.
        ("re\u{17F}ets in 2 days".into(), None),
        ("resets i\u{17F} 2 days".into(), None),
        ("resets in 2 \u{212A}ilo".into(), None),
        ("resets Sep 29 a\u{17F} 11am (UTC)".into(), None),
        // An Arabic-Indic digit is no digit to `\d`.
        ("resets in \u{663} days".into(), None),
        ("resets \u{663}pm (UTC)".into(), None),
        // `\b` falls between ASCII word characters alone: é is none.
        ("resets 3pm\u{E9}".into(), Some("2026-10-03T22:00:00.000Z")),
        ("resets 3pmx (UTC)".into(), None),
        ("resets 3pm_ (UTC)".into(), None),
        ("resets 3pm1 (UTC)".into(), None),
        ("RESETS 3PM (utc)".into(), Some("2026-10-03T15:00:00.000Z")),
    ]);
    // A month is its first three letters, in any case, and then any letters.
    for text in [
        "resets sEp 29 at 11AM (utc)",
        "resets September 29 at 11am (UTC)",
        "resets Sepx 29 at 11am (UTC)",
    ] {
        assert_eq!(
            resets(text, instant("2026-09-26T12:00:00.000Z")).as_deref(),
            Some("2026-09-29T11:00:00.000Z"),
            "{text}"
        );
    }
    assert!(refused_for_quota("429"));
    for text in [
        "\u{664}\u{662}\u{669}: x",
        "429a",
        "429_",
        "ab\n(429)",
        "((429)",
    ] {
        assert!(!refused_for_quota(text), "{text:?}");
    }
    for text in ["429\u{E9}", "(429", "a\r(429)", "429\n"] {
        assert!(refused_for_quota(text), "{text:?}");
    }
}

#[test]
fn a_status_is_what_number_reads_of_it() {
    let reads = |status: Option<&serde_json::Value>| quota_status(status);
    assert!(!reads(None));
    for no in [
        json!(null),
        json!("abc"),
        json!(402.5),
        json!({}),
        json!(true),
    ] {
        assert!(!reads(Some(&no)), "{no}");
    }
    for yes in [json!(" 402 "), json!(["402"]), json!(429.0), json!("0x1AD")] {
        assert!(reads(Some(&yes)), "{yes}");
    }
}

#[test]
fn the_zone_is_named_in_any_ascii_case_and_never_trimmed() {
    for name in [
        "europe/BUCHAREST",
        "UTC",
        "utc",
        "Asia/Calcutta",
        "US/Pacific",
    ] {
        assert!(time_zone(name).is_some(), "{name}");
    }
    for name in [
        " Europe/Bucharest",
        "Europe/Bucharest ",
        "Mars/Olympus",
        "",
        "Etc/Unknown",
        "etc/unknown",
    ] {
        assert!(time_zone(name).is_none(), "{name:?}");
    }
    assert_resets(&[
        ("resets 3pm (utc)".into(), Some("2026-10-03T15:00:00.000Z")),
        ("resets 3pm (Etc/Unknown)".into(), None),
        ("resets 3pm (UTC\n)".into(), None),
        // The first of two parentheses is the zone.
        (
            "resets 3pm (UTC) (Asia/Tokyo)".into(),
            Some("2026-10-03T15:00:00.000Z"),
        ),
        // A zone that is empty, or never closed, is none named: the machine's is read.
        ("resets 3pm ()".into(), Some("2026-10-03T22:00:00.000Z")),
        ("resets 3pm (UTC".into(), Some("2026-10-03T22:00:00.000Z")),
    ]);
}

#[test]
fn what_jiff_cannot_hold_is_a_difference_kept_from_node() {
    // Node 26 takes an offset for a zone, and answers 2026-10-04T12:00:00.000Z.
    assert_resets(&[("resets 3pm (+03:00)".into(), None)]);
    // The last instant jiff holds is 9999-12-30T22:00:00Z. Node answers the
    // times of day it is asked of past it, "+010000-01-01T15:00:00.000Z" for
    // 253402300799000; a span needs no zone, and is answered.
    let last = 253_402_300_799_000.0;
    assert_eq!(resets("resets 3pm (UTC)", last), None);
    assert_eq!(
        resets("resets in 2 days", last).as_deref(),
        Some("+010000-01-02T23:59:59.000Z")
    );
}
