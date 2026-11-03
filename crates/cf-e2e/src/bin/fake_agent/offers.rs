//! Claude Code's offers that wait in a window for a key, as the stand-in draws them mid-turn
//! (T-1183, read from Claude Code 2.1.317).
//!
//! A message with a line `LSP-OFFER <seconds>` makes the window offer to install an LSP plugin
//! as its turn works, as Claude Code does once the turn writes a file of an extension a
//! marketplace has a plugin for: its screen shows the dialog ("LSP plugin recommendation",
//! "Would you like to install this LSP plugin?", its `❯` on "1. Yes, install" and three choices
//! more) over its spinner for that long, as Claude Code closes it by itself after 30 s, while
//! its status says it is busy, as Claude Code's did in a turn at work (the live run of
//! 2026-11-03); then the screen is cleared to its prompt and the turn goes on. What is typed
//! meanwhile answers nothing.
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

use super::say;

/// The word of a line that asks for the LSP offer, and of one that asks for the survey.
const OFFER: &str = "LSP-OFFER ";
const SURVEY: &str = "SURVEY";

/// The file in the window's config folder of the surveys drawn.
pub const SURVEYS: &str = "surveys";

/// The offer, as Claude Code 2.1.317 drew it in a member's window mid-turn, over its spinner
/// (`cargo xtask live harness-dialogs`, 2026-11-03).
const DIALOG: [&str; 18] = [
    "",
    " LSP plugin recommendation",
    "",
    "   LSP provides code intelligence like go-to-definition and error checking",
    "",
    "   Plugin: clangd-lsp",
    "   Marketplace: claude-plugins-official",
    "   Triggered by: .c files",
    "   Description: C/C++ language server (clangd) for code intelligence",
    "",
    "   Would you like to install this LSP plugin?",
    "",
    "   \u{276f} 1. Yes, install",
    "     2. No, not now",
    "     3. Never for this plugin",
    "     4. Disable all LSP recommendations",
    "",
    "\u{273d} Honking\u{2026} (2s \u{b7} \u{2193} 21 tokens)",
];

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

/// The offer, open `open`: the screen shows it while the turn is at work; then it closes by
/// itself and the turn goes on.
pub fn offer(open: Duration) -> io::Result<()> {
    say(&format!("{}\r\n", DIALOG.join("\r\n")))?;
    sleep(open);
    say(PROMPT)
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
            "\u{276f} 1. Yes, install",
            "No, not now",
            "Never for this plugin",
            "Disable all LSP recommendations",
        ] {
            assert!(DIALOG.iter().any(|line| line.contains(words)), "{words}");
        }
    }
}
