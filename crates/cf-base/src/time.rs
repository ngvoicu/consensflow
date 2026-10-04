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
    (ms.abs() <= 8_640_000_000_000_000).then_some(ms)
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
