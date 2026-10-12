//! Live: does a harness's interrupt, pressed at a window that is already idle,
//! leave the next message alone? It is opt-in (`cargo xtask live interrupt`,
//! `npm run live:interrupt`): it spends the machine's quota on a window of each
//! harness named, and needs each one installed and logged in, so it is ignored
//! unless asked for and is no part of `check` or of CI.
//!
//! The daemon interrupts only a window at work, but a turn can end in the moment
//! between its look and its keys. Two Escapes at an idle Devin open its rewind,
//! and the next message's Enter confirmed it, cutting the conversation back
//! (poker-lab, 2026-10-03); Devin's interrupt now ends with one Escape more,
//! which closes it. Each window here answers a first message, takes its
//! harness's interrupt as the daemon presses it ([`Window::interrupt`]: the keys
//! its adapter says), then a second message: that one must be answered, and the
//! first must still be in the harness's record.
//!
//! ```text
//! cargo xtask live interrupt                      Devin
//! cargo xtask live interrupt claude devin         both
//! ```
//!
//! Harnesses: the two the app pastes into, claude and devin (Codex, Pi and
//! OpenCode take their messages through their own queues). They run one after
//! another; the run fails when a window lost its conversation or the message
//! after the interrupt.
//!
//! What the daemon's interrupt does to a window at work (Escape into a long
//! command, at a question hook, a stop that is ignored) is `npm run live:stops`.

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::ask::{self, Item};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{self, Open, Sent, Window, ANSWER};
use cf_e2e::live::{self, Log};
use cf_e2e::rig::Rig;
use cf_e2e::Result;

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows:
/// no window of this test runs it (each is opened on the real harness's own
/// path), but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the check is called in the report.
const WHAT: &str = "interrupt at an idle window";

/// How long a window is left to settle after the interrupt, before the next
/// message goes in.
const SETTLE: Duration = Duration::from_secs(2);

/// What the first message and the second ask for.
const FIRST: (u32, u32) = (1111, 2222);
const SECOND: (u32, u32) = (4444, 1111);

/// A message and the answer that shows the window took it.
struct Ask {
    message: String,
    answer: String,
}

impl Ask {
    /// The message `what` (`First`, `Second`) that asks for a sum its words do not
    /// hold, so that its answer on the screen means the window took it.
    fn sum(what: &str, (a, b): (u32, u32)) -> Self {
        Self {
            message: format!(
                "{what}: reply with only the sum of {a} and {b}, in digits, and run no tools."
            ),
            answer: (a + b).to_string(),
        }
    }
}

/// The two messages of a window: the first, then the one after the interrupt.
fn asks() -> [Ask; 2] {
    [Ask::sum("First", FIRST), Ask::sum("Second", SECOND)]
}

/// What the message after the interrupt came to: it was answered, and the record
/// still holds the first message among the user's turns. A record that has lost
/// the first is a conversation cut back, whatever answered the second.
fn outcome(who: &str, second: &Sent, record: &[Item], screen: &str) -> Check {
    let whole = record
        .iter()
        .any(|item| item.role == "user" && item.text.contains("First:"));
    let detail = if !second.shown {
        format!("the message after it was not answered: {screen}")
    } else if whole {
        format!(
            "the message after it answered in {} s, the conversation whole",
            second.seconds
        )
    } else {
        "the message after it was answered, but the first is gone: the conversation was cut back"
            .to_owned()
    };
    Check::new(who, WHAT, second.shown && whole, detail)
}

/// Has the open `window` answer a first message, interrupts it as the daemon
/// does at a window at rest, and says what the message after came to.
fn check_of(window: &Window, asks: &[Ask; 2]) -> Result<Check> {
    let harness = window.harness();
    let name = harness.name();
    if let Err(screen) = window.ready() {
        return Ok(Check::failed(name, WHAT, format!("did not open: {screen}")));
    }
    let [first, second] = asks;
    let sent = window.send(&window.given(&first.message)?, &first.answer)?;
    if !sent.shown {
        let detail = format!("the first message was not answered: {}", window.tail());
        return Ok(Check::failed(name, WHAT, detail));
    }
    window.still(ANSWER);
    window.interrupt(&ask::interrupt(harness.kind())?)?;
    sleep(SETTLE);
    let sent = window.send(&window.given(&second.message)?, &second.answer)?;
    Ok(outcome(name, &sent, &window.recorded(), &window.tail()))
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of each harness named; cargo xtask live interrupt (npm run live:interrupt)"]
fn an_interrupt_at_an_idle_window_leaves_the_conversation_and_the_next_message_alone() -> Result {
    let log = Log::new("live interrupt");
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let installs = window::pasted_installs(&log)?;
    let rig = Rig::start(live::rig(&installs, FAKE_AGENT))?;
    let asks = asks();
    let mut checks = Vec::new();
    for install in &installs {
        let name = install.harness().name();
        let window = Open::new(&rig, install, "interrupt", &format!("interrupt-{name}"))
            .open_telling(&log, name)?;
        checks.push(check_of(&window, &asks)?);
    }
    rig.close()?;
    report::conclude(&log, "what each check came to", &checks)
}

#[cfg(test)]
mod tests {
    use cf_e2e::live::{Harness, Installed};
    use cf_e2e::rig::Config;
    use serde_json::json;

    use super::*;

    fn item(role: &str, text: &str) -> Item {
        Item {
            id: format!("{role}-1"),
            role: role.to_owned(),
            text: text.to_owned(),
            complete: true,
        }
    }

    fn sent(shown: bool) -> Sent {
        Sent {
            written: json!({ "ok": true }),
            shown,
            seconds: "7.2".to_owned(),
        }
    }

    #[test]
    fn each_message_asks_for_a_sum_it_does_not_hold() {
        let [first, second] = asks();
        assert_eq!(
            first.message,
            "First: reply with only the sum of 1111 and 2222, in digits, and run no tools."
        );
        assert_eq!(
            second.message,
            "Second: reply with only the sum of 4444 and 1111, in digits, and run no tools."
        );
        assert_eq!(
            (first.answer.as_str(), second.answer.as_str()),
            ("3333", "5555")
        );
        for ask in [&first, &second] {
            assert!(!ask.message.contains(&ask.answer), "{}", ask.message);
        }
    }

    #[test]
    fn the_two_answers_differ_so_the_first_answer_left_on_the_screen_is_not_the_second() {
        let [first, second] = asks();
        assert_ne!(first.answer, second.answer);
        // The second message's sum is not hidden in the first's words either.
        assert!(!first.message.contains(&second.answer));
        assert!(!second.message.contains(&first.answer));
    }

    #[test]
    fn a_conversation_whole_is_a_window_that_answered_after_it_with_the_first_message_still_recorded(
    ) {
        let record = [
            item("user", "First: reply with only the sum of 1111 and 2222"),
            item("assistant", "3333"),
            item("user", "Second: reply with only the sum of 4444 and 1111"),
            item("assistant", "5555"),
        ];
        let check = outcome("devin", &sent(true), &record, "screen");
        assert_eq!(
            check,
            Check::passed(
                "devin",
                "interrupt at an idle window",
                "the message after it answered in 7.2 s, the conversation whole"
            )
        );
    }

    #[test]
    fn a_conversation_cut_back_fails_although_the_message_after_it_was_answered() {
        // Devin's rewind, confirmed by the next message's Enter: the first message is gone.
        let record = [
            item("user", "Second: reply with only the sum of 4444 and 1111"),
            item("assistant", "5555"),
        ];
        let check = outcome("devin", &sent(true), &record, "screen");
        assert!(!check.ok);
        assert_eq!(
            check.detail,
            "the message after it was answered, but the first is gone: the conversation was cut back"
        );
    }

    #[test]
    fn a_message_after_the_interrupt_that_nothing_answered_fails_with_the_screen() {
        let record = [item(
            "user",
            "First: reply with only the sum of 1111 and 2222",
        )];
        let check = outcome("claude", &sent(false), &record, "a ⏎ b");
        assert!(!check.ok);
        assert_eq!(check.detail, "the message after it was not answered: a ⏎ b");
    }

    #[test]
    fn only_a_turn_of_the_user_counts_as_the_first_message_being_kept() {
        // A window that echoed the words in its own answer has not kept the user's turn.
        let record = [
            item("assistant", "First: I will add them"),
            item("user", "Second: reply with only the sum of 4444 and 1111"),
        ];
        assert!(!outcome("claude", &sent(true), &record, "").ok);
        assert!(!outcome("claude", &sent(true), &[], "").ok);
    }

    /// What the stand-in agent answers, for the two messages: it answers a
    /// message that says `Reply with exactly: <word>` with the word.
    fn stand_in_asks() -> [Ask; 2] {
        [("First", "3333"), ("Second", "5555")].map(|(what, answer)| Ask {
            message: format!("{what}: Reply with exactly: {answer}"),
            answer: answer.to_owned(),
        })
    }

    #[test]
    fn the_whole_check_runs_against_a_window_that_answers_and_keeps_its_record() -> Result {
        // The stand-in is the Claude Code of this rig (it asks nothing of a folder, and
        // writes Claude's record where Claude does), its home the rig's own.
        let rig = Rig::start(Config::new(FAKE_AGENT))?;
        let home = rig.var("HOME").unwrap_or_default().to_owned();
        let install = Installed::at(Harness::Claude, FAKE_AGENT, &home);
        let window = Open::new(&rig, &install, "interrupt", "interrupt-claude")
            .without_trust()
            .open()?;
        let check = check_of(&window, &stand_in_asks())?;
        assert_eq!(check.who, "claude");
        assert!(check.ok, "{check:?}");
        assert!(
            check.detail.ends_with("the conversation whole"),
            "{check:?}"
        );
        drop(window);
        rig.close()
    }
}
