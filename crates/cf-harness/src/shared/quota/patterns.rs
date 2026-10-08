//! The patterns of Node's quota code, each compiled once, the first time it is
//! used, as JavaScript reads it (`shared::pattern`).

use std::sync::LazyLock;

use regex::Regex;

use crate::shared::pattern::compile;

/// `/resets?\s+in\s+((?:\d+\s*[a-z]+[\s,]*(?:and\s+)?)+)/i`: a span of time,
/// "Resets in 3hr 4min", "reset in 1 week and 2 days". Group 1 is the span.
pub(super) static SPAN: LazyLock<Regex> = LazyLock::new(|| {
    compile(r"(?i-u:resets?)\s+(?i-u:in)\s+((?:[0-9]+\s*[a-zA-Z]+[\s,]*(?:(?i-u:and)\s+)?)+)")
});

/// `/(\d+)\s*([a-z]+)/gi`: one number and its unit within a span. Groups
/// 1 and 2.
pub(super) static UNIT: LazyLock<Regex> = LazyLock::new(|| compile(r"([0-9]+)\s*([a-zA-Z]+)"));

/// A time of day, "resets 7:30pm (Europe/Bucharest)", or on a date, "resets
/// Sep 29 at 11am (Europe/Bucharest)". Groups 1 to 6: the month's name, its
/// day, the hour, the minute, `am` or `pm`, and the zone.
///
/// `/resets?\s+(?:([a-z]{3})[a-z]*\s+(\d{1,2})\s+at\s+)?(\d{1,2})(?::(\d{2}))?\s*([ap]m)\b(?:\s*\(([^)]+)\))?/i`
pub(super) static AT: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"(?i-u:resets?)\s+(?:([a-zA-Z]{3})[a-zA-Z]*\s+([0-9]{1,2})\s+(?i-u:at)\s+)?([0-9]{1,2})(?::([0-9]{2}))?\s*((?i-u:[ap]m))(?-u:\b)(?:\s*\(([^)]+)\))?",
    )
});

/// `/^(?:[^(:\n]*\()?(\d{3})\b/`: a status the text opens on, "429: …" or
/// "OpenAI API error (429): …". Group 1 is the status.
pub(super) static REFUSED: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^(?:[^(:\n]*\()?([0-9]{3})(?-u:\b)"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_builds() {
        for pattern in [&SPAN, &UNIT, &AT, &REFUSED] {
            assert!(pattern.captures_len() > 1, "{}", pattern.as_str());
        }
    }
}
