//! What the check did to a window, in order, and what it saw: the paste and the keys
//! of its trail, and the words of Devin's that say a tool call did not run or a
//! permission was asked, each with where they were seen, when, and the last thing
//! the check did before them, which is what answered a dialog where one was
//! answered by a key.

use std::fmt::Display;
use std::time::{Duration, Instant};

use cf_e2e::live::secs;

use crate::words::SKIPPED;

/// `+12.3 s`: a time since the trail began.
pub fn plus(at: Duration) -> String {
    format!("+{} s", secs(at))
}

/// What the check did to a window, in order, and when: each paste and each key.
#[derive(Debug)]
pub struct Trail {
    began: Instant,
    steps: Vec<(Duration, String)>,
}

impl Trail {
    pub fn new() -> Self {
        Self {
            began: Instant::now(),
            steps: Vec::new(),
        }
    }

    /// The time since the trail began.
    pub fn at(&self) -> Duration {
        self.began.elapsed()
    }

    /// Notes what was just done.
    pub fn note(&mut self, what: impl Display) {
        self.steps.push((self.at(), what.to_string()));
    }

    /// How many steps there are: a mark to ask what was done after ([`Trail::since`]).
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// The steps after the first `from`, as a line, each with its time.
    pub fn since(&self, from: usize) -> String {
        let steps: Vec<String> = self
            .steps
            .iter()
            .skip(from)
            .map(|(at, what)| format!("{what} {}", plus(*at)))
            .collect();
        if steps.is_empty() {
            "none".to_owned()
        } else {
            steps.join(", ")
        }
    }

    /// The last step at or before `at`, with its time.
    pub fn before(&self, at: Duration) -> Option<String> {
        self.steps
            .iter()
            .rev()
            .find(|(done, _)| *done <= at)
            .map(|(done, what)| format!("{what} {}", plus(*done)))
    }
}

/// Words of Devin's, or of its wire, that say a call did not run or a permission was
/// asked or answered, or a screen that holds a window still, seen in a place at a
/// time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    /// Where: `screen`, `store` or `wire`.
    pub source: &'static str,
    /// What: one of the markers, `permission request` / `permission answered` where
    /// the wire says one, or `stalled screen`.
    pub saw: String,
    pub at: Duration,
    /// The last thing the check did before it, where it did any.
    pub after: Option<String>,
    pub excerpt: String,
}

impl Sighting {
    /// Whether it says the user skipped the call.
    pub fn is_skip(&self) -> bool {
        self.saw.eq_ignore_ascii_case(SKIPPED)
    }

    /// As a line of the report.
    pub fn line(&self) -> String {
        format!(
            "{} \"{}\" at {}, after {}: {}",
            self.source,
            self.saw,
            plus(self.at),
            self.after.as_deref().unwrap_or("none of our keys"),
            self.excerpt
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(milliseconds: u64) -> Duration {
        Duration::from_millis(milliseconds)
    }

    #[test]
    fn the_trail_says_what_was_done_after_a_mark_and_the_last_thing_before_a_time() {
        let trail = Trail {
            began: Instant::now(),
            steps: vec![
                (ms(100), "paste m-10 (80 chars)".to_owned()),
                (ms(10_400), "Enter again".to_owned()),
            ],
        };
        assert_eq!(trail.len(), 2);
        assert_eq!(
            trail.since(0),
            "paste m-10 (80 chars) +0.1 s, Enter again +10.4 s"
        );
        assert_eq!(trail.since(1), "Enter again +10.4 s");
        assert_eq!(trail.since(2), "none");
        assert_eq!(trail.before(ms(50)), None);
        assert_eq!(
            trail.before(ms(5_000)).as_deref(),
            Some("paste m-10 (80 chars) +0.1 s")
        );
        assert_eq!(
            trail.before(ms(12_000)).as_deref(),
            Some("Enter again +10.4 s")
        );
    }

    #[test]
    fn what_is_done_is_noted_as_it_is_done() {
        let mut trail = Trail::new();
        assert_eq!(trail.since(0), "none");
        trail.note("paste m-10");
        trail.note("Enter again");
        assert_eq!(trail.len(), 2);
        assert!(trail.since(1).starts_with("Enter again +"));
        assert!(trail.before(trail.at()).is_some());
    }

    #[test]
    fn a_sighting_is_a_line_that_says_where_what_when_and_after_which_key() {
        let skip = Sighting {
            source: "screen",
            saw: "User skipped this tool call".to_owned(),
            at: ms(12_300),
            after: Some("Enter again +10.4 s".to_owned()),
            excerpt: "rejected: User skipped this tool call".to_owned(),
        };
        assert!(skip.is_skip());
        assert_eq!(
            skip.line(),
            "screen \"User skipped this tool call\" at +12.3 s, after Enter again +10.4 s: rejected: User skipped this tool call"
        );
        let asked = Sighting {
            saw: "permission request".to_owned(),
            after: None,
            ..skip
        };
        assert!(!asked.is_skip());
        assert!(asked.line().contains("after none of our keys"));
    }
}
