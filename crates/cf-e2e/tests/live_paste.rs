//! Live: does a harness's window send what ConsensFlow pastes into it? It is
//! opt-in (`cargo xtask live paste`, `npm run live:paste`): it spends the
//! machine's quota on a window of each harness named, and needs each one
//! installed and logged in, so it is ignored unless asked for and is no part of
//! `check` or of CI.
//!
//! Each harness opens in a window of the pane host of a rig, started the way
//! the app starts it ([`cf_e2e::live::window`]), in a folder of its own under the
//! user's home, on its cheap model ([`cf_e2e::live::agents`]). Three messages go
//! in the way the daemon sends one (`pane.write_paste`: a bracketed paste, then
//! Enter once the window has drawn it): one line, a few lines shaped like a
//! ConsensFlow message, and one of about 3,800 characters, which Devin and Claude
//! fold into a placeholder. Each asks for a sum the message does not hold, so
//! its answer on the screen means the window sent it; and the harness's own
//! record must hold the message whole, its marks included (— “” → € …), as the
//! app reads it there. A message the record holds whole was sent even where the
//! model answered it otherwise than with the sum (Claude on Sonnet may ask what
//! to do with a result instead). Each goes in as the app gives it to that window: Devin
//! on Windows gets its marks in ASCII. Nothing presses Enter a second time: a
//! message left waiting in the input is a failure, the one Devin showed on
//! Windows.
//!
//! ```text
//! cargo xtask live paste                          Devin
//! cargo xtask live paste claude devin             both
//! cargo xtask live paste --harness claude --long 8000 --long 16000
//!                                                 long messages of those lengths, one of each
//! ```
//!
//! Harnesses: the two the app pastes into, claude and devin (Codex, Pi and
//! OpenCode take their messages through their own queues). They run one after
//! another; the run fails when any message was not sent.
//!
//! This is the shape of a live test of one window: read what was asked for
//! ([`live::list`]), find the harnesses on the machine and say where each is
//! ([`window::pasted_installs`], [`live::Log::found`]), start the rig they run in
//! ([`live::rig`]), open a window for each ([`window::Open`]), say what each check
//! came to ([`report::Check`]), and end the run by saying them all and failing it
//! with the checks that did not hold ([`report::conclude`]).

use std::time::Duration;

use cf_e2e::live::ask::{self, Item};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{self, Open, ANSWER};
use cf_e2e::live::{self, Installed, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows:
/// no window of this test runs it (each is opened on the real harness's own
/// path), but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The length of the long message when no `--long` says another, in characters.
const LONG: usize = 3700;

/// A message the daemon pastes, and the sum its answer is.
struct Case {
    name: String,
    sum: (u32, u32),
    body: Box<dyn Fn(&str) -> String>,
}

/// What each message asks for: a sum the message does not hold.
fn ask(sum: (u32, u32)) -> String {
    format!(
        "Reply with only the sum of {} and {}, in digits, and run no tools.",
        sum.0, sum.1
    )
}

/// The messages: one line, a few lines shaped like a ConsensFlow message, and a
/// long one of each length in `lengths`.
fn cases(lengths: &[usize]) -> Vec<Case> {
    let mut cases = vec![
        Case {
            name: "one line".to_owned(),
            sum: (1234, 4321),
            body: Box::new(str::to_owned),
        },
        Case {
            name: "a ConsensFlow message".to_owned(),
            sum: (2468, 1357),
            body: Box::new(|line| {
                [
                    "[ConsensFlow m-1 · T-1 · result from @worker]",
                    "The page is done — “Contact” moved → the footer, €0 spent…",
                    "",
                    "Files changed: index.html.",
                    line,
                ]
                .join("\n")
            }),
        },
    ];
    for (at, length) in (0_u32..).zip(lengths) {
        let length = *length;
        cases.push(Case {
            name: format!("a long message ({length} characters)"),
            sum: (3141 + at, 2718 + at),
            body: Box::new(move |line| long_message(length, line)),
        });
    }
    cases
}

/// A message of at least `length` characters, as a result of a worker's is,
/// ending in `line`.
fn long_message(length: usize, line: &str) -> String {
    let mut lines = vec!["[ConsensFlow m-2 · T-1 · result from @worker]".to_owned()];
    let mut section = 1;
    while lines.join("\n").chars().count() < length {
        lines.push(format!(
            "{section}. Section {section} — headings, “links” → footer: all match…"
        ));
        section += 1;
    }
    lines.push(line.to_owned());
    lines.join("\n")
}

/// The lengths of the long messages `--long` asked for, or [`LONG`] for one.
fn lengths(asked: &[String]) -> Result<Vec<usize>> {
    if asked.is_empty() {
        return Ok(vec![LONG]);
    }
    asked
        .iter()
        .map(|length| {
            length
                .parse::<usize>()
                .ok()
                .filter(|length| *length > 0)
                .ok_or_else(|| Error::Daemon("--long takes a length in characters".to_owned()))
        })
        .collect()
}

/// The first 160 characters of `text`, those that are no printable ASCII as
/// `\u` and their code: what a record that holds the marks otherwise looks like.
fn escaped(text: &str) -> String {
    text.chars()
        .take(160)
        .map(|at| {
            if (' '..='~').contains(&at) {
                at.to_string()
            } else {
                format!("\\u{:04x}", at as u32)
            }
        })
        .collect()
}

/// What the harness's record holds of the user's turns, oldest first.
fn users(window: &window::Window) -> Vec<Item> {
    window
        .recorded()
        .into_iter()
        .filter(|item| item.role == "user")
        .collect()
}

/// What a message pasted into a window came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Came {
    /// Its answer showed and its record holds it whole.
    Answered,
    /// Its record holds it whole, but the model answered it otherwise than with
    /// the sum: it was sent all the same.
    Recorded,
    /// Its answer showed, but its record holds it otherwise than it was pasted.
    Otherwise,
    /// No answer showed and no record holds it: it was not sent, or it waits in
    /// the input.
    NotSent,
}

impl Came {
    /// What a message came to, given whether its answer `shown` and whether its
    /// record `kept` it whole.
    fn to(shown: bool, kept: bool) -> Self {
        match (shown, kept) {
            (true, true) => Self::Answered,
            (false, true) => Self::Recorded,
            (true, false) => Self::Otherwise,
            (false, false) => Self::NotSent,
        }
    }

    /// Whether the window sent the message and the app can read it whole.
    fn holds(self) -> bool {
        matches!(self, Self::Answered | Self::Recorded)
    }
}

/// Sends each message to the window of `install`, one after another, and says what
/// each came to: it was sent (its answer is on the screen or in the record) and
/// the record holds it whole.
fn checks_of(rig: &Rig, install: &Installed, cases: &[Case], log: &Log) -> Result<Vec<Check>> {
    let name = install.harness().name();
    let window =
        Open::new(rig, install, "paste", &format!("paste-{name}")).open_telling(log, name)?;
    if let Err(screen) = window.ready() {
        return Ok(vec![Check::failed(name, "opens", screen)]);
    }
    let mut checks = Vec::new();
    for case in cases {
        let answer = (case.sum.0 + case.sum.1).to_string();
        let body = window.given(&(case.body)(&ask(case.sum)))?;
        log.say(format!("{name}: sends {}", case.name));
        let sent = window.send(&body, &answer)?;
        // Its record holds what was pasted, whole: where the app looks for it. It
        // is looked for even when no answer showed: a model may answer otherwise.
        let whole = body.replace("\r\n", "\n").trim().to_owned();
        let mut seen = Vec::new();
        let kept = live::until(Duration::from_secs(10), Duration::from_millis(500), || {
            seen = users(&window);
            seen.iter()
                .any(|item| item.text.replace("\r\n", "\n").contains(&whole))
                .then_some(())
        })
        .is_some();
        let came = Came::to(sent.shown, kept);
        let detail = match came {
            Came::Answered => format!("sent and recorded whole, answered in {} s", sent.seconds),
            Came::Recorded => format!(
                "sent and recorded whole; answered with no sum: {}",
                window.tail()
            ),
            Came::Otherwise => {
                let last = seen.last().map_or("(no record found)", |item| &item.text);
                format!("sent, but recorded otherwise: {}", escaped(last))
            }
            Came::NotSent => format!("NOT SENT ({}): {}", sent.written, window.tail()),
        };
        checks.push(Check::new(
            name,
            &case.name,
            sent.written["ok"] == true && came.holds(),
            detail,
        ));
        if came == Came::NotSent {
            break;
        }
        window.still(ANSWER);
    }
    Ok(checks)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of each harness named; cargo xtask live paste (npm run live:paste)"]
fn a_window_sends_every_message_the_daemon_pastes_into_it() -> Result {
    let log = Log::begin("live paste")?;
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let cases = cases(&lengths(&live::list("long"))?);
    let installs = window::pasted_installs(&log)?;
    let mut rig = Rig::start(live::rig(&installs, FAKE_AGENT))?;
    if live::keep_requested() {
        log.say(format!(
            "{} is set: the home stays at {}",
            live::KEEP,
            rig.keep_home().display()
        ));
    }
    let mut checks = Vec::new();
    for install in &installs {
        checks.extend(checks_of(&rig, install, &cases, &log)?);
    }
    rig.close()?;
    report::conclude(&log, "what each check came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_the_record_holds_whole_was_sent_whether_the_model_answered_the_sum_or_not() {
        assert_eq!(Came::to(true, true), Came::Answered);
        assert_eq!(Came::to(false, true), Came::Recorded);
        assert_eq!(Came::to(true, false), Came::Otherwise);
        assert_eq!(Came::to(false, false), Came::NotSent);
        let holding: Vec<bool> = [
            Came::Answered,
            Came::Recorded,
            Came::Otherwise,
            Came::NotSent,
        ]
        .into_iter()
        .map(Came::holds)
        .collect();
        assert_eq!(holding, [true, true, false, false]);
    }

    #[test]
    fn each_message_asks_for_a_sum_it_does_not_hold() {
        assert_eq!(
            ask((1234, 4321)),
            "Reply with only the sum of 1234 and 4321, in digits, and run no tools."
        );
        for case in cases(&[LONG, 8000]) {
            let message = (case.body)(&ask(case.sum));
            let answer = (case.sum.0 + case.sum.1).to_string();
            assert!(!message.contains(&answer), "{}: {message}", case.name);
        }
    }

    #[test]
    fn there_is_a_line_a_consensflow_message_and_a_long_message_for_each_length_asked() {
        let named: Vec<(String, (u32, u32))> = cases(&[3700, 16000])
            .iter()
            .map(|case| (case.name.clone(), case.sum))
            .collect();
        assert_eq!(
            named,
            [
                ("one line".to_owned(), (1234, 4321)),
                ("a ConsensFlow message".to_owned(), (2468, 1357)),
                ("a long message (3700 characters)".to_owned(), (3141, 2718)),
                ("a long message (16000 characters)".to_owned(), (3142, 2719)),
            ]
        );
        // No two ask for the same answer, which a window's screen would still show.
        let answers: Vec<u32> = cases(&[3700, 16000])
            .iter()
            .map(|case| case.sum.0 + case.sum.1)
            .collect();
        for (at, answer) in answers.iter().enumerate() {
            assert!(!answers[at + 1..].contains(answer), "{answers:?}");
        }
    }

    #[test]
    fn the_message_shaped_like_a_consensflow_one_has_the_marks_the_record_must_keep() {
        let case = &cases(&[])[1];
        let message = (case.body)("THE ASK");
        assert_eq!(
            message,
            "[ConsensFlow m-1 · T-1 · result from @worker]\n\
             The page is done — “Contact” moved → the footer, €0 spent…\n\
             \n\
             Files changed: index.html.\n\
             THE ASK"
        );
    }

    #[test]
    fn a_long_message_is_as_long_as_asked_in_characters_and_ends_in_the_ask() {
        for length in [200, 3700, 8000] {
            let message = long_message(length, "THE ASK");
            let count = message.chars().count();
            // It stops at the section that reaches the length, and the ask follows.
            assert!(count >= length && count < length + 100, "{length}: {count}");
            assert!(message.ends_with("\nTHE ASK"));
            assert!(message
                .starts_with("[ConsensFlow m-2 · T-1 · result from @worker]\n1. Section 1 — "));
        }
    }

    #[test]
    fn the_lengths_asked_for_are_whole_numbers_of_characters_and_one_long_message_is_the_default() {
        let given = |words: &[&str]| {
            words
                .iter()
                .map(|word| (*word).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(lengths(&[]).unwrap(), [LONG]);
        assert_eq!(lengths(&given(&["8000", "16000"])).unwrap(), [8000, 16000]);
        for wrong in ["0", "-5", "many", "3.5", ""] {
            let failed = lengths(&given(&[wrong])).unwrap_err().to_string();
            assert_eq!(failed, "--long takes a length in characters", "{wrong:?}");
        }
    }

    #[test]
    fn a_record_that_holds_the_marks_otherwise_shows_them_as_their_codes() {
        assert_eq!(
            escaped("plain, printable ~ text"),
            "plain, printable ~ text"
        );
        assert_eq!(
            escaped("a — “b” → €"),
            "a \\u2014 \\u201cb\\u201d \\u2192 \\u20ac"
        );
        assert_eq!(escaped("line\nbreak"), "line\\u000abreak");
        assert_eq!(escaped(&"x".repeat(300)).len(), 160);
    }
}
