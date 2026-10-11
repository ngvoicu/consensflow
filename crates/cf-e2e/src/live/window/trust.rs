//! Claude Code's question about a folder it has not seen, answered as a person
//! answers it. Claude asks whether to trust a folder before an interactive
//! session starts in it, and nobody can answer that in an unattended window: so
//! the folder is trusted once, here, in a window of the run's pane host that is
//! closed when it has been.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::thread::sleep;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::json;

use crate::rig::Rig;
use crate::{pattern, Error, Result};

/// How long a Claude window has to draw its first screen, and to draw what
/// follows an answer.
const DRAWS: Duration = Duration::from_secs(60);

/// How long a window's screen holds still before it counts as drawn.
const STILL: Duration = Duration::from_secs(3);

const DOWN: [u8; 3] = [27, 91, 66];
const ENTER: [u8; 1] = [13];

/// The choice that trusts the folder.
const YES: &str = "Yes, I trust this folder";

/// The id of the next window opened to trust a folder: one of its own, for a
/// run that trusts several folders (or the same one again) in one rig.
fn next_pane_id() -> String {
    static OPENED: AtomicU64 = AtomicU64::new(0);
    format!("trust-{}", OPENED.fetch_add(1, Ordering::Relaxed) + 1)
}

/// Claude's question about a folder it has not seen.
fn asks_to_trust(screen: &str) -> bool {
    static ASKS: OnceLock<Regex> = OnceLock::new();
    pattern::once(&ASKS, r"(?i)trust this folder").is_match(screen)
}

/// The choice of the question that the `❯` is on, if the screen shows one.
fn chosen(screen: &str) -> Option<String> {
    static CHOSEN: OnceLock<Regex> = OnceLock::new();
    pattern::once(
        &CHOSEN,
        r"❯\s*(?:\d+\.\s*)?(No, exit|Yes, I trust this folder)",
    )
    .captures_iter(screen)
    .last()
    .map(|found| found[1].to_owned())
}

/// Trusts `folder` for Claude Code, once, as a person would: opens Claude there
/// in a window of the run's pane host (`argv`: the real Claude Code, started as
/// the run starts it, so that this window, unattended on the human's own setup,
/// reaches none of their MCP servers either), moves to "Yes, I trust this
/// folder", confirms it with Enter once the ❯ is on it, lets Claude write that
/// down, and closes the window. Since Claude Code 2.1.287 the question opens on
/// "No, exit", where an Enter alone would quit. What came of it, in a word.
pub fn claude(rig: &Rig, argv: &[String], folder: &Path) -> Result<&'static str> {
    let id = next_pane_id();
    let pane = json!({ "id": id, "generation": 1 });
    let opened = rig.host(
        "pane.open",
        json!({
            "id": id, "generation": 1, "cwd": folder, "argv": argv, "env": {},
            "size": { "rows": 40, "cols": 120 },
        }),
    )?;
    if opened["ok"] == false {
        return Err(Error::Daemon(format!(
            "Claude did not open in {} to be trusted there: {opened}",
            folder.display()
        )));
    }
    let screen = || -> String {
        let shown = rig
            .host(
                "pane.snapshot",
                json!({ "id": pane["id"], "generation": 1, "tail": 12 }),
            )
            .ok()
            .and_then(|answer| answer["tail"].as_array().cloned())
            .unwrap_or_default();
        shown
            .iter()
            .map(super::super::text)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let kill = || {
        let _ = rig.host("pane.kill", pane.clone());
    };
    let still = |within: Duration| -> bool {
        let started = Instant::now();
        let (mut seen, mut since) = (screen(), Instant::now());
        while started.elapsed() < within {
            sleep(Duration::from_millis(250));
            let now = screen();
            if now != seen {
                (seen, since) = (now, Instant::now());
            } else if !seen.is_empty() && since.elapsed() >= STILL {
                return true;
            }
        }
        false
    };
    if !still(DRAWS) {
        let shown = screen();
        kill();
        return Err(Error::Daemon(format!(
            "Claude drew no screen in {} s in {}; its screen: {shown}",
            DRAWS.as_secs(),
            folder.display()
        )));
    }
    if !asks_to_trust(&screen()) {
        kill();
        return Ok("already trusted");
    }
    for presses in 0..=3 {
        if chosen(&screen()).as_deref() == Some(YES) {
            break;
        }
        if presses == 3 {
            let shown = screen();
            kill();
            return Err(Error::Daemon(format!(
                "could not choose \"{YES}\"; its screen: {shown}"
            )));
        }
        rig.host(
            "pane.input",
            json!({ "id": pane["id"], "generation": 1, "bytes": DOWN }),
        )?;
        sleep(Duration::from_millis(500));
    }
    rig.host(
        "pane.input",
        json!({ "id": pane["id"], "generation": 1, "bytes": ENTER }),
    )?;
    // Claude writes the trust down as it moves on to its prompt.
    still(DRAWS);
    kill();
    Ok("trusted")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_asks_about_a_folder_it_has_not_seen_and_says_which_choice_it_is_on() {
        let screen = "Quick safety check: Is this a project you created or one you trust?\n\
                      ❯ 1. Yes, I trust this folder\n  2. No, exit";
        assert!(asks_to_trust(screen));
        assert_eq!(chosen(screen).as_deref(), Some(YES));
        // Since Claude Code 2.1.287 the question opens on "No, exit", where an Enter alone would quit.
        let on_no =
            "Do you trust the files in this folder?\n❯ No, exit\n  Yes, I trust this folder";
        assert_eq!(chosen(on_no).as_deref(), Some("No, exit"));
        assert!(!asks_to_trust("Welcome back! Try \"fix lint errors\""));
        assert_eq!(chosen("nothing chosen"), None);
        // The newest choice is the one shown last.
        assert_eq!(
            chosen("❯ No, exit\n...\n❯ Yes, I trust this folder").as_deref(),
            Some(YES)
        );
    }

    #[test]
    fn each_window_opened_to_trust_a_folder_has_an_id_of_its_own() {
        let (first, second) = (next_pane_id(), next_pane_id());
        assert_ne!(first, second);
        assert!(first.starts_with("trust-") && second.starts_with("trust-"));
    }
}
