//! Time as JavaScript wrote and read it: an instant in milliseconds since
//! the epoch, written as `Date.prototype.toISOString` writes it
//! (`2026-10-04T08:00:00.000Z`), and read from the date-time format
//! `Date.parse` is specified to accept. What Node wrote reads back, and goes
//! out again, as it was.

/// The clock a component reads: milliseconds since the epoch, injected so a
/// test, or a replay of what Node recorded, says what time it is.
pub trait Clock {
    fn now_ms(&mut self) -> i64;
}

/// The system's clock.
pub struct SystemClock;

impl Clock for SystemClock {
    #[allow(clippy::disallowed_methods)] // The one place the time of day is read.
    fn now_ms(&mut self) -> i64 {
        let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        since.map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
    }
}

const MS_PER_DAY: i64 = 86_400_000;

/// The latest instant a date holds, in milliseconds either side of the epoch.
const LAST_MS: i64 = 8_640_000_000_000_000;

/// `ms` as `toISOString` writes it: always UTC, three fractional digits, `Z`;
/// a year past 9999 or before 0 with a sign and six digits.
pub fn iso(ms: i64) -> String {
    let days = ms.div_euclid(MS_PER_DAY);
    let of_day = ms.rem_euclid(MS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{}{:06}", if year < 0 { '-' } else { '+' }, year.abs())
    };
    format!(
        "{year}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        of_day / 3_600_000,
        of_day / 60_000 % 60,
        of_day / 1000 % 60,
        of_day % 1000
    )
}

/// `TimeClip(ms)`: the instant `new Date(ms)` holds, in whole milliseconds
/// toward zero. None where that date is invalid, and `toISOString` throws:
/// for a number that is no finite one, or one past what a date holds.
pub fn time_clip(ms: f64) -> Option<i64> {
    // The range is tested before the fraction is cut, as `TimeClip` tests it.
    if !ms.is_finite() || ms.abs() > 8.64e15 {
        return None;
    }
    // In range, so an i64 holds it exactly.
    #[allow(clippy::cast_possible_truncation)]
    Some(ms.trunc() as i64)
}

/// What `Date.parse` makes of `text` in the date-time format ECMAScript
/// defines (`YYYY`, `YYYY-MM`, `YYYY-MM-DD`, then optionally `THH:mm`,
/// `:ss`, `.sss` and `Z` or `±HH:mm`; `±YYYYYY` years): milliseconds since the
/// epoch, or none where it gives NaN. A date alone is UTC.
///
/// V8 also reads what the format does not hold, through a legacy parser: a
/// time with no offset as local time, a space for the `T`, a day past the
/// month's last as one in the next. ConsensFlow writes none of these (every
/// time it stores is `toISOString`'s), so they are read as none here.
pub fn parse(text: &str) -> Option<i64> {
    let mut rest = text;
    let year = take_year(&mut rest)?;
    let month = take_part(&mut rest, '-', 2).unwrap_or(1);
    let day = if text.len() - rest.len() > 4 {
        take_part(&mut rest, '-', 2).unwrap_or(1)
    } else {
        1
    };
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let mut of_day = 0;
    if let Some(time) = rest.strip_prefix('T') {
        rest = time;
        let hours = take_digits(&mut rest, 2)?;
        let minutes = take_part(&mut rest, ':', 2)?;
        let seconds = take_part(&mut rest, ':', 2).unwrap_or(0);
        let millis = match rest.strip_prefix('.') {
            Some(fraction) => {
                let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
                if digits == 0 {
                    return None;
                }
                rest = &fraction[digits..];
                // JavaScript keeps the first three digits; more are read and dropped.
                let first: String = fraction[..digits]
                    .chars()
                    .chain("00".chars())
                    .take(3)
                    .collect();
                first.parse::<i64>().ok()?
            }
            None => 0,
        };
        let ends_day = hours == 24 && minutes == 0 && seconds == 0 && millis == 0;
        if (hours > 23 && !ends_day) || minutes > 59 || seconds > 59 {
            return None;
        }
        of_day = ((hours * 60 + minutes) * 60 + seconds) * 1000 + millis;
        let offset = match rest.as_bytes().first() {
            Some(b'Z') => {
                rest = &rest[1..];
                0
            }
            Some(sign @ (b'+' | b'-')) => {
                let sign = if *sign == b'-' { -1 } else { 1 };
                rest = &rest[1..];
                let hours = take_digits(&mut rest, 2)?;
                let minutes = take_part(&mut rest, ':', 2)?;
                if hours > 23 || minutes > 59 {
                    return None;
                }
                sign * (hours * 60 + minutes) * 60_000
            }
            _ => return None,
        };
        of_day -= offset;
    }
    if !rest.is_empty() {
        return None;
    }
    let ms = days_from_civil(year, month, day) * MS_PER_DAY + of_day;
    // ECMAScript's time values reach a hundred million days either side of the epoch.
    (ms.abs() <= LAST_MS).then_some(ms)
}

/// `Date.UTC(year, month, day, hour, minute)` for whole numbers, as V8
/// computes it (`MakeDay`, `MakeTime`, `MakeDate`, `TimeClip`): the instant
/// that wall clock reads in UTC, in milliseconds since the epoch.
///
/// - The month counts from 0 and carries into the year: month 12 is January
///   of the next year, month -1 December of the one before.
/// - A day past the month's end, an hour past 23 and a minute past 59 carry
///   on: February 31, 2026 is March 3, and 7:75pm is 8:15pm.
/// - A year from 0 to 99 is that year of the 1900s.
/// - The month's first day is counted in whole numbers, V8's way: before the
///   year -399,999 its division cuts toward zero, a day off the calendar.
///   The rest is in doubles, each product and sum rounded as JavaScript's.
///
/// None where JavaScript gives NaN: past what a date holds, or, as V8 reads
/// it, for a year more than a million either side of 0 or a month more than
/// ten million.
// Each whole number is a JavaScript number here: one past 2^53 is the double
// it rounds to, as it was in JavaScript.
#[allow(clippy::cast_precision_loss)]
pub fn utc_ms(year: i64, month: i64, day: i64, hour: i64, minute: i64) -> Option<i64> {
    let year = if (0..=99).contains(&year) {
        1900 + year
    } else {
        year
    };
    if !(-1_000_000..=1_000_000).contains(&year) || !(-10_000_000..=10_000_000).contains(&month) {
        return None;
    }
    let first = month_start(year + month.div_euclid(12), month.rem_euclid(12))?;
    let days = (first - 1) as f64 + day as f64;
    let time = hour as f64 * 3_600_000.0 + minute as f64 * 60_000.0;
    time_clip(days * 86_400_000.0 + time)
}

/// The day of the first of `month` (from 0) in `year`, counted from the
/// epoch's as V8 counts it: its year counted from a year 399,999 before 0,
/// in whole numbers whose division cuts toward zero.
fn month_start(year: i64, month: i64) -> Option<i64> {
    const BEFORE: i64 = 399_999;
    const COMMON: [i64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    const LEAP: [i64; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];
    let days = |year: i64| {
        let years = year + BEFORE;
        365 * years + years / 4 - years / 100 + years / 400
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let before = if leap { LEAP } else { COMMON };
    Some(days(year) - days(1970) + before.get(usize::try_from(month).ok()?)?)
}

fn take_year(rest: &mut &str) -> Option<i64> {
    match rest.as_bytes().first() {
        Some(sign @ (b'+' | b'-')) => {
            let negative = *sign == b'-';
            *rest = &rest[1..];
            let year = take_digits(rest, 6)?;
            // -000000 is no year.
            if negative && year == 0 {
                return None;
            }
            Some(if negative { -year } else { year })
        }
        _ => take_digits(rest, 4),
    }
}

/// `separator` then exactly `width` digits; none, and nothing taken, when the separator is not next.
fn take_part(rest: &mut &str, separator: char, width: usize) -> Option<i64> {
    let after = rest.strip_prefix(separator)?;
    let mut probe = after;
    let value = take_digits(&mut probe, width)?;
    *rest = probe;
    Some(value)
}

fn take_digits(rest: &mut &str, width: usize) -> Option<i64> {
    let digits = rest.get(..width)?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    *rest = &rest[width..];
    digits.parse().ok()
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    era * 146_097 + of_cycle - 719_468
}

/// The date `days` after 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days - era * 146_097;
    let of_cycle = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * of_cycle + of_cycle / 4 - of_cycle / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = of_cycle + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_an_instant_as_to_iso_string_does() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_790_928_000_000), "2026-10-02T08:00:00.000Z");
        assert_eq!(iso(1_759_573_311_070), "2025-10-04T10:21:51.070Z");
        assert_eq!(iso(-1), "1969-12-31T23:59:59.999Z");
        assert_eq!(iso(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(iso(253_402_300_800_000), "+010000-01-01T00:00:00.000Z");
        assert_eq!(iso(-62_198_755_200_000), "-000001-01-01T00:00:00.000Z");
    }

    #[test]
    fn a_number_of_milliseconds_is_the_instant_a_date_holds() {
        // Node 26's new Date(ms).toISOString() for each.
        let cases = [
            (0.0, "1970-01-01T00:00:00.000Z"),
            (1.9, "1970-01-01T00:00:00.001Z"),
            (-1.9, "1969-12-31T23:59:59.999Z"),
            (-0.5, "1970-01-01T00:00:00.000Z"),
            (1_790_000_000_000.75, "2026-09-21T14:13:20.000Z"),
            (8.64e15, "+275760-09-13T00:00:00.000Z"),
            (-8.64e15, "-271821-04-20T00:00:00.000Z"),
        ];
        for (ms, written) in cases {
            assert_eq!(time_clip(ms).map(iso).as_deref(), Some(written), "{ms}");
        }
        // Each of these throws a RangeError ("Invalid time value").
        for ms in [
            8.64e15 + 2.0,
            -8.64e15 - 2.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            1e300,
        ] {
            assert_eq!(time_clip(ms), None, "{ms}");
        }
    }

    #[test]
    fn reads_a_wall_clock_as_date_utc_reads_it() {
        // Node 26's Date.UTC(year, month, day, hour, minute) for each, written
        // by toISOString.
        let cases = [
            ((2026, 8, 19, 10, 0), "2026-09-19T10:00:00.000Z"),
            ((2026, 1, 31, 9, 0), "2026-03-03T09:00:00.000Z"),
            ((2026, 8, 0, 9, 0), "2026-08-31T09:00:00.000Z"),
            ((2026, 12, 1, 0, 0), "2027-01-01T00:00:00.000Z"),
            ((2026, -1, 1, 0, 0), "2025-12-01T00:00:00.000Z"),
            ((2026, 0, 1, 19, 75), "2026-01-01T20:15:00.000Z"),
            ((2026, 11, 31, 23, 60), "2027-01-01T00:00:00.000Z"),
            ((2026, 11, 32, 0, 0), "2027-01-01T00:00:00.000Z"),
            ((2028, 1, 29, 12, 0), "2028-02-29T12:00:00.000Z"),
            ((2027, 1, 29, 12, 0), "2027-03-01T12:00:00.000Z"),
            ((2026, 0, 1, 24, 0), "2026-01-02T00:00:00.000Z"),
            ((2026, 0, 1, -1, 0), "2025-12-31T23:00:00.000Z"),
            ((2026, 0, 1, 0, -1), "2025-12-31T23:59:00.000Z"),
            ((2026, 0, 1, 25, 61), "2026-01-02T02:01:00.000Z"),
            ((2026, 0, 1, 0, 100_000_000), "2216-02-19T10:40:00.000Z"),
            ((2026, 1_200_000, 1, 0, 0), "+102026-01-01T00:00:00.000Z"),
            ((100, 0, 1, 0, 0), "0100-01-01T00:00:00.000Z"),
            ((-1, 0, 1, 0, 0), "-000001-01-01T00:00:00.000Z"),
            ((275_760, 8, 13, 0, 0), "+275760-09-13T00:00:00.000Z"),
            ((-271_821, 3, 20, 0, 0), "-271821-04-20T00:00:00.000Z"),
        ];
        for ((year, month, day, hour, minute), written) in cases {
            assert_eq!(
                utc_ms(year, month, day, hour, minute).map(iso).as_deref(),
                Some(written),
                "{year}, {month}, {day}, {hour}, {minute}"
            );
        }
        assert_eq!(utc_ms(2026, 8, 19, 10, 0), Some(1_789_812_000_000));
        assert_eq!(utc_ms(1969, 11, 31, 23, 59), Some(-60_000));
        assert_eq!(utc_ms(1970, 0, 1, 0, 0), Some(0));
    }

    #[test]
    fn a_month_carries_into_the_years_around_it_and_a_day_into_the_months() {
        // Node 26's Date.UTC(2026, month, 15, 6, 30) for each month, by toISOString.
        let months = [
            (-25, "2023-12-15T06:30:00.000Z"),
            (-24, "2024-01-15T06:30:00.000Z"),
            (-13, "2024-12-15T06:30:00.000Z"),
            (-12, "2025-01-15T06:30:00.000Z"),
            (-11, "2025-02-15T06:30:00.000Z"),
            (-2, "2025-11-15T06:30:00.000Z"),
            (-1, "2025-12-15T06:30:00.000Z"),
            (0, "2026-01-15T06:30:00.000Z"),
            (11, "2026-12-15T06:30:00.000Z"),
            (12, "2027-01-15T06:30:00.000Z"),
            (13, "2027-02-15T06:30:00.000Z"),
            (23, "2027-12-15T06:30:00.000Z"),
            (24, "2028-01-15T06:30:00.000Z"),
            (25, "2028-02-15T06:30:00.000Z"),
            (100, "2034-05-15T06:30:00.000Z"),
        ];
        for (month, written) in months {
            assert_eq!(
                utc_ms(2026, month, 15, 6, 30).map(iso).as_deref(),
                Some(written),
                "month {month}"
            );
        }
        // Node's Date.UTC(year, month, day) for each.
        let days = [
            ((2026, 0, -400), "2024-11-26T00:00:00.000Z"),
            ((2026, 2, -1), "2026-02-27T00:00:00.000Z"),
            ((2024, 2, 0), "2024-02-29T00:00:00.000Z"),
            ((2023, 2, 0), "2023-02-28T00:00:00.000Z"),
            ((2100, 2, 0), "2100-02-28T00:00:00.000Z"),
            ((2000, 2, 0), "2000-02-29T00:00:00.000Z"),
            ((1900, 2, 0), "1900-02-28T00:00:00.000Z"),
            ((2026, 0, 366), "2027-01-01T00:00:00.000Z"),
            ((2024, 0, 366), "2024-12-31T00:00:00.000Z"),
            ((2024, 0, 367), "2025-01-01T00:00:00.000Z"),
        ];
        for ((year, month, day), written) in days {
            assert_eq!(
                utc_ms(year, month, day, 0, 0).map(iso).as_deref(),
                Some(written),
                "{year}, {month}, {day}"
            );
        }
    }

    #[test]
    fn a_year_from_0_to_99_is_of_the_1900s() {
        assert_eq!(utc_ms(0, 0, 1, 0, 0), Some(-2_208_988_800_000));
        assert_eq!(utc_ms(99, 11, 31, 0, 0), Some(946_598_400_000));
        assert_eq!(
            utc_ms(100, 0, 1, 0, 0),
            Some(-59_011_459_200_000),
            "the year 100 is the year 100"
        );
    }

    #[test]
    fn a_year_or_a_month_past_what_v8_reads_is_none_though_the_days_would_bring_it_back() {
        // Node 26's Date.UTC(year, month, day): a year may be a million either
        // side of 0 and a month ten million, whatever the day then makes of them.
        // V8 counts the days of a year before -399,999 a day off, as its
        // integer division cuts toward zero there.
        let read = [
            ((-1_000_000, 0, 365_242_500), "0000-01-01T00:00:00.000Z"),
            ((-400_001, 0, 146_097_500), "0000-05-15T00:00:00.000Z"),
            (
                (-1_000_000, -10_000_000, 700_000_000),
                "+083201-07-28T00:00:00.000Z",
            ),
            ((999_999, 0, -300_000_000), "+178626-11-24T00:00:00.000Z"),
            ((1_000_000, 0, -300_000_000), "+178627-11-24T00:00:00.000Z"),
            ((-399_999, 0, 200_000_000), "+147582-05-27T00:00:00.000Z"),
            (
                (2026, 10_000_000, -250_000_000),
                "+150882-07-29T00:00:00.000Z",
            ),
        ];
        for ((year, month, day), written) in read {
            assert_eq!(
                utc_ms(year, month, day, 0, 0).map(iso).as_deref(),
                Some(written),
                "{year}, {month}, {day}"
            );
        }
        // NaN, each.
        for (year, month, day) in [
            (1_000_001, 0, -300_000_000),
            (-1_000_001, 0, 300_000_000),
            (2026, 10_000_001, -250_000_000),
            (2026, -10_000_001, 250_000_000),
        ] {
            assert_eq!(
                utc_ms(year, month, day, 0, 0),
                None,
                "{year}, {month}, {day}"
            );
        }
    }

    #[test]
    fn each_product_and_sum_is_rounded_as_javascript_rounds_it() {
        // Node 26: the minutes' product is no double, and rounds before the sum.
        assert_eq!(
            utc_ms(1970, 0, 1, 100_000_000_000, -5_999_999_999_999),
            Some(60_032)
        );
        assert_eq!(
            utc_ms(1970, 0, 1, 2_400_000_000, 0),
            Some(8_640_000_000_000_000)
        );
        assert_eq!(
            utc_ms(1970, 0, 100_000_001, 0, 0),
            Some(8_640_000_000_000_000)
        );
    }

    #[test]
    fn a_wall_clock_past_what_a_date_holds_is_none() {
        // Each is NaN in Node 26.
        for (year, month, day, hour, minute) in [
            (275_760, 8, 13, 0, 1),
            (275_760, 8, 14, 0, 0),
            (-271_821, 3, 19, 23, 59),
            (1_000_000, 0, 1, 0, 0),
            (2026, 10_000_000, 1, 0, 0),
            (2026, -10_000_001, 1, 0, 0),
            (1_000_001, 0, 1, 0, 0),
            (2026, 0, 1_000_000_000, 0, 0),
            (2026, 0, 1, 0, i64::MAX),
        ] {
            assert_eq!(
                utc_ms(year, month, day, hour, minute),
                None,
                "{year}, {month}, {day}, {hour}, {minute}"
            );
        }
    }

    #[test]
    fn reads_what_date_parse_reads_in_its_format() {
        let cases = [
            ("2026-10-02T08:00:00.000Z", Some(1_790_928_000_000)),
            ("2026-10-02T08:00:00Z", Some(1_790_928_000_000)),
            ("2026-10-02T08:00Z", Some(1_790_928_000_000)),
            ("2026-10-02T11:00:00.000+03:00", Some(1_790_928_000_000)),
            ("2026-10-02T08:00:00.0001Z", Some(1_790_928_000_000)),
            ("2026-10-02T08:00:00.5Z", Some(1_790_928_000_500)),
            ("2026-10-02", Some(1_790_899_200_000)),
            ("2026-10", Some(1_790_812_800_000)),
            ("2026", Some(1_767_225_600_000)),
            ("2000-02-29T00:00:00.000Z", Some(951_782_400_000)),
            ("2026-10-02T24:00:00.000Z", Some(1_790_985_600_000)),
            ("+010000-01-01T00:00:00.000Z", Some(253_402_300_800_000)),
        ];
        for (text, ms) in cases {
            assert_eq!(parse(text), ms, "{text}");
        }
        for text in [
            "soon",
            "",
            "2026-13-01",
            "2026-10-02T25:00Z",
            // V8's legacy parser reads these three; the format does not hold them.
            "2026-10-02T08:00:00.000",
            "2026-10-02 08:00:00Z",
            "2026-02-31",
            "-000000-01-01T00:00:00Z",
            "2026-10-02T08:00:00.Z",
            "2026-10-02T24:00:01Z",
        ] {
            assert_eq!(parse(text), None, "{text:?} is no time");
        }
    }

    #[test]
    fn reads_back_what_it_writes() {
        for ms in [
            0,
            1,
            -1,
            1_790_928_000_123,
            951_782_400_000,
            -62_198_755_200_000,
        ] {
            assert_eq!(parse(&iso(ms)), Some(ms));
        }
    }
}
