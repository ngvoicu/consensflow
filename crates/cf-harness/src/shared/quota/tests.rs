//! The shared quota against what Node answered: the tables of
//! `tests/goldens/records/tables.json`, the sentences of `tests/quota.test.mjs`,
//! and the readings no table has a row for.

use super::reset::time_zone;
use super::*;
use cf_base::time::parse;

mod a_quota_refusal;
mod tables;
mod words;

/// The zone a reset that names none is read in, as the tables were made:
/// Node ran with `TZ` set to it.
fn local() -> TimeZone {
    time_zone("America/Los_Angeles").unwrap()
}

/// `Date.parse(text)`, the instant a refusal is read at.
#[allow(clippy::cast_precision_loss)] // A date is within 8.64e15, which a double holds exactly.
fn instant(text: &str) -> f64 {
    parse(text).unwrap() as f64
}

/// What `exhausted_quota(text, at_ms)` says the reset is, in `local`'s zone.
fn resets(text: &str, at_ms: f64) -> Option<String> {
    match exhausted_quota(text, at_ms, &local()).unwrap() {
        Quota::Exhausted { resets_at, .. } => resets_at,
        Quota::Usage { .. } => unreachable!("a refusal is exhausted"),
    }
}

#[test]
fn a_quota_is_written_as_node_wrote_it() {
    let refused = Quota::Exhausted {
        at: Some("2026-09-19T10:00:00.000Z".to_owned()),
        resets_at: None,
    };
    assert_eq!(
        serde_json::to_string(&refused).unwrap(),
        r#"{"state":"exhausted","at":"2026-09-19T10:00:00.000Z","resetsAt":null}"#
    );
    let usage = Quota::Usage {
        level: Level::Low,
        used_percent: Some(serde_json::Number::from(97)),
        resets_at: Some("2026-09-26T11:49:53.000Z".to_owned()),
    };
    assert_eq!(
        serde_json::to_string(&usage).unwrap(),
        r#"{"state":"low","usedPercent":97,"resetsAt":"2026-09-26T11:49:53.000Z"}"#
    );
}
