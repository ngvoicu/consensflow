//! What one case of a check saw, in the order it saw it: the lines it says at the
//! end of the case, and whether the case held. A line of what was seen is a line;
//! a check of the case is a line that says `ok  ` or `FAIL`, the name of what was
//! checked and what was seen, and a check that did not hold is a case that did not.
//! (A case that holds is no proof of more than its checks: what a case shows, it
//! shows in the lines.)

use std::fmt::Display;

use super::super::seconds;

/// What a case saw and whether it held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    lines: Vec<String>,
    held: bool,
}

impl Default for Account {
    fn default() -> Self {
        Self::new()
    }
}

impl Account {
    /// A case that saw nothing yet, and holds.
    pub const fn new() -> Self {
        Self {
            lines: Vec::new(),
            held: true,
        }
    }

    /// A line of what the case saw.
    pub fn say(&mut self, text: impl Display) {
        self.lines.push(text.to_string());
    }

    /// A check of the case: it held where `ok` says so, with what was seen
    /// either way.
    pub fn check(&mut self, name: &str, ok: bool, detail: impl Display) {
        self.lines.push(format!(
            "{} {name}: {detail}",
            if ok { "ok  " } else { "FAIL" }
        ));
        self.held &= ok;
    }

    /// A check that did not hold, as a line of its own: `FAIL` and what was seen.
    pub fn fail(&mut self, text: impl Display) {
        self.lines.push(format!("FAIL {text}"));
        self.held = false;
    }

    /// The case does not hold unless `ok`: for a condition whose lines the case
    /// has said already, or that has none.
    pub fn require(&mut self, ok: bool) {
        self.held &= ok;
    }

    /// Whether every check held.
    pub const fn held(&self) -> bool {
        self.held
    }

    /// What the case saw, a line to each.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// What it saw, to say as one text.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
}

/// `count` of `word`, as it is said of one or of more: `1 Escape`, `3 Escapes`.
pub fn plural(count: usize, word: &str) -> String {
    format!("{count} {word}{}", if count == 1 { "" } else { "s" })
}

/// A moment as the seconds since `from`, both in milliseconds: `+12.3 s`.
pub fn plus(stamp_ms: i64, from_ms: i64) -> String {
    format!("+{} s", seconds(stamp_ms - from_ms))
}

/// A moment that may not be known, as [`plus`] says it: `+? s` when it is not.
pub fn plus_maybe(stamp_ms: Option<i64>, from_ms: i64) -> String {
    stamp_ms.map_or_else(|| "+? s".to_owned(), |stamp| plus(stamp, from_ms))
}

/// `text` on one line (every run of white space a single space), and no more than
/// `most` characters of it, cut where it is: what a record's words are quoted by.
/// ([`super::super::cut`] marks a cut; a line that quotes a record does not.)
pub fn squash(text: &str, most: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(most)
        .collect()
}

/// The time of day an ISO 8601 time says (`2026-10-11T09:30:00.123Z` is
/// `09:30:00.123`), where it says one.
pub fn clock(at: &str) -> &str {
    at.get(11..23).unwrap_or(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_case_holds_until_a_check_of_it_does_not() {
        let mut case = Account::new();
        assert!(case.held());
        case.say("T-1 given");
        case.check("claimed", true, "m-4 claimed 1.2 s after it was given");
        assert!(case.held());
        case.check("never pasted", false, "1 delivery.begun");
        assert!(!case.held());
        // A check that held afterwards does not undo one that did not.
        case.check("waiting to working", true, "queued → waiting → working");
        assert!(!case.held());
        assert_eq!(
            case.lines(),
            [
                "T-1 given",
                "ok   claimed: m-4 claimed 1.2 s after it was given",
                "FAIL never pasted: 1 delivery.begun",
                "ok   waiting to working: queued → waiting → working",
            ]
        );
        assert_eq!(case.text().lines().count(), 4);
    }

    #[test]
    fn a_failure_with_no_name_and_a_condition_with_no_line_each_end_the_hold() {
        let mut case = Account::new();
        case.fail("the member never put its question on the board");
        assert_eq!(
            case.lines(),
            ["FAIL the member never put its question on the board"]
        );
        assert!(!case.held());
        let mut quiet = Account::new();
        quiet.require(true);
        assert!(quiet.held() && quiet.lines().is_empty());
        quiet.require(false);
        assert!(!quiet.held() && quiet.lines().is_empty());
        assert_eq!(Account::default(), Account::new());
    }

    #[test]
    fn a_count_is_said_in_the_singular_for_one_and_in_the_plural_for_any_other() {
        assert_eq!(plural(0, "Escape"), "0 Escapes");
        assert_eq!(plural(1, "Escape"), "1 Escape");
        assert_eq!(plural(3, "note"), "3 notes");
    }

    #[test]
    fn a_record_is_quoted_on_one_line_and_cut_where_it_is_with_no_mark_of_the_cut() {
        assert_eq!(squash("a\n  b\t\tc", 10), "a b c");
        assert_eq!(squash("abcdef", 3), "abc");
        assert_eq!(squash("ééé x", 4), "ééé ");
        assert_eq!(squash("", 5), "");
    }

    #[test]
    fn a_moment_that_is_not_known_is_said_so() {
        assert_eq!(plus_maybe(Some(2_000), 500), "+1.5 s");
        assert_eq!(plus_maybe(None, 500), "+? s");
    }

    #[test]
    fn a_moment_is_said_as_the_seconds_since_another_and_a_time_by_its_time_of_day() {
        assert_eq!(plus(13_300, 1_000), "+12.3 s");
        assert_eq!(plus(900, 1_000), "+-0.1 s");
        assert_eq!(clock("2026-10-11T09:30:00.123Z"), "09:30:00.123");
        assert_eq!(clock("soon"), "soon");
    }
}
