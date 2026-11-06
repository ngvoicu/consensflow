//! What the stand-in Codex's TUI draws of a thread, as Codex's draws it (T-1240): with
//! `CF_TEST_CODEX_SCREEN=drawn` a message the thread takes is drawn after a `›`, the lines
//! after it a column in, and once the turn is over the model's answer after a `•`, `Worked
//! for 5s • 16:00`, and the composer (`› Ask Codex to do anything`) with its footer (the model
//! and its effort, the folder) and the line of its keys under it, a blank line between each:
//! the layout of Codex 0.164's recorded screens (the probes of T-model-switch,
//! the screens after its first turn and after a resume). The pane host's tail of it has no blank line,
//! so the composer, the footer and the line over them are one under the other in one column,
//! which is what the engine's reading of a dialog by its shape took for a list of choices.

use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

/// Whether the TUI draws its screen as Codex does.
pub fn drawn() -> bool {
    cf_e2e::process::own_var("CF_TEST_CODEX_SCREEN").as_deref() == Some("drawn")
}

/// How many rows the composer, its footer and its line of keys took where they were last drawn
/// (a blank line after each): what the next drawing takes down first, as Codex redraws its
/// composer at the bottom of the screen and lets the conversation scroll above it.
static COMPOSER_ROWS: AtomicUsize = AtomicUsize::new(0);

/// Writes `lines`, a blank line after each, in the raw mode of the terminal.
fn write(lines: &[String]) {
    let mut out = std::io::stdout();
    for line in lines {
        let _ = write!(out, "{line}\r\n\r\n");
    }
    let _ = out.flush();
}

/// A message of the user's, as Codex echoes it: its first line after a `›`, the rest a column
/// in.
fn echo(text: &str) -> Vec<String> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let mut drawn: Vec<String> = lines
        .next()
        .map(|first| format!("› {first}"))
        .into_iter()
        .collect();
    drawn.extend(lines.map(|line| format!("  {line}")));
    drawn
}

/// `above`, and under it the composer at rest with its footer and the line of keys under that:
/// the composer drawn before is taken down first.
pub fn at_rest(above: &[String]) {
    let folder = std::env::current_dir()
        .map(|folder| folder.display().to_string())
        .unwrap_or_default();
    let before = COMPOSER_ROWS.swap(0, Ordering::Relaxed);
    if before > 0 {
        let _ = write!(std::io::stdout(), "\x1b[{before}A\x1b[J");
    }
    write(above);
    let composer = [
        "› Ask Codex to do anything".to_owned(),
        format!("  GPT-5.6-Luna default · {folder}"),
        "  ? for shortcuts".to_owned(),
    ];
    write(&composer);
    COMPOSER_ROWS.store(composer.len() * 2, Ordering::Relaxed);
}

/// What a notice of the server adds to the screen: the message a turn began on, and what a turn
/// over leaves.
pub fn draw(notice: &Value) {
    match notice["method"].as_str() {
        Some("turn/started") => {
            if let Some(text) = notice["params"]["input"].as_str() {
                at_rest(&echo(text));
            }
        }
        Some("turn/completed") => at_rest(&[
            "• Answered the worker; I will tell you when its result is back.".to_owned(),
            "  Worked for 5s • 16:00".to_owned(),
        ]),
        _ => {}
    }
}
