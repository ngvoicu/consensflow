//! The publication date: an RFC 3339 time with its zone said (`Z` or an
//! offset), as `2026-09-09T12:00:00Z`. The feed carries the text as it was
//! given, so this only holds it to the form installed apps parse, and to a
//! time that exists: a month from 1 to 12, a day the month has, an hour from 0
//! to 23 and a minute or a second from 0 to 59.

/// A date the feed will not carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("publication date must be RFC3339 with an explicit timezone")]
pub struct NotRfc3339;

/// Holds `date` to the form: `YYYY-MM-DDTHH:MM:SS`, a fraction of a second if
/// there is one, then `Z` or `+HH:MM` or `-HH:MM`. The letters are capitals.
pub fn check(date: &str) -> Result<(), NotRfc3339> {
    parse(&mut date.as_bytes()).ok_or(NotRfc3339)
}

/// Reads the whole of `rest`, or says it is not a date.
fn parse(rest: &mut &[u8]) -> Option<()> {
    let year = number(rest, 4)?;
    literal(rest, b'-')?;
    let month = number(rest, 2)?;
    literal(rest, b'-')?;
    let day = number(rest, 2)?;
    literal(rest, b'T')?;
    let hour = number(rest, 2)?;
    literal(rest, b':')?;
    let minute = number(rest, 2)?;
    literal(rest, b':')?;
    let second = number(rest, 2)?;
    if literal(rest, b'.').is_some() {
        digits(rest)?;
    }
    if literal(rest, b'Z').is_none() {
        offset(rest)?;
    }
    let exists = (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour < 24
        && minute < 60
        && second < 60;
    (exists && rest.is_empty()).then_some(())
}

/// `+HH:MM` or `-HH:MM`: an hour of the day and a minute of the hour.
fn offset(rest: &mut &[u8]) -> Option<()> {
    let (sign, after) = rest.split_first()?;
    if !matches!(sign, b'+' | b'-') {
        return None;
    }
    *rest = after;
    let hour = number(rest, 2)?;
    literal(rest, b':')?;
    let minute = number(rest, 2)?;
    (hour < 24 && minute < 60).then_some(())
}

/// `count` digits, as the number they make.
fn number(rest: &mut &[u8], count: usize) -> Option<u32> {
    let (digits, after) = rest.split_at_checked(count)?;
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    *rest = after;
    Some(
        digits
            .iter()
            .fold(0, |sum, digit| sum * 10 + u32::from(digit - b'0')),
    )
}

/// One or more digits.
fn digits(rest: &mut &[u8]) -> Option<()> {
    let count = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
    *rest = &rest[count..];
    (count > 0).then_some(())
}

/// `byte`, if it is what comes next.
fn literal(rest: &mut &[u8], byte: u8) -> Option<()> {
    match rest.split_first() {
        Some((first, after)) if *first == byte => {
            *rest = after;
            Some(())
        }
        _ => None,
    }
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_with_its_zone_said_is_a_date() {
        for date in [
            "2026-09-09T12:00:00Z",
            "2026-10-09T06:30:15.123+02:00",
            "2026-10-09T06:30:15.1234567890123-11:59",
            "2026-12-31T23:59:59Z",
            "0000-01-01T00:00:00Z",
            "2024-02-29T00:00:00Z",
            "2000-02-29T00:00:00+23:59",
        ] {
            assert_eq!(check(date), Ok(()), "{date}");
        }
    }

    #[test]
    fn what_is_not_in_the_form_is_refused() {
        for date in [
            "",
            "next friday",
            "2026-09-09",
            "2026-09-09T12:00",
            "2026-09-09 12:00:00Z",
            "2026-09-09t12:00:00Z",
            "2026-09-09T12:00:00z",
            // The zone is said, and as the form has it.
            "2026-09-09T12:00:00",
            "2026-09-09T12:00:00+0200",
            "2026-09-09T12:00:00+02",
            "2026-09-09T12:00:00 +02:00",
            "2026-09-09T12:00:00ZZ",
            "2026-09-09T12:00:00Z ",
            " 2026-09-09T12:00:00Z",
            "2026-09-09T12:00:00Z\n",
            // A fraction has digits.
            "2026-09-09T12:00:00.Z",
            "2026-09-09T12:00:00,5Z",
            // Digits are the ASCII ones, in the width the form has.
            "26-09-09T12:00:00Z",
            "12026-09-09T12:00:00Z",
            "2026-9-9T12:00:00Z",
            "２０２６-09-09T12:00:00Z",
            "2026-09-09T12:00:0aZ",
            "+026-09-09T12:00:00Z",
        ] {
            assert_eq!(check(date), Err(NotRfc3339), "{date:?}");
        }
    }

    #[test]
    fn a_time_that_does_not_exist_is_refused() {
        for date in [
            "2026-13-99T99:99:99Z",
            "2026-00-10T12:00:00Z",
            "2026-13-10T12:00:00Z",
            "2026-09-00T12:00:00Z",
            "2026-09-31T12:00:00Z",
            "2026-02-29T12:00:00Z",
            "1900-02-29T12:00:00Z",
            "2026-01-32T12:00:00Z",
            "2026-09-09T24:00:00Z",
            "2026-09-09T12:60:00Z",
            "2026-09-09T12:00:60Z",
            "2026-09-09T12:00:00+24:00",
            "2026-09-09T12:00:00-02:60",
        ] {
            assert_eq!(check(date), Err(NotRfc3339), "{date}");
        }
    }

    #[test]
    fn every_month_has_the_days_it_has() {
        let lengths = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        for (at, days) in lengths.into_iter().enumerate() {
            let month = at + 1;
            let last = format!("2026-{month:02}-{days:02}T00:00:00Z");
            let after = format!("2026-{month:02}-{:02}T00:00:00Z", days + 1);
            assert_eq!(check(&last), Ok(()), "{last}");
            assert_eq!(check(&after), Err(NotRfc3339), "{after}");
        }
        // Leap years: every fourth, but not every hundredth, unless every four hundredth.
        for (year, leap) in [(2024, true), (2100, false), (2000, true), (2026, false)] {
            let day = format!("{year}-02-29T00:00:00Z");
            assert_eq!(check(&day).is_ok(), leap, "{day}");
        }
    }

    #[test]
    fn the_refusal_is_in_the_words_of_the_script_it_ports() {
        assert_eq!(
            NotRfc3339.to_string(),
            "publication date must be RFC3339 with an explicit timezone"
        );
    }
}
