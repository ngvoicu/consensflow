//! The time as the evals write it and read it back: ISO 8601 with milliseconds
//! and a `Z`, as the ledger and the daemon's events have it, and the stamp a
//! report's file is named by.

use jiff::Timestamp;

/// `at` as the ledger writes a time: `2026-10-11T09:30:00.123Z`.
pub fn iso(at: Timestamp) -> String {
    at.strftime("%Y-%m-%dT%H:%M:%S.%3fZ").to_string()
}

/// The time now, as the ledger writes one.
pub fn now() -> String {
    iso(Timestamp::now())
}

/// What a report's file is named by: [`iso`] with its colons and its dot made
/// dashes (`2026-10-11T09-30-00-123Z`), so that the name sorts by time and is a
/// name on every platform.
pub fn stamp(at: Timestamp) -> String {
    iso(at).replace([':', '.'], "-")
}

/// When an ISO 8601 time says it is, in milliseconds since the epoch: none for
/// text that is no time.
pub fn millis(text: &str) -> Option<i64> {
    text.parse::<Timestamp>()
        .ok()
        .map(|time| time.as_millisecond())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(milliseconds: i64) -> Timestamp {
        Timestamp::from_millisecond(milliseconds).unwrap()
    }

    #[test]
    fn a_time_is_written_as_the_ledger_writes_one_with_three_digits_of_a_second() {
        assert_eq!(iso(at(1_791_711_000_123)), "2026-10-11T09:30:00.123Z");
        // A whole second still has its three digits, which JavaScript's toISOString gives.
        assert_eq!(iso(at(1_791_711_000_000)), "2026-10-11T09:30:00.000Z");
        assert!(now().ends_with('Z') && now().len() == 24, "{}", now());
    }

    #[test]
    fn a_report_is_stamped_without_the_colons_and_the_dot_a_file_name_cannot_hold() {
        assert_eq!(stamp(at(1_791_711_000_123)), "2026-10-11T09-30-00-123Z");
        // The old reports' names have the same shape.
        let old = "2026-09-26T00-13-20-553Z";
        assert_eq!(old.len(), stamp(at(0)).len());
    }

    #[test]
    fn a_time_is_read_back_in_milliseconds_and_text_that_is_no_time_is_none() {
        assert_eq!(millis("2026-10-11T09:30:00.123Z"), Some(1_791_711_000_123));
        assert_eq!(millis("2026-10-11T09:30:00Z"), Some(1_791_711_000_000));
        assert_eq!(millis("t"), None);
        assert_eq!(millis(""), None);
    }
}
