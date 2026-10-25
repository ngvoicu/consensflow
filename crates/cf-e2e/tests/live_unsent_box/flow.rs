//! A strategy's run in one window: a first message pasted and never sent, the
//! strategy's keys, a second message pasted as the daemon pastes, which must
//! arrive alone and be answered, and, when it did, the same keys at the empty box
//! of a window at rest before a third. What it comes to is an [`Outcome`], and
//! what the run makes of that a check ([`check_of`]).

use std::sync::OnceLock;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live;
use cf_e2e::live::ask::Item;
use cf_e2e::live::report::Check;
use cf_e2e::live::window::{Window, ANSWER};
use cf_e2e::pattern;
use regex::Regex;
use serde_json::json;

use super::keys::Strategy;

/// How long a message may take to reach the record, and how often it is looked at.
const ARRIVES: Duration = Duration::from_secs(90);
const LOOK: Duration = Duration::from_millis(500);

/// How long the window is left to draw the first paste, and the keys after it.
const DRAWN: Duration = Duration::from_secs(2);

/// What a strategy that clears the box and is harmless at an empty one is said to do.
const CLEARS: &str = "clears the box and is harmless at an empty one";

/// A message shaped like the daemon's, which asks for one word back.
pub fn message(n: u32, word: &str) -> String {
    format!("[ConsensFlow m-{n} · T-1 · task from @chief]\nReply with exactly one line: {word}")
}

/// The bytes of a paste that is never sent: `text` in brackets, as the daemon's
/// paste is written, with no Enter after it.
pub fn unsent(text: &str) -> Vec<u8> {
    [b"\x1b[200~", text.as_bytes(), b"\x1b[201~"].concat()
}

/// Whether `text`, the record of a message given, is the message `n` alone: it
/// holds that message, and no message of another number is in it.
pub fn alone(text: Option<&str>, n: u32) -> bool {
    static MARK: OnceLock<Regex> = OnceLock::new();
    let Some(text) = text else {
        return false;
    };
    text.contains(&format!("m-{n} "))
        && pattern::once(&MARK, r"m-([0-9]+) ")
            .captures_iter(text)
            .all(|found| found[1].parse() == Ok(n))
}

/// What a pasted message came to: the record of it as given, and whether the
/// window answered it.
pub struct Read {
    pub text: Option<String>,
    pub answered: bool,
}

impl Read {
    /// What the record says of the message `n` and the answer `word`.
    pub fn of(items: &[Item], n: u32, word: &str) -> Self {
        let mark = format!("m-{n} ");
        let at = items
            .iter()
            .position(|item| item.role == "user" && item.text.contains(&mark));
        Self {
            text: at.map(|at| items[at].text.clone()),
            answered: at.is_some_and(|at| {
                items[at + 1..]
                    .iter()
                    .any(|item| item.role == "assistant" && item.text.contains(word))
            }),
        }
    }

    /// Whether it was taken: it arrived alone, and was answered.
    pub fn taken(&self, n: u32) -> bool {
        alone(self.text.as_deref(), n) && self.answered
    }

    /// The line a run says of it.
    pub fn line(&self, n: u32, what: &str) -> String {
        let quoted = json!(self
            .text
            .as_ref()
            .map(|text| text.chars().take(160).collect::<String>()));
        format!(
            "{} {what}: m-{n} {}, {}: {quoted}",
            if self.taken(n) { "ok  " } else { "FAIL" },
            if alone(self.text.as_deref(), n) {
                "arrived alone"
            } else if self.text.is_some() {
                "arrived with another"
            } else {
                "never arrived"
            },
            if self.answered {
                "answered"
            } else {
                "not answered"
            },
        )
    }
}

/// What a strategy came to.
pub struct Outcome {
    pub ok: bool,
    pub lines: Vec<String>,
}

impl Outcome {
    /// What it is said to do.
    pub fn said(&self) -> &'static str {
        if self.ok {
            CLEARS
        } else {
            "does not"
        }
    }
}

/// Pastes the message `n`, which asks for `word`, as the daemon pastes, and reads
/// what the record made of it once it answered, or the time ran out.
fn paste_and_read(window: &Window, n: u32, word: &str) -> cf_e2e::Result<Read> {
    window.paste(&window.given(&message(n, word))?)?;
    let read = live::until(ARRIVES, LOOK, || {
        let read = Read::of(&window.recorded(), n, word);
        read.answered.then_some(read)
    });
    Ok(read.unwrap_or_else(|| Read::of(&window.recorded(), n, word)))
}

/// Runs `strategy` in the open `window`.
pub fn flow(window: &Window, strategy: &Strategy) -> cf_e2e::Result<Outcome> {
    let mut lines = Vec::new();
    if let Err(screen) = window.ready() {
        lines.push(format!("FAIL it did not open: {screen}"));
        return Ok(Outcome { ok: false, lines });
    }
    window.press(&unsent(&window.given(&message(1, "FIRST"))?))?;
    sleep(DRAWN);
    lines.push(format!(
        "the first message pasted, never sent: {}",
        window.tail()
    ));
    window.press(strategy.keys)?;
    sleep(DRAWN);
    lines.push(format!("after {}: {}", strategy.name, window.tail()));
    let second = paste_and_read(window, 2, "SECOND")?;
    lines.push(second.line(2, "the next message"));
    let first = Read::of(&window.recorded(), 1, "FIRST");
    if first.text.is_some() {
        lines.push("FAIL the first message, never sent, reached the record".to_owned());
    }
    let cleared = second.taken(2) && first.text.is_none();
    let mut harmless = true;
    if cleared {
        window.still(ANSWER);
        window.press(strategy.keys)?;
        sleep(DRAWN);
        let third = paste_and_read(window, 3, "THIRD")?;
        harmless = third.taken(3) && !window.closed();
        lines.push(third.line(
            3,
            "the same keys at the empty box of a window at rest, then",
        ));
        if window.closed() {
            lines.push("FAIL the window ended".to_owned());
        }
    }
    Ok(Outcome {
        ok: cleared && harmless,
        lines,
    })
}

/// What a strategy's outcome is as a check of the run of `who`: a strategy that
/// is not required is reported as what it did and does not fail the run.
pub fn check_of(who: &str, strategy: &Strategy, outcome: &Outcome, required: bool) -> Check {
    let detail = if outcome.ok || required {
        outcome.said().to_owned()
    } else {
        format!("{}, and nothing here requires it to", outcome.said())
    };
    Check::new(who, strategy.name, outcome.ok || !required, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(role: &str, text: &str) -> Item {
        Item {
            id: "i".to_owned(),
            role: role.to_owned(),
            text: text.to_owned(),
            complete: true,
        }
    }

    #[test]
    fn a_paste_never_sent_is_the_daemons_brackets_with_no_enter() {
        assert_eq!(unsent("a\nb"), b"\x1b[200~a\nb\x1b[201~");
        assert_eq!(
            message(2, "SECOND"),
            "[ConsensFlow m-2 · T-1 · task from @chief]\nReply with exactly one line: SECOND"
        );
    }

    #[test]
    fn a_message_is_alone_when_no_other_number_is_in_its_record() {
        assert!(alone(Some(&message(2, "SECOND")), 2));
        let both = format!("{}\n{}", message(1, "FIRST"), message(2, "SECOND"));
        assert!(!alone(Some(&both), 2));
        assert!(!alone(Some(&message(12, "X")), 1));
        assert!(!alone(None, 2));
    }

    #[test]
    fn a_message_is_taken_when_it_arrived_alone_and_an_answer_came_after_it() {
        let items = [
            item("user", &message(2, "SECOND")),
            item("assistant", "SECOND"),
        ];
        let read = Read::of(&items, 2, "SECOND");
        assert!(read.taken(2));
        assert!(read
            .line(2, "the next message")
            .starts_with("ok   the next message: m-2 arrived alone, answered"));
        // An answer before it is no answer to it.
        let items = [
            item("assistant", "SECOND"),
            item("user", &message(2, "SECOND")),
        ];
        assert!(!Read::of(&items, 2, "SECOND").taken(2));
        // With the first message in front of it.
        let both = format!("{}\n{}", message(1, "FIRST"), message(2, "SECOND"));
        let items = [item("user", &both), item("assistant", "SECOND")];
        let read = Read::of(&items, 2, "SECOND");
        assert!(!read.taken(2));
        assert!(read.line(2, "x").contains("arrived with another, answered"));
        assert!(Read::of(&[], 2, "SECOND")
            .line(2, "x")
            .contains("never arrived, not answered: null"));
    }

    #[test]
    fn a_strategy_that_is_not_required_is_reported_for_what_it_did_and_does_not_fail_the_run() {
        let none = super::super::keys::strategy("none").unwrap();
        let failed = Outcome {
            ok: false,
            lines: vec![],
        };
        assert_eq!(
            check_of("devin", none, &failed, false),
            Check::passed("devin", "none", "does not, and nothing here requires it to")
        );
        assert_eq!(
            check_of("devin", none, &failed, true),
            Check::failed("devin", "none", "does not")
        );
    }
}
