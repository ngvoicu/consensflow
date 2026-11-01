//! Live: a prompt the human pastes into a window, then sends with Enter, goes, on that Enter
//! (T-972; the issue page `codex-windows-ignores-enter-after-paste`). It is opt-in (`cargo
//! xtask live paste-enter`): it spends the machine's quota on a few short turns of each harness
//! named, and needs each one installed and logged in, so it is ignored unless asked for and is
//! no part of `check` or of CI.
//!
//! Seen on zeewin (2026-10-03): the evals' runner typed the owner's prompt into a Codex chief as
//! a bracketed paste with its Enter, and every Codex chief on Windows needed a second Enter; in
//! two runs even the later ones did not send it, and the screen ended with the prompt still in
//! Codex's input (`[Pasted Content 7726 chars]`). The daemon never pastes into Codex (its
//! messages go through Codex's own queue), so this is what a human's paste meets in the app.
//!
//! The harness named is the chief of a project on the native daemon of a rig, a window the
//! human types into as the app's page does: the bytes its terminal sends for a paste (each line
//! break a carriage return, between the bracketed paste's markers), as the human's keys
//! (`pane.typed`, which the pane host takes as the page's). Each case is a paste once the chief
//! is idle, in one of two shapes:
//!
//! - **at once**: the paste and its Enter in one write, as the evals' runner types it;
//! - **then Enter**: the paste, and Enter a second later, as a person presses it.
//!
//! and three lengths: a line, a few lines, and one of about 7,700 characters, which Codex folds
//! into `[Pasted Content … chars]`. A case's paste went when the board shows the chief at work
//! after it (the daemon's own `window.activity`); where it does not within five seconds, the
//! human presses Enter again, up to three times, as the evals' runner does. Each asks for one
//! word back, so the turn is short.
//!
//! ```text
//! cargo xtask live paste-enter                     Codex
//! cargo xtask live paste-enter codex devin claude
//! cargo xtask windows --host zeewin2 -- cargo xtask live paste-enter
//! ```
//!
//! The run fails when a paste needed an Enter more than its own, or never went: it says, for
//! each, how many it took and what the window last drew, and when the pane host wrote its Enter.
//!
//! **The cause (T-1048):** on Windows the console hands a paste on to the program as key
//! presses, and Codex takes an Enter that comes while they are still coming, or within 120 ms
//! of them, for a line break in the paste (its `paste_burst`). On Windows the pane host now
//! holds the human's keys after their paste until the window has drawn it and gone quiet, as a
//! delivery's Enter waits (`cf_panes::arbiter`'s `HOLDS_HUMAN_PASTES`).

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{self, find, named, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::trace::Trace;
use cf_e2e::live::{self, between, Harness, Log, Who};
use cf_e2e::Result;

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The harness run where none is named: the one that showed it.
const DEFAULT: [Harness; 1] = [Harness::Codex];

/// The key Enter sends, and how a bracketed paste begins and ends.
const ENTER: &[u8] = b"\r";
const PASTE_BEGINS: &[u8] = b"\x1b[200~";
const PASTE_ENDS: &[u8] = b"\x1b[201~";

/// How long a person takes between the paste and its Enter.
const BEAT: Duration = Duration::from_secs(1);

/// How long a window has to go to work on an Enter before the human presses it again, and
/// how many more times they do.
const TAKES: Duration = Duration::from_secs(5);
const AGAIN: usize = 3;

/// How long the chief has to be idle again after a case.
const IDLE: Duration = Duration::from_secs(180);

/// How a paste and its Enter reach the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// In one write, as the evals' runner types them.
    AtOnce,
    /// The paste, then Enter a beat later, as a person presses it.
    ThenEnter,
}

impl Shape {
    pub const ALL: [Shape; 2] = [Shape::AtOnce, Shape::ThenEnter];

    pub const fn name(self) -> &'static str {
        match self {
            Shape::AtOnce => "at once",
            Shape::ThenEnter => "then Enter",
        }
    }
}

/// The writes a paste of `text` is, in `shape`: one, or the paste and then Enter. Each line
/// break is a carriage return, as the page's terminal sends a paste (xterm's
/// `prepareTextForTerminal`).
pub fn writes(shape: Shape, text: &str) -> Vec<Vec<u8>> {
    let typed = text.replace("\r\n", "\r").replace('\n', "\r");
    let paste = [PASTE_BEGINS, typed.as_bytes(), PASTE_ENDS].concat();
    match shape {
        Shape::AtOnce => vec![[paste.as_slice(), ENTER].concat()],
        Shape::ThenEnter => vec![paste, ENTER.to_vec()],
    }
}

/// The texts pasted, by length, each asking for the word `word` back: a line, a few lines, and
/// one of at least `long` characters.
pub fn texts(word: &str, long: usize) -> Vec<(&'static str, String)> {
    let ask = format!("Reply with only the word {word}, and run no tools.");
    let lines = format!(
        "[a note from the owner]\nThe page is done — “Contact” moved → the footer.\n\n{ask}"
    );
    let mut body = vec!["[a long note from the owner]".to_owned()];
    let mut section = 1;
    while body.join("\n").chars().count() < long {
        body.push(format!(
            "{section}. Section {section}: headings, links and the footer all match the brief."
        ));
        section += 1;
    }
    body.push(ask.clone());
    vec![
        ("a line", ask),
        ("a few lines", lines),
        ("a long paste", body.join("\n")),
    ]
}

/// Every case, in the order run: each shape, each length, each asking a word of its own
/// (`PASTED0` to `PASTED5`), so that what a window answered names the paste it answered.
pub fn cases() -> Vec<(Shape, &'static str, String)> {
    let mut cases = Vec::new();
    for shape in Shape::ALL {
        for at in 0..3 {
            let word = format!("PASTED{}", cases.len());
            let (length, text) = texts(&word, LONG).swap_remove(at);
            cases.push((shape, length, text));
        }
    }
    cases
}

/// The length of the long paste: the one Codex kept in its box on zeewin.
const LONG: usize = 7700;

/// What a paste came to: the Enters it took beyond its own (none: its own sent it), or none
/// of them sent it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Went {
    After(usize),
    Never,
}

impl Went {
    /// Whether the paste went on its own Enter.
    pub fn held(self) -> bool {
        self == Went::After(0)
    }

    pub fn line(self) -> String {
        match self {
            Went::After(0) => "it went on its own Enter".to_owned(),
            Went::After(more) => format!(
                "it went only after {more} more Enter{}",
                if more == 1 { "" } else { "s" }
            ),
            Went::Never => format!("it never went, {} Enters in all", AGAIN + 1),
        }
    }
}

/// The project: the harness is its chief, a window the human types into.
fn plan(harness: Harness) -> Plan {
    Plan::new(
        &format!("paste-enter-{}", harness.name()),
        members::member_as(harness, "chief"),
    )
}

/// The chief's pane, as the board has it.
fn chief_pane(members: &Members) -> Result<serde_json::Value> {
    Ok(members.lane(members.chief())?["pane"].clone())
}

/// One case: the paste written in its shape once the chief is idle, and Enter pressed again
/// while the chief does not go to work. Besides how it went, what the window showed when Enter
/// was first pressed again, and how long after the paste its Enter was written: the pane host
/// answers the human's keys once they are written, a held Enter too.
fn case(
    members: &Members,
    shape: Shape,
    text: &str,
    log: &Log,
) -> Result<(Went, String, Duration)> {
    find(
        log,
        "the chief to be idle",
        IDLE,
        Duration::from_secs(1),
        || Ok((members.lane(members.chief())?["activity"]["state"] == "idle").then_some(())),
    )?;
    let pane = chief_pane(members)?;
    let mark = Trace::read(members.rig()).len();
    let pasted = Instant::now();
    for (at, write) in writes(shape, text).iter().enumerate() {
        if at > 0 {
            sleep(BEAT);
        }
        members.rig().types(&pane, write)?;
    }
    let entered = pasted.elapsed();
    // What the window showed when the human first pressed Enter again: the paste still in its
    // box, or a turn that was only slow to start.
    let mut kept = String::new();
    for more in 0..=AGAIN {
        sleep(TAKES);
        if Trace::read(members.rig()).worked_after(members.chief(), mark) {
            return Ok((Went::After(more), kept, entered));
        }
        if more < AGAIN {
            if kept.is_empty() {
                kept = members.tail(pane["id"].as_str().unwrap_or_default());
            }
            log.say("the chief has not gone to work: the human presses Enter again");
            members.rig().types(&pane, ENTER)?;
        }
    }
    Ok((
        Went::Never,
        members.tail(pane["id"].as_str().unwrap_or_default()),
        entered,
    ))
}

/// What each case came to, in order: its shape, its length, and how it went. The rig's close
/// failing is the error, after every case ran.
type Ran = Vec<(Shape, &'static str, Result<(Went, String, Duration)>)>;

/// Every case run on `harness`'s chief, one after another.
fn pastes(harness: Harness, log: &Log) -> Result<Ran> {
    let members = Members::open(&plan(harness), Path::new(FAKE_AGENT), log)?;
    let mut ran = Vec::new();
    for (shape, length, text) in cases() {
        log.say(format!("{length} pasted {}", shape.name()));
        ran.push((shape, length, case(&members, shape, &text, log)));
    }
    members.close()?;
    Ok(ran)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on six short turns of each harness named; cargo xtask live paste-enter"]
fn a_prompt_the_human_pastes_and_sends_with_enter_goes_on_that_enter() -> Result {
    let log = Log::begin("live paste-enter")?;
    let harnesses = named(
        &live::list("harness"),
        &Harness::ALL,
        Harness::name,
        "harness",
        &DEFAULT,
    )?;
    let mut checks = Vec::new();
    for harness in harnesses {
        let log = log.scoped(harness.name());
        let who = Who::Harness(harness);
        let before = who.version();
        let ran = pastes(harness, &log);
        let version = between(before, who.version());
        match ran {
            Ok(ran) => {
                for (shape, length, came) in ran {
                    let (held, detail) = match came {
                        Ok((went, screen, entered)) => {
                            (went.held(), outcome(went, &screen, entered))
                        }
                        Err(failed) => (false, failed.to_string()),
                    };
                    checks.push(Check::new(
                        harness.name(),
                        &format!("{length}, {}", shape.name()),
                        held,
                        format!("{version}: {detail}"),
                    ));
                }
            }
            Err(failed) => checks.push(Check::failed(
                harness.name(),
                "the pastes",
                format!("{version}: {failed}"),
            )),
        }
    }
    report::conclude(&log, "how each paste went", &checks)
}

/// A case's line: how it went, when its Enter was written, and what the window showed when the
/// human first pressed Enter again, where they did.
fn outcome(went: Went, screen: &str, entered: Duration) -> String {
    let line = format!(
        "{} (its Enter written {} ms after the paste)",
        went.line(),
        entered.as_millis()
    );
    if screen.is_empty() {
        line
    } else {
        format!("{line}; its screen when Enter was pressed again: {screen}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_once_is_one_write_of_the_paste_and_enter_and_then_enter_is_two() {
        assert_eq!(
            writes(Shape::AtOnce, "hi"),
            [b"\x1b[200~hi\x1b[201~\r".to_vec()]
        );
        assert_eq!(
            writes(Shape::ThenEnter, "hi"),
            [b"\x1b[200~hi\x1b[201~".to_vec(), b"\r".to_vec()]
        );
    }

    /// A paste's line breaks are carriage returns, as the page's terminal sends them.
    #[test]
    fn a_pastes_line_breaks_are_written_as_the_pages_terminal_sends_them() {
        assert_eq!(
            writes(Shape::ThenEnter, "one\ntwo\r\nthree")[0],
            b"\x1b[200~one\rtwo\rthree\x1b[201~"
        );
    }

    #[test]
    fn a_cases_line_says_when_its_enter_was_written_and_the_screen_where_it_was_kept() {
        assert_eq!(
            outcome(Went::After(0), "", Duration::from_millis(1_234)),
            "it went on its own Enter (its Enter written 1234 ms after the paste)"
        );
        assert_eq!(
            outcome(
                Went::After(1),
                "[Pasted Content 7775 chars]",
                Duration::from_millis(9)
            ),
            "it went only after 1 more Enter (its Enter written 9 ms after the paste); \
             its screen when Enter was pressed again: [Pasted Content 7775 chars]"
        );
    }

    #[test]
    fn the_texts_are_a_line_a_few_lines_and_a_long_paste_each_asking_its_word() {
        let texts = texts("PASTED3", LONG);
        let lengths: Vec<&str> = texts.iter().map(|(length, _)| *length).collect();
        assert_eq!(lengths, ["a line", "a few lines", "a long paste"]);
        for (_, text) in &texts {
            assert!(text.ends_with("Reply with only the word PASTED3, and run no tools."));
        }
        assert_eq!(texts[0].1.lines().count(), 1);
        assert!(texts[1].1.lines().count() > 1 && texts[1].1.contains('—'));
        assert!(texts[2].1.chars().count() >= LONG);
    }

    #[test]
    fn each_case_is_a_shape_and_a_length_and_asks_a_word_of_its_own() {
        let cases = cases();
        assert_eq!(cases.len(), 6);
        for (at, (shape, length, text)) in cases.iter().enumerate() {
            assert_eq!(*shape, Shape::ALL[at / 3]);
            assert_eq!(*length, ["a line", "a few lines", "a long paste"][at % 3]);
            assert!(
                text.ends_with(&format!("word PASTED{at}, and run no tools.")),
                "{text}"
            );
        }
    }

    #[test]
    fn only_a_paste_that_went_on_its_own_enter_holds() {
        assert!(Went::After(0).held());
        assert!(!Went::After(1).held());
        assert!(!Went::Never.held());
        assert_eq!(Went::After(0).line(), "it went on its own Enter");
        assert_eq!(Went::After(2).line(), "it went only after 2 more Enters");
        assert_eq!(Went::Never.line(), "it never went, 4 Enters in all");
    }

    #[test]
    fn codex_is_run_where_none_is_named_and_the_chief_is_the_harness() {
        assert_eq!(
            named(&[], &Harness::ALL, Harness::name, "harness", &DEFAULT).unwrap(),
            [Harness::Codex]
        );
        let plan = plan(Harness::Codex);
        assert_eq!(plan.chief, "chief");
        assert_eq!(plan.roster.len(), 1);
        assert_eq!(plan.roster[0]["kind"], "codex");
    }
}
