//! What Devin's terminal says of what it is doing: a mark for each change, in a control
//! sequence of its own (`OSC 7501`) that a terminal which does not know it passes by.
//! `state=` is `idle`, `working`, `blocked` or `done`; `kind` names what a blocked window
//! waits on (`permission`); `msg` is the words of its status line, in base64. It is
//! Devin's own word for a window that waits for an answer, so a dialog is told by it
//! and not by guessing at the words of one.

use std::time::Duration;

use crate::trail::plus;

/// How a mark begins in the text of a screen (the escape before it is not in the text),
/// and the character that ends it (the backslash of its terminator).
const START: &str = "]7501;";
const END: char = '\\';

/// How much of a mark that has not ended yet is kept for the next look: more than any
/// mark Devin writes, so that a stray start does not make the rest of the screen wait.
const PENDING: usize = 400;

/// One mark: what the window is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    /// `idle`, `working`, `blocked` or `done`.
    pub state: String,
    /// What a blocked window waits on.
    pub kind: Option<String>,
    /// The words of its status line.
    pub msg: Option<String>,
}

impl Mark {
    /// Whether the window waits for an answer to a permission.
    pub fn asks_permission(&self) -> bool {
        self.state == "blocked" && self.kind.as_deref() == Some("permission")
    }

    /// As words: `blocked on permission (Running command)`.
    pub fn said(&self) -> String {
        let on = self
            .kind
            .as_deref()
            .map_or_else(String::new, |kind| format!(" on {kind}"));
        let words = self
            .msg
            .as_deref()
            .map_or_else(String::new, |msg| format!(" ({msg})"));
        format!("{}{on}{words}", self.state)
    }
}

/// The mark whose text (between its start and its end) is `body`: `state=idle:app=devin`.
/// What ends a mark is the escape and the backslash of its terminator: the escape is the
/// last character of the body, and no part of the mark.
fn parse(body: &str) -> Option<Mark> {
    let mut mark = Mark {
        state: String::new(),
        kind: None,
        msg: None,
    };
    for field in body.trim_end_matches(char::is_control).split(':') {
        let (name, value) = field.split_once('=')?;
        match name {
            "state" => mark.state = value.to_owned(),
            "kind" => mark.kind = Some(value.to_owned()),
            "msg" => mark.msg = Some(unbase64(value)?),
            _ => {}
        }
    }
    (!mark.state.is_empty()).then_some(mark)
}

/// `text` as standard base64 says it, where it is words.
fn unbase64(text: &str) -> Option<String> {
    let (mut bits, mut have) = (0u32, 0u32);
    let mut bytes = Vec::new();
    for byte in text.bytes().take_while(|byte| *byte != b'=') {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        have += 6;
        if have >= 8 {
            have -= 8;
            bytes.push(u8::try_from((bits >> have) & 0xff).ok()?);
            bits &= (1 << have) - 1;
        }
    }
    String::from_utf8(bytes).ok()
}

/// The marks a window printed, as they changed: what it was doing, and since when.
#[derive(Debug, Default)]
pub struct Marks {
    /// What was printed after the last mark read whole, which may be the start of one.
    pending: String,
    changes: Vec<(Duration, Mark)>,
}

impl Marks {
    /// Reads the marks in `printed`, new text of the screen, seen at `at`. A mark that
    /// says what the last said is no change.
    pub fn feed(&mut self, printed: &str, at: Duration) {
        self.pending.push_str(printed);
        while let Some(from) = self.pending.find(START) {
            let body = from + START.len();
            let Some(length) = self.pending[body..].find(END) else {
                // Not ended yet, or the start of no mark: what is before it goes.
                self.pending.drain(..from);
                if self.pending.len() > PENDING {
                    self.pending.clear();
                }
                return;
            };
            let mark = parse(&self.pending[body..body + length]);
            self.pending.drain(..body + length + END.len_utf8());
            if let Some(mark) = mark {
                if self.changes.last().is_none_or(|(_, last)| *last != mark) {
                    self.changes.push((at, mark));
                }
            }
        }
        // Nothing of a mark in what is left but its first characters, perhaps.
        let keep = START.len() - 1;
        let mut cut = self.pending.len().saturating_sub(keep);
        while !self.pending.is_char_boundary(cut) {
            cut += 1;
        }
        self.pending.drain(..cut);
    }

    /// What the window was last said to be doing, and since when.
    pub fn now(&self) -> Option<&(Duration, Mark)> {
        self.changes.last()
    }

    /// How many changes there are: a mark to ask what came after ([`Marks::since`]).
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// The changes after the first `from`, as a line, each with its time; the first
    /// few and the last few where there are many.
    pub fn since(&self, from: usize) -> String {
        const SHOWN: usize = 12;
        let said: Vec<String> = self.changes[from.min(self.changes.len())..]
            .iter()
            .map(|(at, mark)| format!("{} {}", mark.said(), plus(*at)))
            .collect();
        if said.is_empty() {
            return "no mark of Devin's".to_owned();
        }
        if said.len() <= SHOWN {
            return said.join(", ");
        }
        let (head, tail) = (&said[..SHOWN / 2], &said[said.len() - SHOWN / 2..]);
        format!(
            "{}, … {} more …, {}",
            head.join(", "),
            said.len() - SHOWN,
            tail.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    #[test]
    fn the_words_of_a_status_line_are_read_from_base64() {
        assert_eq!(unbase64("VGhpbmtpbmc=").as_deref(), Some("Thinking"));
        assert_eq!(
            unbase64("UnVubmluZyBjb21tYW5k").as_deref(),
            Some("Running command")
        );
        assert_eq!(
            unbase64("RGV2aW4gZmluaXNoZWQ=").as_deref(),
            Some("Devin finished")
        );
        assert_eq!(unbase64("").as_deref(), Some(""));
        assert_eq!(unbase64("not base64!"), None);
    }

    #[test]
    fn a_mark_says_the_state_what_a_blocked_window_waits_on_and_the_words_of_its_status() {
        let idle = parse("state=idle:app=devin").unwrap();
        assert_eq!(idle.said(), "idle");
        assert!(!idle.asks_permission());
        let asked =
            parse("state=blocked:kind=permission:app=devin:msg=UnVubmluZyBjb21tYW5k").unwrap();
        assert!(asked.asks_permission());
        assert_eq!(asked.said(), "blocked on permission (Running command)");
        let working = parse("state=working:app=devin:msg=VGhpbmtpbmc=").unwrap();
        assert_eq!(working.said(), "working (Thinking)");
        // No state, or a field that is none: no mark.
        assert_eq!(parse("app=devin"), None);
        assert_eq!(parse("state"), None);
    }

    #[test]
    fn the_marks_a_screen_prints_are_read_as_they_change_and_a_mark_split_between_looks_is_read_whole(
    ) {
        let mut marks = Marks::default();
        marks.feed("Context: 19k ]7501;state=idle:app=devin\\ more", at(1));
        // The same state again is no change.
        marks.feed("]7501;state=idle:app=devin\\", at(2));
        marks.feed("text ]7501;state=work", at(3));
        marks.feed("ing:app=devin:msg=VGhpbmtpbmc=\\ ", at(4));
        marks.feed(
            "]7501;state=blocked:kind=permission:app=devin:msg=UnVubmluZyBjb21tYW5k\\",
            at(9),
        );
        assert_eq!(marks.len(), 3);
        assert_eq!(
            marks.since(0),
            "idle +1.0 s, working (Thinking) +4.0 s, blocked on permission (Running command) +9.0 s"
        );
        assert_eq!(
            marks.since(2),
            "blocked on permission (Running command) +9.0 s"
        );
        assert_eq!(marks.since(9), "no mark of Devin's");
        let (since, now) = marks.now().unwrap();
        assert_eq!((*since, now.asks_permission()), (at(9), true));
    }

    #[test]
    fn a_mark_that_ends_with_the_escape_and_the_backslash_of_its_terminator_is_read_whole() {
        // A terminal control sequence is begun by an escape and ended by one and a
        // backslash: the escape that ends the mark is the last character of its words.
        let mut marks = Marks::default();
        marks.feed(
            "\u{1b}]7501;state=blocked:kind=permission:app=devin:msg=UnVubmluZyBjb21tYW5k\u{1b}\\",
            at(2),
        );
        marks.feed("\u{1b}]7501;state=idle:app=devin\u{1b}\\", at(3));
        assert_eq!(
            marks.since(0),
            "blocked on permission (Running command) +2.0 s, idle +3.0 s"
        );
        assert_eq!(marks.len(), 2);
    }

    #[test]
    fn a_start_that_never_ends_does_not_hold_the_rest_of_the_screen_back() {
        let mut marks = Marks::default();
        marks.feed(&format!("]7501;{}", "x".repeat(500)), at(1));
        assert_eq!(marks.len(), 0);
        marks.feed("then ]7501;state=done:app=devin\\", at(2));
        assert_eq!(marks.since(0), "done +2.0 s");
        // Characters that are not one byte are no trouble to what is kept.
        marks.feed("é]75", at(3));
        marks.feed("01;state=idle:app=devin\\", at(4));
        assert_eq!(marks.since(1), "idle +4.0 s");
    }

    #[test]
    fn many_changes_are_told_by_the_first_few_and_the_last_few() {
        let mut marks = Marks::default();
        for at_second in 0..20u64 {
            let state = if at_second % 2 == 0 { "idle" } else { "done" };
            marks.feed(&format!("]7501;state={state}:app=devin\\"), at(at_second));
        }
        let said = marks.since(0);
        assert!(said.starts_with("idle +0.0 s, done +1.0 s"), "{said}");
        assert!(said.contains("… 8 more …"), "{said}");
        assert!(said.ends_with("done +19.0 s"), "{said}");
    }
}
