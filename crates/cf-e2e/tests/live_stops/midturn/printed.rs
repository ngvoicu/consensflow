//! What a window printed: how many lines of the numbers it was asked for, how far
//! they have come on its screen as the pane host draws it, and what else it drew
//! since the pause (a hint of its own, a footer, a dialog).

use cf_e2e::live::members::{plural, squash};
use serde_json::Value;

/// How many lines of `screen` are numbers and no more.
pub fn numbers_on(screen: &str) -> usize {
    screen.lines().filter(|line| is_number(line)).count()
}

/// How far the numbers have come on `screen`: the highest number on a line that is
/// numbers and no word, a line that wraps a run of them included, and none above
/// `most`, which no one wrote (a run of rows that a repaint glued, a count).
pub fn progress(screen: &str, most: usize) -> usize {
    screen
        .lines()
        .filter(|line| is_number(line))
        .flat_map(|line| line.split(|at: char| !at.is_ascii_digit()))
        .filter_map(|run| run.parse::<usize>().ok())
        .filter(|number| *number <= most)
        .max()
        .unwrap_or(0)
}

/// The lines of the screen that a `pane.snapshot` answer of the pane host holds, as it
/// draws it: none where it holds none.
pub fn tail_lines(snapshot: &Value) -> Vec<String> {
    snapshot["tail"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|line| line.as_str().map(str::to_owned))
        .collect()
}

/// Whether the line holds digits and nothing but digits of letters or numbers: a TUI
/// draws a number with a border, a bullet or its neighbours (a paragraph it wraps
/// holds a run of them), none of which is a word.
fn is_number(line: &str) -> bool {
    let mut words = line.chars().filter(|character| character.is_alphanumeric());
    let first = words.next();
    first.is_some_and(|first| first.is_ascii_digit()) && words.all(|rest| rest.is_ascii_digit())
}

/// What a window printed after a mark: the lines of numbers it went on with, and
/// what else it drew, the last of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printed {
    pub numbers: usize,
    pub others: Vec<String>,
}

/// How many lines of what is not a number a line of the case quotes.
const OTHERS: usize = 6;

/// What `screen` holds after its first `mark` characters.
pub fn printed_since(screen: &str, mark: usize) -> Printed {
    let after: String = screen.chars().skip(mark).collect();
    let mut others: Vec<String> = Vec::new();
    // What else it drew is what has a word or a number in it that is not a number alone:
    // a border, a rule, a prompt's mark is drawing and says nothing.
    for line in after
        .lines()
        .filter(|line| !is_number(line))
        .map(str::trim)
        .filter(|line| line.chars().any(char::is_alphanumeric))
    {
        let line = squash(line, 100);
        // A screen drawn again says its footer again: once, until it says another.
        if others.last() != Some(&line) {
            others.push(line);
        }
    }
    others.drain(..others.len().saturating_sub(OTHERS));
    Printed {
        numbers: numbers_on(&after),
        others,
    }
}

impl Printed {
    /// What else it drew, as a part of a line of the case: the lines, or `nothing`.
    pub fn others_line(&self) -> String {
        if self.others.is_empty() {
            "nothing".to_owned()
        } else {
            self.others.join(" ⏎ ")
        }
    }

    /// As a part of a line of the case: how many lines of numbers, and what else.
    pub fn line(&self) -> String {
        format!(
            "{} of numbers; besides: {}",
            plural(self.numbers, "line"),
            self.others_line()
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_line_is_a_number_when_it_holds_digits_and_no_word() {
        let screen = "ready\n 12 \n13\n\nv2\n3000\n";
        assert_eq!(numbers_on(screen), 3);
        assert_eq!(numbers_on(""), 0);
    }

    #[test]
    fn a_number_a_tui_draws_with_a_border_a_bullet_or_its_neighbours_is_still_a_number() {
        let drawn =
            "│ 123 │\n● 124\n  125 126 127 128\n│ 1,5 │\n 129 ▌\nesc twice to interrupt\n│ it │\n";
        assert_eq!(numbers_on(drawn), 5);
        // A line with a word in it, however many numbers, is not.
        assert_eq!(numbers_on("10 apples\nv2 3\n│   │\n"), 0);
    }

    #[test]
    fn what_a_window_printed_since_the_mark_is_its_numbers_and_the_last_other_lines_once_each() {
        let before = "start\n1\n2\n";
        let screen = format!(
            "{before}3\n4\nesc twice to interrupt\n5\nesc twice to interrupt\n6\nesc again to interrupt\n7\n"
        );
        let printed = printed_since(&screen, before.chars().count());
        assert_eq!(printed.numbers, 5);
        // The footer drawn again among the numbers is said once.
        assert_eq!(
            printed.others,
            ["esc twice to interrupt", "esc again to interrupt"]
        );
        assert_eq!(
            printed.line(),
            "5 lines of numbers; besides: esc twice to interrupt ⏎ esc again to interrupt"
        );
        let again = printed_since("a\nfooter\nfooter\n", 0);
        assert_eq!(again.others, ["a", "footer"]);
        // A border or a rule says nothing, and a line that is a number with a border is a number.
        let drawn = printed_since("╭────╮\n│ 7 │\n│ Interrupted │\n╰────╯\n", 0);
        assert_eq!(
            (drawn.numbers, drawn.others.as_slice()),
            (1, ["│ Interrupted │".to_owned()].as_slice())
        );
        let none = printed_since("1\n2\n", 0);
        assert_eq!(none.line(), "2 lines of numbers; besides: nothing");
        // What else it drew, said alone, for a case whose member writes no numbers.
        assert_eq!(none.others_line(), "nothing");
        let drew = printed_since("64 bytes from 127.0.0.1\nCanceled\n", 0);
        assert_eq!(drew.others_line(), "64 bytes from 127.0.0.1 ⏎ Canceled");
        assert_eq!(printed_since("ab", 5).numbers, 0);
    }

    #[test]
    fn the_numbers_have_come_as_far_as_the_highest_on_a_line_that_is_numbers_and_no_word() {
        // A screen as the pane host draws it: a few lines of the reply among a TUI's own.
        let drawn = "devin 3000.11.3\n│ 1180 │\n● 1181\n  1182 1183 1184\nesc twice to interrupt\n";
        assert_eq!(progress(drawn, 5_000), 1_184);
        // A run of rows that a repaint glued is no number anyone wrote, and a count over
        // what was asked for is none either; neither moves it.
        assert_eq!(progress("40\n123412351236\n99999\n", 5_000), 40);
        assert_eq!(progress("1180\n1181\n", 1_000), 0);
        // A version and a footer are words, and a number too long for any is no number.
        assert_eq!(progress("v2\nesc 5\n3000.11.3 beta\n", 5_000), 0);
        assert_eq!(progress(&"9".repeat(40), 5_000), 0);
        assert_eq!(progress("", 5_000), 0);
    }

    #[test]
    fn the_lines_a_snapshot_holds_are_its_tail_and_none_where_it_has_none() {
        let snapshot = json!({ "ok": true, "tail": ["one", "2", 3, "four"] });
        assert_eq!(tail_lines(&snapshot), ["one", "2", "four"]);
        assert!(tail_lines(&json!({ "ok": true })).is_empty());
        assert!(tail_lines(&Value::Null).is_empty());
    }

    #[test]
    fn only_the_last_lines_of_what_else_a_window_drew_are_quoted() {
        let screen: String = (1..=10).map(|at| format!("line {at}\n")).collect();
        let printed = printed_since(&screen, 0);
        assert_eq!(printed.others.len(), OTHERS);
        assert_eq!(printed.others.last().map(String::as_str), Some("line 10"));
        assert_eq!(printed.others.first().map(String::as_str), Some("line 5"));
    }
}
