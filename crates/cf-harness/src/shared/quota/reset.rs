//! The reset a refusal names (`namedReset`, `resetIn`, `resetAt` and
//! `zonedTime`, `hosts/lib/quota.js`): in a span of time, or at a time of day
//! in a zone.

use cf_base::js;
use cf_base::time::{time_clip, utc_ms};
use jiff::tz::TimeZone;
use jiff::Timestamp;

use super::date;
use super::patterns::{AT, SPAN, UNIT};
use super::zone::time_zone;

/// A day, in milliseconds: what a span of days adds, and how long a date may
/// be gone by before it is next year's.
const DAY_MS: f64 = 86_400_000.0;

/// A month's name, by its first three letters.
const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// When a refusal says the quota comes back, from `at_ms`: in a span, or at
/// a time; none when it does not say. A time with no zone is read in
/// `local`. Fails where a span reaches past what a date holds, as
/// `toISOString` throws there.
pub(super) fn named_reset(
    text: &str,
    at_ms: f64,
    local: &TimeZone,
) -> Result<Option<String>, String> {
    if !at_ms.is_finite() {
        return Ok(None);
    }
    Ok(match reset_in(text, at_ms)? {
        Some(span) => Some(span),
        None => reset_at(text, at_ms, local),
    })
}

/// "Resets in 3 days", "Resets in 3hr 4min": the time that names, from
/// `at_ms`; none otherwise, and for a unit that is none.
fn reset_in(text: &str, at_ms: f64) -> Result<Option<String>, String> {
    let Some(span) = SPAN.captures(text) else {
        return Ok(None);
    };
    let mut ms = 0.0;
    for unit in UNIT.captures_iter(&span[1]) {
        let Some(each) = unit_ms(&unit[2]) else {
            return Ok(None);
        };
        ms += js::number(&unit[1]) * each;
    }
    date(at_ms + ms).map(Some)
}

/// A unit's length by its first letter: "2 days", "3hr 4min", "35 minutes",
/// "4h 30m". So "2 months" is two minutes. None for a letter that is no unit.
fn unit_ms(unit: &str) -> Option<f64> {
    match unit.chars().next()?.to_ascii_lowercase() {
        'w' => Some(604_800_000.0),
        'd' => Some(DAY_MS),
        'h' => Some(3_600_000.0),
        'm' => Some(60_000.0),
        's' => Some(1_000.0),
        _ => None,
    }
}

/// Claude's own words, "resets 7:30pm (Europe/Bucharest)" or "resets Sep 29
/// at 11am (Europe/Bucharest)": the next time that wall clock reads so, after
/// `at_ms`, in the zone it names (`local` when it names none); none when the
/// words are not of that shape or the zone is unknown. "Resets at 3pm" is not
/// of that shape.
fn reset_at(text: &str, at_ms: f64, local: &TimeZone) -> Option<String> {
    let found = AT.captures(text)?;
    let hour = whole(&found[3]) % 12
        + if found[5].eq_ignore_ascii_case("pm") {
            12
        } else {
            0
        };
    let minute = found.get(4).map_or(0, |minute| whole(minute.as_str()));
    let zone = match found.get(6) {
        Some(name) => time_zone(name.as_str())?,
        None => local.clone(),
    };
    let today = wall_clock(&zone, time_clip(at_ms)?)?;
    let at = if let Some(name) = found.get(1) {
        let name = name.as_str().to_ascii_lowercase();
        let month = MONTHS.iter().position(|month| *month == name)?;
        let month = i64::try_from(month).ok()?;
        let day = whole(&found[2]);
        let mut at = zoned_time(&zone, today.year, month, day, hour, minute)?;
        // A date gone by more than a day is next year's.
        if as_ms(at) < at_ms - DAY_MS {
            at = zoned_time(&zone, today.year + 1, month, day, hour, minute)?;
        }
        at
    } else {
        let mut at = zoned_time(&zone, today.year, today.month - 1, today.day, hour, minute)?;
        if as_ms(at) <= at_ms {
            at = zoned_time(
                &zone,
                today.year,
                today.month - 1,
                today.day + 1,
                hour,
                minute,
            )?;
        }
        at
    };
    date(as_ms(at)).ok()
}

/// An instant as the double JavaScript compares it as. Exact: an instant a
/// date holds is within ±8.64e15, which is under 2^53.
#[allow(clippy::cast_precision_loss)]
fn as_ms(instant: i64) -> f64 {
    instant as f64
}

/// The whole number the digits of `text` write: the patterns allow nothing
/// else there, so it is never the default.
fn whole(text: &str) -> i64 {
    text.parse().unwrap_or_default()
}

/// The instant a wall clock in `zone` reads that time; `Date.UTC`'s overflow
/// carries a day past the month's end. Twice, once for the zone's offset and
/// once more if that guess crossed a change of it, so that of the two times a
/// fold gives, and the one a gap lacks, it names what `Intl` named.
fn zoned_time(
    zone: &TimeZone,
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
) -> Option<i64> {
    let wanted = utc_ms(year, month, day, hour, minute)?;
    let mut instant = wanted;
    for _ in 0..2 {
        let shown = wall_clock(zone, instant)?;
        instant += wanted
            - utc_ms(
                shown.year,
                shown.month - 1,
                shown.day,
                shown.hour,
                shown.minute,
            )?;
    }
    Some(instant)
}

/// What a wall clock reads: its date and its time to the minute.
struct WallClock {
    year: i64,
    /// From 1.
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
}

/// What `instant` (milliseconds since the epoch) reads on the wall of
/// `zone`, as `Intl` writes its parts: a year before 1 as the year of its
/// era, before Christ (1 for the year 0, 2 for -1). None for an instant past
/// the years jiff holds, 9999 either way, where `Intl` still formats one.
fn wall_clock(zone: &TimeZone, instant: i64) -> Option<WallClock> {
    let shown = zone.to_datetime(Timestamp::from_millisecond(instant).ok()?);
    let year = i64::from(shown.year());
    Some(WallClock {
        year: if year < 1 { 1 - year } else { year },
        month: i64::from(shown.month()),
        day: i64::from(shown.day()),
        hour: i64::from(shown.hour()),
        minute: i64::from(shown.minute()),
    })
}
