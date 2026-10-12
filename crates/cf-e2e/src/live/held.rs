//! What the board says of a worker's window besides what its state says, and which of
//! its words say that the daemon holds the task's result back for what the turn left
//! running. A check that waits for a worker's task looks at the board once a second
//! and notes each word it finds there once, with when it was first and last said, so
//! that a run that fails can say what the window was held for, and for how long.

use std::time::Duration;

use super::secs;

/// Whether the board's `reason` for a window says its task's result is held back for
/// what the turn left: the words of the engine for it begin `background`
/// (`background work running (2 processes): …` for the processes of the window's tree,
/// `background work ended; the agent may go on` for the moments after they have, and
/// `background agents running` for the agents or workflows its harness says are
/// pending). A reason the board gives for any other thing (`waiting for the human`)
/// holds nothing back for work.
pub fn is_hold(reason: &str) -> bool {
    reason.starts_with("background ")
}

/// A word the board said of a window, and when: the first and the last look of the run
/// that found it said, each as the time since the run began.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub reason: String,
    pub first: Duration,
    pub last: Duration,
}

impl Said {
    /// The word and when it was said: `"background work running (1 process): sleep 9"
    /// (said from 14.0 s to 61.0 s)`.
    pub fn line(&self) -> String {
        format!(
            "\"{}\" (said from {} s to {} s)",
            self.reason,
            secs(self.first),
            secs(self.last)
        )
    }
}

/// What the board said of a window, each word once, in the order it first said them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Reasons {
    said: Vec<Said>,
}

impl Reasons {
    /// The board's word for a window at the look made `at` into the run: noted once, and
    /// said last then where it was said before. The board says none for a window it has
    /// nothing to add of, and an empty word is no word.
    pub fn note(&mut self, at: Duration, reason: &str) {
        if reason.is_empty() {
            return;
        }
        match self.said.iter_mut().find(|said| said.reason == reason) {
            Some(said) => said.last = at,
            None => self.said.push(Said {
                reason: reason.to_owned(),
                first: at,
                last: at,
            }),
        }
    }

    /// Every word the board said, in the order it first said them.
    pub fn said(&self) -> &[Said] {
        &self.said
    }

    /// The words that say the window is held for what its turn left ([`is_hold`]).
    pub fn holds(&self) -> Vec<&Said> {
        self.said
            .iter()
            .filter(|said| is_hold(&said.reason))
            .collect()
    }

    /// How long the board said the window was held: from the first look that found it
    /// said to the last, whichever of its words said it. None where it never said so.
    /// (A look a second apart, so the hold lasted at least this long, and a look or two
    /// more.)
    pub fn held_between(&self) -> Option<(Duration, Duration)> {
        let holds = self.holds();
        let from = holds.iter().map(|said| said.first).min()?;
        let to = holds.iter().map(|said| said.last).max()?;
        Some((from, to))
    }

    /// What the board said of a window held for what its turn left, for a verdict: each
    /// word and when it was said, `|` between them. None where it said none.
    pub fn held(&self) -> Option<String> {
        let holds = self.holds();
        if holds.is_empty() {
            return None;
        }
        let lines: Vec<String> = holds.iter().map(|said| said.line()).collect();
        Some(lines.join(" | "))
    }

    /// Everything the board said, a line to each word, for a report; `(nothing)` where
    /// it said none.
    pub fn block(&self) -> String {
        self.joined("\n")
    }

    /// Everything the board said, on one line, `|` between the words; `(nothing)` where
    /// it said none.
    pub fn line(&self) -> String {
        self.joined(" | ")
    }

    fn joined(&self, between: &str) -> String {
        if self.said.is_empty() {
            return "(nothing)".to_owned();
        }
        let lines: Vec<String> = self.said.iter().map(Said::line).collect();
        lines.join(between)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNING: &str =
        "background work running (2 processes): npm exec @playwright/mcp; node server.js";

    fn at(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    #[test]
    fn the_words_that_say_the_result_is_held_for_work_are_the_engines_and_no_other() {
        for hold in [
            RUNNING,
            "background work running (1 process): hostname",
            "background work running (10 processes)",
            "background work ended; the agent may go on",
            "background agents running",
        ] {
            assert!(is_hold(hold), "{hold}");
        }
        for other in [
            "",
            "waiting for the human",
            "waiting for the answer to a question",
            "backgrounded",
            "Background work running",
        ] {
            assert!(!is_hold(other), "{other:?}");
        }
    }

    #[test]
    fn a_word_is_noted_once_with_when_it_was_first_and_last_said_and_an_empty_one_is_none() {
        let mut reasons = Reasons::default();
        reasons.note(at(1), "");
        reasons.note(at(2), "waiting for the human");
        reasons.note(at(3), RUNNING);
        reasons.note(at(4), "");
        reasons.note(at(5), RUNNING);
        reasons.note(at(6), "waiting for the human");
        assert_eq!(
            reasons.said(),
            [
                Said {
                    reason: "waiting for the human".to_owned(),
                    first: at(2),
                    last: at(6)
                },
                Said {
                    reason: RUNNING.to_owned(),
                    first: at(3),
                    last: at(5)
                },
            ]
        );
        assert_eq!(Reasons::default().said(), []);
    }

    #[test]
    fn only_the_words_of_a_hold_are_holds_and_none_said_is_no_hold() {
        let mut reasons = Reasons::default();
        assert!(reasons.holds().is_empty());
        assert_eq!(reasons.held_between(), None);
        assert_eq!(reasons.held(), None);
        reasons.note(at(2), "waiting for the human");
        assert!(reasons.holds().is_empty());
        assert_eq!(reasons.held_between(), None);
        assert_eq!(reasons.held(), None);
        reasons.note(at(3), "background agents running");
        let holds: Vec<&str> = reasons
            .holds()
            .iter()
            .map(|said| said.reason.as_str())
            .collect();
        assert_eq!(holds, ["background agents running"]);
    }

    #[test]
    fn a_window_is_held_from_the_first_look_that_found_it_said_to_the_last_whichever_word_said_it()
    {
        let mut reasons = Reasons::default();
        reasons.note(at(10), RUNNING);
        reasons.note(at(11), RUNNING);
        reasons.note(at(12), "background work ended; the agent may go on");
        reasons.note(at(20), "background work ended; the agent may go on");
        reasons.note(at(21), "waiting for the human");
        assert_eq!(reasons.held_between(), Some((at(10), at(20))));
    }

    #[test]
    fn a_hold_is_said_as_each_word_and_when_it_was_said_and_what_else_the_board_said_is_left_out() {
        let mut reasons = Reasons::default();
        reasons.note(at(1), "waiting for the human");
        reasons.note(at(14), RUNNING);
        reasons.note(at(61), RUNNING);
        reasons.note(at(62), "background work ended; the agent may go on");
        assert_eq!(
            reasons.held().as_deref(),
            Some(
                "\"background work running (2 processes): npm exec @playwright/mcp; node server.js\" (said from 14.0 s to 61.0 s) | \"background work ended; the agent may go on\" (said from 62.0 s to 62.0 s)"
            )
        );
    }

    #[test]
    fn everything_the_board_said_is_a_block_of_lines_or_a_line_and_nothing_is_said_so() {
        let mut reasons = Reasons::default();
        assert_eq!(reasons.block(), "(nothing)");
        assert_eq!(reasons.line(), "(nothing)");
        reasons.note(at(1), "waiting for the human");
        reasons.note(at(14), "background agents running");
        assert_eq!(
            reasons.block(),
            "\"waiting for the human\" (said from 1.0 s to 1.0 s)\n\"background agents running\" (said from 14.0 s to 14.0 s)"
        );
        assert_eq!(
            reasons.line(),
            "\"waiting for the human\" (said from 1.0 s to 1.0 s) | \"background agents running\" (said from 14.0 s to 14.0 s)"
        );
    }
}
