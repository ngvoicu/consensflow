//! Claude Code's offers that wait in a window for a key, as the stand-in draws them mid-turn
//! (T-1183, read from Claude Code 2.1.317).
//!
//! A message with a line `LSP-OFFER <seconds>` makes the window offer to install an LSP plugin
//! as its turn works, as Claude Code does once the turn writes a file of an extension a
//! marketplace has a plugin for: its status says `waiting`, `waitingFor` "dialog open", and its
//! screen shows the dialog ("LSP plugin recommendation", "Would you like to install this LSP
//! plugin?", its `❯` on "Yes, install" and three choices more), for that long, as Claude Code
//! closes it by itself after 30 s; then the screen is cleared to its prompt, the status is busy
//! again, and the turn goes on. What is typed meanwhile answers nothing.
//!
//! A message with a line `SURVEY` makes the window draw Claude Code's session survey under its
//! prompt as its turn works ("How is Claude doing this session? (optional)", a digit a
//! choice), unless its environment says `CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY`, as Claude Code
//! draws none then; a survey drawn is written down (`surveys`, in its config folder).

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::process::own_var;

use super::records::Records;
use super::say;

/// The word of a line that asks for the LSP offer, and of one that asks for the survey.
const OFFER: &str = "LSP-OFFER ";
const SURVEY: &str = "SURVEY";

/// The file in the window's config folder of the surveys drawn.
pub const SURVEYS: &str = "surveys";

/// The offer, as Claude Code 2.1.317's dialog draws it.
const DIALOG: &str = "\r\n\
╭──────────────────────────────────────────────────────────────╮\r\n\
│ LSP plugin recommendation                                    │\r\n\
│                                                              │\r\n\
│ LSP provides code intelligence like go-to-definition and     │\r\n\
│ error checking                                               │\r\n\
│                                                              │\r\n\
│ Plugin:       rust-analyzer-lsp                              │\r\n\
│ Marketplace:  claude-plugins-official                        │\r\n\
│ Triggered by: .rs files                                      │\r\n\
│                                                              │\r\n\
│ Would you like to install this LSP plugin?                   │\r\n\
│                                                              │\r\n\
│ ❯ Yes, install                                               │\r\n\
│   No, not now                                                │\r\n\
│   Never for this plugin                                      │\r\n\
│   Disable all LSP recommendations                            │\r\n\
╰──────────────────────────────────────────────────────────────╯\r\n";

/// The session survey, as Claude Code draws it under its prompt.
const SURVEY_LINES: &str =
    "\r\n How is Claude doing this session? (optional)\r\n 1: Bad    2: Fine   3: Good   0: Dismiss\r\n";

/// The screen the window goes back to once the offer is closed: its prompt.
const PROMPT: &str =
    "\x1b[2J\x1b[H fake agent: the offer closed by itself · bypass permissions on\r\n";

/// How long the offer stays where the message asks for it: its seconds.
pub fn offer_in(text: &str) -> Option<Duration> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(OFFER)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|seconds| seconds.parse().ok())
            .map(Duration::from_secs)
    })
}

/// Whether the message asks for the survey.
pub fn survey_in(text: &str) -> bool {
    text.lines().any(|line| line.trim().starts_with(SURVEY))
}

/// Appends `line` to the file `name` of the config folder `config`.
fn note(config: &Path, name: &str, line: &str) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(config.join(name))?;
    file.write_all(format!("{line}\n").as_bytes())
}

/// The offer, open `open`: the status says it waits on it and the screen shows it; then it
/// closes by itself and the turn goes on.
pub fn offer(records: &Records, pid: u32, open: Duration) -> io::Result<()> {
    records.waiting(pid, "dialog open")?;
    say(DIALOG)?;
    sleep(open);
    say(PROMPT)?;
    records.status(pid, "busy")
}

/// The survey, unless the window's environment turns it off; one drawn is written down in the
/// window's config folder.
pub fn survey() -> io::Result<()> {
    if own_var("CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY").is_some() {
        return Ok(());
    }
    if let Some(config) = own_var("CLAUDE_CONFIG_DIR") {
        note(Path::new(&config), SURVEYS, "session")?;
    }
    say(SURVEY_LINES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_asks_for_the_offer_with_its_seconds_and_another_for_the_survey() {
        assert_eq!(
            offer_in("LSP-OFFER 8 Reply with exactly: OK"),
            Some(Duration::from_secs(8))
        );
        assert_eq!(
            offer_in("Brief\n  LSP-OFFER 3\nmore"),
            Some(Duration::from_secs(3))
        );
        assert_eq!(offer_in("LSP-OFFER soon"), None);
        assert_eq!(offer_in("Reply with exactly: OK"), None);
        assert!(survey_in("SURVEY Reply with exactly: OK"));
        assert!(!survey_in("Reply with exactly: OK"));
    }

    #[test]
    fn the_offer_draws_claude_codes_question_and_its_four_choices() {
        for words in [
            "Would you like to install this LSP plugin?",
            "❯ Yes, install",
            "No, not now",
            "Never for this plugin",
            "Disable all LSP recommendations",
        ] {
            assert!(DIALOG.contains(words), "{words}");
        }
    }
}
