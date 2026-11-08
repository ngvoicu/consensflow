//! Claude Code's offers that wait in a window for a key, as the stand-in draws them mid-turn
//! (T-1183, read from Claude Code 2.1.317).
//!
//! A message with a line `LSP-OFFER <seconds>` makes the window offer to install an LSP plugin
//! as its turn works, as Claude Code does once the turn writes a file of an extension a
//! marketplace has a plugin for: its screen shows the dialog ("LSP plugin recommendation",
//! "Would you like to install this LSP plugin?", its `❯` on "1. Yes, install" and three choices
//! more) over its spinner for that long, as Claude Code closes it by itself after 30 s, while
//! its status says it is busy, as Claude Code's did in a turn at work (the live run of
//! 2026-11-03); then the screen is cleared to its prompt and the turn goes on. A digit typed
//! alone while it is open answers it at once with the choice of that number, as Claude Code's
//! lists take a digit; how it was answered is written down (`offer-answers`, in its config
//! folder: the digit, or `timeout` where nothing answered it), which Claude Code itself writes
//! nowhere for "No, not now" (T-1192). With a word `TURN-ENDS` after the offer's seconds
//! (`LSP-OFFER 30 TURN-ENDS`) the turn ends under the open offer once a file `turn-ends` is in
//! its config folder (the case writes it when the chief was told), as the live window's did on
//! 2026-11-06 (T-1261): its status turns to `waiting` ("dialog open"), the spinner under the
//! offer goes, and the line that closes a turn is drawn above it, so that the pane host's tail,
//! the last twelve lines with a letter or a digit in them, slides down a line.
//!
//! A message with a line `HOOK-ASK <seconds>` makes the window ask, as its turn works, what
//! Claude Code asks where a plugin's PreToolUse hook answers "ask" for a call (the screen of a
//! worker on the owner's kluris board, 2026-11-03, T-1198): "Hook tool.check requires
//! confirmation for this command: …", "Do you want to proceed?", `❯ 1. Yes`, `2. No`, its status
//! `waiting` on a "permission prompt"; for that long, or until a digit answers it, written down
//! after `hook:` in `offer-answers`.
//!
//! A message with a line `SURVEY` makes the window draw Claude Code's session survey under its
//! prompt as its turn works ("How is Claude doing this session? (optional)", a digit a
//! choice), unless its environment says `CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY`, as Claude Code
//! draws none then; a survey drawn is written down (`surveys`, in its config folder).
//!
//! A message with a line `PERMISSION-ASK <seconds>` makes the window ask, as its turn works, what
//! Claude Code 2.1.319 asks of a command it cannot allow by itself, a dangerous `rm`: the command,
//! the rule's words, and under them the line that counts down to the block of the request ("⚠
//! Claude Code will automatically block this request in 0:12, to avoid holding up an unattended
//! session"), redrawn each second, with the window's status `waiting`. At the end of the clock
//! Claude Code blocks the request by itself, as on the owner's board on 2026-11-06 (T-1268): the
//! status goes back to `busy` while the screen still shows the prompt for some seconds more, and
//! then the window goes back to its prompt. Nothing typed into it answers it.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::process::own_var;

use super::records::Records;
use super::say;

/// The word of a line that asks for the LSP offer, and of one that asks for the survey.
const OFFER: &str = "LSP-OFFER ";
const SURVEY: &str = "SURVEY";

/// The files in the window's config folder of the surveys drawn and of how each offer was
/// answered, and the one a case writes there to end the turn under an open offer.
pub const SURVEYS: &str = "surveys";
pub const ANSWERS: &str = "offer-answers";
pub const TURN_ENDS: &str = "turn-ends";

/// Whether an offer is open, and the digit typed into it: 0 for none yet.
static OPEN: AtomicBool = AtomicBool::new(false);
static ANSWER: AtomicU8 = AtomicU8::new(0);

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

/// What the offer's screen is once the turn under it ended: the line that closes a turn, then the
/// offer without its spinner, as the live window drew it (2026-11-06, T-1261).
const TURN_ENDED: &str = "\u{273b} Baked for 9s \u{b7} done 8:21 AM \u{b7} 1 shell still running";

/// The session survey, as Claude Code draws it under its prompt.
const SURVEY_LINES: &str =
    "\r\n How is Claude doing this session? (optional)\r\n 1: Bad    2: Fine   3: Good   0: Dismiss\r\n";

/// The screen the window goes back to once the offer is closed: its prompt.
const PROMPT: &str =
    "\x1b[2J\x1b[H fake agent: the offer closed by itself · bypass permissions on\r\n";

/// The ask of a plugin's PreToolUse hook on a Bash call, as Claude Code 2.1.317 drew it in a
/// worker's window on the owner's kluris board (2026-11-03, T-1198, the screenshot's text).
const HOOK_ASK: [&str; 18] = [
    "",
    "\u{2502} git status -s",
    "\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}",
    "\u{2502} Hook tool.check requires confirmation for this",
    "\u{2502} command:",
    "\u{2502} This call's input holds your account email",
    "\u{2502} address, which may leave this machine. If nobody",
    "\u{2502} asked to send it, go on without it.",
    "\u{2502} [plugin:cc-plugin-privacy]",
    "plugin hooks.json to update hooks",
    "",
    "Do you want to proceed?",
    "\u{276f} 1. Yes",
    "  2. No",
    "",
    "Esc to cancel \u{b7} Tab to amend",
    "",
    "",
];

/// The word of a line that asks for a hook's ask.
const HOOK: &str = "HOOK-ASK ";

/// The word of a line that asks for a permission prompt with a countdown.
const PERMISSION: &str = "PERMISSION-ASK ";

/// How long after the status goes back to `busy` the prompt still shows on the screen, as it did
/// on the owner's board, more than a look (5 s) of the engine's.
const BLOCKED_LINGERS: Duration = Duration::from_secs(7);

/// How long the permission prompt asks where the message asks for it: the seconds of its clock.
pub fn permission_ask_in(text: &str) -> Option<Duration> {
    seconds_after(text, PERMISSION)
}

/// The screen of the permission prompt of a dangerous `rm`, `left` seconds before Claude Code
/// blocks it by itself, as the owner's board showed it (the paths are placeholders).
fn permission_screen(left: u64) -> String {
    let clock = format!("{}:{:02}", left / 60, left % 60);
    format!(
        "\x1b[2J\x1b[H Bash command\r\n   $T/tree/app/tests/shots/make.mjs $T/scratch/ && ls $T/scratch\r\n   Dangerous rm operation on possibly-empty variable path: \"$S/$l\" in `rm \"$S/$l\"`\r\n \u{26a0} Claude Code will automatically block this request in {clock}, to avoid holding up an unattended session\r\n\r\n Do you want to proceed?\r\n \u{276f} 1. Yes\r\n   2. No\r\n"
    )
}

/// The permission prompt, its clock counting down `open`: the screen is redrawn each second and the
/// window's status says it waits on a permission prompt; at the end of the clock Claude Code
/// blocks the request by itself, the status goes back to `busy` and the prompt stays on the screen
/// a few seconds more ([`BLOCKED_LINGERS`]).
pub fn permission_ask(records: &Records, pid: u32, open: Duration) -> io::Result<()> {
    records.waiting(pid, "permission prompt")?;
    ANSWER.store(0, Ordering::SeqCst);
    OPEN.store(true, Ordering::SeqCst);
    for left in (0..=open.as_secs()).rev() {
        say(&permission_screen(left))?;
        sleep(Duration::from_secs(1));
    }
    records.status(pid, "busy")?;
    sleep(BLOCKED_LINGERS);
    OPEN.store(false, Ordering::SeqCst);
    // A digit typed into it while it was open (nothing should: it is no dialog of the list of
    // safe declines) is written down, or that nothing answered it.
    let answer = match ANSWER.swap(0, Ordering::SeqCst) {
        0 => "blocked".to_owned(),
        digit => char::from(digit).to_string(),
    };
    if let Some(config) = own_var("CLAUDE_CONFIG_DIR") {
        note(Path::new(&config), ANSWERS, &format!("permission:{answer}"))?;
    }
    say(PROMPT)
}

/// How long the hook's ask stays where the message asks for it: its seconds.
pub fn hook_ask_in(text: &str) -> Option<Duration> {
    seconds_after(text, HOOK)
}

/// How long the offer stays where the message asks for it: its seconds.
pub fn offer_in(text: &str) -> Option<Duration> {
    seconds_after(text, OFFER)
}

/// Whether the turn ends under the offer, where the message's line of it says `TURN-ENDS`.
pub fn turn_ends_in(text: &str) -> bool {
    text.lines().any(|line| {
        line.trim()
            .strip_prefix(OFFER)
            .is_some_and(|rest| rest.split_whitespace().any(|word| word == "TURN-ENDS"))
    })
}

/// The seconds a line of `text` says after `word`.
fn seconds_after(text: &str, word: &str) -> Option<Duration> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(word)
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
pub fn offer(records: &Records, pid: u32, open: Duration, turn_ends: bool) -> io::Result<()> {
    let mut ended = false;
    open_for(&DIALOG, "", open, |_| {
        let told_to_end = own_var("CLAUDE_CONFIG_DIR")
            .is_some_and(|config| Path::new(&config).join(TURN_ENDS).exists());
        if turn_ends && told_to_end && !ended {
            ended = true;
            records.waiting(pid, "dialog open")?;
            // The offer's lines without the spinner (and the blank line above it), under the
            // line that closes a turn.
            let kept = &DIALOG[..DIALOG.len() - 2];
            say(&format!(
                "\x1b[2J\x1b[H{TURN_ENDED}\r\n{}\r\n",
                kept.join("\r\n")
            ))?;
        }
        Ok(())
    })?;
    records.status(pid, "busy")
}

/// The ask of a plugin's PreToolUse hook, open `open`: the screen shows it as Claude Code
/// 2.1.317 drew it in a worker's window on 2026-11-03 (T-1198), and the window's status says it
/// waits on a permission prompt; then it closes, as a person would answer it, and the turn goes
/// on. How it was answered is written down with the offers', after `hook:`; true where it was
/// answered "No" (`2`), which ends the turn in Claude Code: the model is told to stop and wait.
pub fn hook_ask(records: &Records, pid: u32, open: Duration) -> io::Result<bool> {
    records.waiting(pid, "permission prompt")?;
    let answer = open_for(&HOOK_ASK, "hook:", open, |_| Ok(()))?;
    records.status(pid, "busy")?;
    Ok(answer == Some(b'2'))
}

/// The dialog `lines`, open `open` or until a digit answers it, its answer written down after
/// `tag`; `tick` is called at each step of the wait with how long it has been.
fn open_for(
    lines: &[&str],
    tag: &str,
    open: Duration,
    mut tick: impl FnMut(Duration) -> io::Result<()>,
) -> io::Result<Option<u8>> {
    ANSWER.store(0, Ordering::SeqCst);
    OPEN.store(true, Ordering::SeqCst);
    say(&format!("{}\r\n", lines.join("\r\n")))?;
    let step = Duration::from_millis(100);
    let mut waited = Duration::ZERO;
    while waited < open && ANSWER.load(Ordering::SeqCst) == 0 {
        sleep(step);
        waited += step;
        tick(waited)?;
    }
    OPEN.store(false, Ordering::SeqCst);
    let digit = ANSWER.swap(0, Ordering::SeqCst);
    let answer = match digit {
        0 => "timeout".to_owned(),
        digit => char::from(digit).to_string(),
    };
    if let Some(config) = own_var("CLAUDE_CONFIG_DIR") {
        note(Path::new(&config), ANSWERS, &format!("{tag}{answer}"))?;
    }
    say(PROMPT)?;
    Ok((digit != 0).then_some(digit))
}

/// What is typed into the window's box, where an offer is open and it is a digit of one of its
/// choices alone: the offer takes it as its answer, and the box is left empty. False where no
/// offer is open or the box holds anything else.
pub fn takes(typed: &str) -> bool {
    let digit = match typed.as_bytes() {
        [digit @ b'1'..=b'4'] => *digit,
        _ => return false,
    };
    if !OPEN.load(Ordering::SeqCst) {
        return false;
    }
    ANSWER.store(digit, Ordering::SeqCst);
    true
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
        assert_eq!(
            permission_ask_in("PERMISSION-ASK 14 Reply with exactly: OK"),
            Some(Duration::from_secs(14))
        );
        assert_eq!(permission_ask_in("HOOK-ASK 14"), None);
        assert!(permission_screen(117).contains("block this request in 1:57, to avoid"));
        assert!(permission_screen(3).contains("block this request in 0:03, to avoid"));
        assert!(turn_ends_in(
            "LSP-OFFER 30 TURN-ENDS Reply with exactly: OK"
        ));
        assert!(!turn_ends_in("LSP-OFFER 30 Reply with exactly: OK"));
        assert!(!turn_ends_in("TURN-ENDS"));
        assert_eq!(offer_in("Reply with exactly: OK"), None);
        assert!(survey_in("SURVEY Reply with exactly: OK"));
        assert!(!survey_in("Reply with exactly: OK"));
    }

    #[test]
    fn a_digit_typed_alone_answers_an_open_offer_and_nothing_else() {
        assert!(!takes("2"), "no offer is open");
        OPEN.store(true, Ordering::SeqCst);
        assert!(!takes("22"));
        assert!(!takes("5"));
        assert!(!takes("x"));
        assert!(takes("2"));
        assert_eq!(ANSWER.swap(0, Ordering::SeqCst), b'2');
        OPEN.store(false, Ordering::SeqCst);
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
