//! The zones a refusal names, where Node's `Intl` and the bundled database
//! differ: ICU's own names, offsets, the names `Intl` refuses, and a year
//! before 1. Each answer is Node 26's (ICU 78, `TZ` set to
//! `America/Los_Angeles`, the instant `AT` unless said).

use jiff::Timestamp;

use super::*;

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
        "Factory",
        "factory",
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
fn every_name_intl_takes_names_the_zone_icu_gives_it() {
    fn at<'a>(text: &str, read: &'a str) -> (String, Option<&'a str>) {
        (text.to_owned(), Some(read))
    }
    assert_resets(&[
        at("429: resets 3pm (PST)", "2026-10-03T22:00:00.000Z"),
        at("429: resets 3pm (pst)", "2026-10-03T22:00:00.000Z"),
        at("resets 3pm (IST)", "2026-10-04T09:30:00.000Z"),
        at("resets 3pm (CST)", "2026-10-03T20:00:00.000Z"),
        at("resets 3pm (AGT)", "2026-10-03T18:00:00.000Z"),
        at("resets 3pm (US/Pacific-New)", "2026-10-03T22:00:00.000Z"),
        at(
            "resets 3pm (Canada/East-Saskatchewan)",
            "2026-10-03T21:00:00.000Z",
        ),
        at("resets 3pm (SystemV/PST8PDT)", "2026-10-03T22:00:00.000Z"),
        at("resets 3pm (SystemV/AST4)", "2026-10-03T19:00:00.000Z"),
        at("resets Oct 5 at 9am (EST)", "2026-10-05T14:00:00.000Z"),
        // SystemV's daylight time starts on April's last Sunday, not its first.
        at(
            "resets Apr 20 at 9am (SystemV/EST5EDT)",
            "2027-04-20T14:00:00.000Z",
        ),
        ("resets 3pm (Factory)".to_owned(), None),
        ("resets 3pm (UT)".to_owned(), None),
        ("resets 3pm (GMT+03:00)".to_owned(), None),
    ]);
    let april = instant("2026-04-10T12:00:00.000Z");
    assert_eq!(
        resets("resets 3pm (SystemV/PST8PDT)", april).as_deref(),
        Some("2026-04-10T23:00:00.000Z")
    );
    assert_eq!(
        resets("resets 3pm (America/Los_Angeles)", april).as_deref(),
        Some("2026-04-10T22:00:00.000Z")
    );
    assert_eq!(
        resets(
            "resets 3pm (systemv/yst9ydt)",
            instant("2026-07-01T00:00:00.000Z")
        )
        .as_deref(),
        Some("2026-07-01T23:00:00.000Z")
    );
}

#[test]
fn an_offset_is_a_zone_as_intl_takes_one() {
    assert_resets(&[
        (
            "resets 3pm (+03:00)".into(),
            Some("2026-10-04T12:00:00.000Z"),
        ),
        (
            "resets 3pm (+0530)".into(),
            Some("2026-10-04T09:30:00.000Z"),
        ),
        (
            "resets 3pm (\u{2212}03)".into(),
            Some("2026-10-03T18:00:00.000Z"),
        ),
        (
            "resets 3pm (-00:00)".into(),
            Some("2026-10-03T15:00:00.000Z"),
        ),
        (
            "resets 3pm (+23:59)".into(),
            Some("2026-10-03T15:01:00.000Z"),
        ),
        ("resets 3pm (+24:00)".into(), None),
    ]);
    let seconds =
        |name: &str| time_zone(name).map(|zone| zone.to_offset(Timestamp::UNIX_EPOCH).seconds());
    for (name, offset) in [
        ("+00", 0),
        ("+0000", 0),
        ("+1200", 43_200),
        ("+0330", 12_600),
        ("-0330", -12_600),
        ("\u{2212}0330", -12_600),
        ("+23", 82_800),
        ("-23:59", -86_340),
        ("+00:59", 3_540),
        ("+19:00", 68_400),
    ] {
        assert_eq!(seconds(name), Some(offset), "{name}");
    }
    for name in [
        "+0",
        "+000",
        "+00000",
        "+12:5",
        "+12:00 ",
        "+03:0",
        "+030",
        "+03:00Z",
        "+03.5",
        "\u{2212}",
        "+",
        "++03",
        "+3:30",
        "+24",
        "+00:60",
        "\u{FF0B}03:00",
        "+03\u{200B}:00",
        "+3",
        "+3:00",
        "+03:00:00",
        "+0360",
        "+\u{20AC}0",
    ] {
        assert_eq!(seconds(name), None, "{name:?}");
    }
}

#[test]
fn a_year_before_1_is_the_year_of_its_era_as_intl_writes_it() {
    // `Intl` writes the year 0 as 1 (BC), which `Date.UTC` reads as 1901.
    for (at_ms, read) in [
        (-62_167_219_200_000.0, "1901-01-01T15:00:00.000Z"),
        (-62_198_755_200_000.0, "1902-01-01T15:00:00.000Z"),
        (-62_135_596_800_001.0, "1901-12-31T15:00:00.000Z"),
    ] {
        assert_eq!(
            resets("resets 3pm (UTC)", at_ms).as_deref(),
            Some(read),
            "{at_ms}"
        );
    }
}
