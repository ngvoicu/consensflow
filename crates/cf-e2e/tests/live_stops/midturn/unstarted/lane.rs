//! The member's lane over the wait: each change of what the board said of it (the
//! window it has, what the window does and why), with the moment it was seen at.

use cf_e2e::live::members::{plus, squash};
use serde_json::Value;

/// How many changes are said from each end of a long history.
const KEPT: usize = 8;

/// How much of the board's reason for a state a change says.
const REASON: usize = 100;

/// What the board said of the member's lane through the wait.
pub struct Waiting {
    started: i64,
    last: Option<String>,
    changes: Vec<String>,
    windowed: bool,
}

impl Waiting {
    /// A wait that began at `started`, in milliseconds since the epoch.
    pub const fn new(started: i64) -> Self {
        Self {
            started,
            last: None,
            changes: Vec::new(),
            windowed: false,
        }
    }

    /// The lane as the board has it at `now`: noted when it is other than it was.
    pub fn note(&mut self, lane: &Value, now: i64) {
        self.windowed |= lane["pane"]["id"].is_string();
        let word = said(lane);
        if self.last.as_ref() != Some(&word) {
            self.changes
                .push(format!("{word} at {}", plus(now, self.started)));
            self.last = Some(word);
        }
    }

    /// The history, as a line: the first changes and the last, where it is long.
    pub fn line(&self) -> String {
        if self.changes.is_empty() {
            return "never looked at".to_owned();
        }
        if self.changes.len() <= 2 * KEPT {
            return self.changes.join(", ");
        }
        let skipped = self.changes.len() - 2 * KEPT;
        format!(
            "{}, … {skipped} more changes …, {}",
            self.changes[..KEPT].join(", "),
            self.changes[self.changes.len() - KEPT..].join(", ")
        )
    }

    /// Whether the lane ever had a window.
    pub const fn had_a_window(&self) -> bool {
        self.windowed
    }
}

/// The lane as one word of the history: its window (or none), what it does, and
/// the board's reason for it where it gives one.
pub fn said(lane: &Value) -> String {
    let window = lane["pane"]["id"].as_str().unwrap_or("no window");
    let state = lane["activity"]["state"].as_str().unwrap_or("undefined");
    let reason = lane["activity"]["reason"]
        .as_str()
        .filter(|reason| !reason.is_empty())
        .map(|reason| format!(" ({})", squash(reason, REASON)))
        .unwrap_or_default();
    format!("{window}: {state}{reason}")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn lane(pane: Option<&str>, state: &str, reason: Option<&str>) -> Value {
        json!({
            "pane": pane.map(|id| json!({ "id": id, "generation": 1 })),
            "activity": { "state": state, "reason": reason },
        })
    }

    #[test]
    fn the_lane_is_said_by_its_window_what_it_does_and_the_boards_reason() {
        assert_eq!(said(&lane(None, "closed", None)), "no window: closed");
        assert_eq!(
            said(&lane(
                Some("p1-devin-a-b"),
                "unknown",
                Some("the record could not be read")
            )),
            "p1-devin-a-b: unknown (the record could not be read)"
        );
        assert_eq!(said(&Value::Null), "no window: undefined");
        let long = "word ".repeat(60);
        assert_eq!(
            said(&lane(Some("p"), "waiting", Some(&long))),
            format!("p: waiting ({})", squash(&long, REASON))
        );
    }

    #[test]
    fn only_a_change_of_the_lane_is_a_line_of_its_history_with_the_moment_it_was_seen() {
        let mut waiting = Waiting::new(1_000);
        assert_eq!(waiting.line(), "never looked at");
        assert!(!waiting.had_a_window());
        waiting.note(&lane(None, "closed", None), 1_000);
        waiting.note(&lane(None, "closed", None), 1_500);
        assert!(!waiting.had_a_window());
        waiting.note(&lane(Some("p1-devin-a-b"), "starting", None), 13_200);
        waiting.note(&lane(Some("p1-devin-a-b"), "starting", None), 14_000);
        assert!(waiting.had_a_window());
        waiting.note(
            &lane(
                Some("p1-devin-a-b"),
                "unknown",
                Some("the record could not be read"),
            ),
            20_000,
        );
        waiting.note(&lane(None, "closed", None), 258_400);
        assert_eq!(
            waiting.line(),
            "no window: closed at +0.0 s, p1-devin-a-b: starting at +12.2 s, \
             p1-devin-a-b: unknown (the record could not be read) at +19.0 s, \
             no window: closed at +257.4 s"
        );
    }

    #[test]
    fn a_long_history_is_said_by_its_first_changes_and_its_last() {
        let mut waiting = Waiting::new(0);
        for at in 0..30 {
            let state = if at % 2 == 0 { "working" } else { "idle" };
            waiting.note(&lane(Some("p"), state, None), at * 1_000);
        }
        let line = waiting.line();
        assert!(
            line.starts_with("p: working at +0.0 s, p: idle at +1.0 s"),
            "{line}"
        );
        assert!(line.contains(", … 14 more changes …, "), "{line}");
        assert!(line.ends_with("p: idle at +29.0 s"), "{line}");
        assert_eq!(line.matches(" at +").count(), 2 * KEPT);
    }
}
