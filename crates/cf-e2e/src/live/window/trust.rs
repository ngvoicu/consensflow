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
use serde_json::{json, Value};

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
const YES_ADD: &str = "Yes, add it";
const ADD_TITLE: &str = "Adding a new directory:";
const ADD_QUESTION: &str = "This session";

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
        || (screen.lines().any(|line| line.trim() == ADD_TITLE)
            && screen.lines().any(|line| line.trim().ends_with(YES_ADD))
            && screen.lines().any(|line| line.trim().ends_with("No, exit")))
}

/// The choice of the question that the `❯` is on, if the screen shows one.
fn chosen(screen: &str) -> Option<String> {
    static CHOSEN: OnceLock<Regex> = OnceLock::new();
    pattern::once(
        &CHOSEN,
        r"❯\s*(?:\d+\.\s*)?(No, exit|Yes, I trust this folder|Yes, add it)",
    )
    .captures_iter(screen)
    .last()
    .map(|found| found[1].to_owned())
}

/// Whether the added-directory dialog names exactly one of this invocation's
/// run-owned added directories. No other screen text is part of the path, and
/// no missing character is guessed at a row boundary.
fn added_by_run(screen: &str, argv: &[String]) -> bool {
    let lines: Vec<&str> = screen.lines().collect();
    let Some(title) = lines.iter().rposition(|line| line.trim() == ADD_TITLE) else {
        return false;
    };
    let after = &lines[title + 1..];
    let Some(end) = after
        .iter()
        .position(|line| line.trim().starts_with(ADD_QUESTION))
    else {
        return false;
    };
    let choice = |line: &&str, words: &str| {
        let line = line.trim();
        line.strip_prefix('❯').unwrap_or(line).trim() == words
    };
    let (Some(no), Some(yes)) = (
        after.iter().rposition(|line| choice(line, "No, exit")),
        after.iter().rposition(|line| choice(line, YES_ADD)),
    ) else {
        return false;
    };
    let last = no.max(yes);
    let selected = after
        .iter()
        .rposition(|line| line.trim_start().starts_with('❯'));
    if no <= end
        || yes <= end
        || no.abs_diff(yes) != 1
        || after[last + 1..]
            .iter()
            .any(|line| !line.trim().is_empty() && !line.contains("Enter to confirm"))
        || !matches!(selected, Some(at) if at == no || at == yes)
    {
        return false;
    }
    let mut path = &after[..end];
    if path.first().is_some_and(|line| line.trim().is_empty()) {
        path = &path[1..];
    }
    if path.last().is_some_and(|line| line.trim().is_empty()) {
        path = &path[..path.len() - 1];
    }
    if path.is_empty() {
        return false;
    }
    argv.windows(2)
        .filter(|pair| pair[0] == "--add-dir")
        .filter(|pair| crate::door::is_made_or_below(Path::new(&pair[1])))
        .any(|pair| {
            let name = Path::new(&pair[1]);
            let real = name.canonicalize().ok().map(|real| {
                let real = real.to_string_lossy();
                if cfg!(windows) {
                    crate::files::plain_name(&real)
                } else {
                    real.into_owned()
                }
            });
            path_matches(path, &pair[1]) || real.is_some_and(|real| path_matches(path, &real))
        })
}

fn path_matches(lines: &[&str], expected: &str) -> bool {
    let expected = if cfg!(windows) {
        expected.replace('/', "\\")
    } else {
        expected.to_owned()
    };
    let Some(first) = lines.first() else {
        return false;
    };
    let indent = &first[..first.len() - first.trim_start().len()];
    let mut remaining = expected.as_str();
    for line in lines {
        let Some(segment) = line.strip_prefix(indent) else {
            return false;
        };
        if segment.is_empty() {
            return false;
        }
        let segment = if cfg!(windows) {
            segment.replace('/', "\\")
        } else {
            segment.to_owned()
        };
        let Some(rest) = remaining.strip_prefix(&segment) else {
            return false;
        };
        remaining = rest;
    }
    remaining.is_empty()
}

fn screen_text(rows: &[Value]) -> String {
    rows.iter()
        .map(super::super::text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn has_screen_content(screen: &str) -> bool {
    screen.chars().any(char::is_alphanumeric)
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
                json!({ "id": pane["id"], "generation": 1, "bottom": 40 }),
            )
            .ok()
            .and_then(|answer| answer["bottom"].as_array().cloned())
            .unwrap_or_default();
        screen_text(&shown)
    };
    let kill = || {
        let _ = rig.close_pane(pane.clone());
    };
    let still = |within: Duration| -> bool {
        let started = Instant::now();
        let (mut seen, mut since) = (screen(), Instant::now());
        while started.elapsed() < within {
            sleep(Duration::from_millis(250));
            let now = screen();
            if now != seen {
                (seen, since) = (now, Instant::now());
            } else if has_screen_content(&seen) && since.elapsed() >= STILL {
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
    let mut answered = false;
    let max_questions = 1 + argv
        .iter()
        .filter(|word| word.as_str() == "--add-dir")
        .count();
    for _ in 0..=max_questions {
        let shown = screen();
        if !asks_to_trust(&shown) {
            kill();
            return Ok(if answered {
                "trusted"
            } else {
                "already trusted"
            });
        }
        let added = shown.lines().any(|line| line.trim() == ADD_TITLE);
        if added && !added_by_run(&shown, argv) {
            kill();
            return Err(Error::Daemon(
                "Claude asked to add a directory this run did not add; no key was pressed"
                    .to_owned(),
            ));
        }
        let yes = if added { YES_ADD } else { YES };
        for presses in 0..=3 {
            let current = screen();
            if added && (!asks_to_trust(&current) || !added_by_run(&current, argv)) {
                kill();
                return Err(Error::Daemon(
                    "the added-directory question changed; no further key was pressed".to_owned(),
                ));
            }
            if chosen(&current).as_deref() == Some(yes) {
                break;
            }
            if added && chosen(&current).as_deref() != Some("No, exit") {
                kill();
                return Err(Error::Daemon(
                    "the added-directory choice was not readable; no further key was pressed"
                        .to_owned(),
                ));
            }
            if presses == 3 {
                kill();
                return Err(Error::Daemon(format!("could not choose \"{yes}\"")));
            }
            rig.host(
                "pane.input",
                json!({ "id": pane["id"], "generation": 1, "bytes": DOWN }),
            )?;
            sleep(Duration::from_millis(500));
            still(DRAWS);
        }
        let before = screen();
        if added && (!added_by_run(&before, argv) || chosen(&before).as_deref() != Some(yes)) {
            kill();
            return Err(Error::Daemon(
                "the added-directory question changed before confirmation; Enter was not pressed"
                    .to_owned(),
            ));
        }
        rig.host(
            "pane.input",
            json!({ "id": pane["id"], "generation": 1, "bytes": ENTER }),
        )?;
        // Never press Enter twice into the same screen: it could answer what follows.
        still(DRAWS);
        if screen() == before {
            kill();
            return Err(Error::Daemon(
                "the trust screen did not move after Enter".to_owned(),
            ));
        }
        answered = true;
    }
    kill();
    Err(Error::Daemon("too many Claude trust questions".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Claude Code 2.1.326, zeewin2: the run-owned path is replaced by a fixture.
    fn added_screen(on_yes: bool) -> String {
        let choices = if on_yes {
            "  No, exit\n❯ Yes, add it"
        } else {
            "❯ No, exit\n  Yes, add it"
        };
        format!("Adding a new directory:\nC:\\run\\role\nThis session hasn't worked here before. Is this a directory you created or one you trust?\nClaude Code'll be able to read, edit, and execute files here. Its skills, commands, and agents load and can run\ncommands, and so can the git settings of its repository. Your answer holds for this session. Starting Claude Code in\nthis directory and trusting it there lasts, and covers its whole repository or everything below it.\nSecurity guide\n{choices}\nEnter to confirm · Esc to cancel")
    }

    #[test]
    fn added_folder_on_no_is_a_question() {
        let screen = added_screen(false);
        assert!(asks_to_trust(&screen));
        assert_eq!(chosen(&screen).as_deref(), Some("No, exit"));
    }

    #[test]
    fn added_folder_on_yes_is_a_question_and_confirms() {
        let screen = added_screen(true);
        assert!(asks_to_trust(&screen));
        assert_eq!(chosen(&screen).as_deref(), Some("Yes, add it"));
    }

    #[test]
    fn added_folder_must_be_an_argument_made_by_this_run() {
        let own = crate::door::made("added-trust-test-").unwrap();
        let role = own.path().join("role");
        crate::door::make_dir(&role).unwrap();
        let name = role.to_string_lossy().to_string();
        let screen = added_screen(false).replace(r"C:\run\role", &name);
        let argv = vec!["claude".to_owned(), "--add-dir".to_owned(), name.clone()];
        assert!(added_by_run(&screen, &argv));
        assert!(!added_by_run(&screen, &["claude".to_owned()]));
        assert!(!added_by_run(
            &screen.replace(&name, &format!("{name}-other")),
            &argv
        ));
        // The test harness owns its temporary parent too. A filesystem root is
        // outside that allocation on both Unix and Windows.
        let foreign = own.path().ancestors().last().unwrap();
        let foreign_name = foreign.to_string_lossy().to_string();
        let foreign_screen = added_screen(false).replace(r"C:\run\role", &foreign_name);
        assert!(!crate::door::is_made_or_below(foreign));
        assert!(!added_by_run(
            &foreign_screen,
            &["--add-dir".to_owned(), foreign_name]
        ));
    }

    #[test]
    fn safety_added_raw_screen_preserves_the_real_dialog() {
        let own = crate::door::made("added-trust-raw-").unwrap();
        let name = own.path().to_string_lossy().to_string();
        let screen = added_screen(false)
            .replace(r"C:\run\role", &name)
            .replace("Adding a new directory:\n", "Adding a new directory:\n\n")
            .replace("\nThis session", "\n\nThis session")
            .replace("Yes, add it\nEnter", "Yes, add it\n\nEnter");
        assert!(added_by_run(&screen, &["--add-dir".to_owned(), name]));
    }

    #[test]
    fn safety_added_punctuation_row_is_not_erased_by_kit() {
        let own = crate::door::made("added-trust-punctuation-").unwrap();
        let role = own.path().join("role");
        crate::door::make_dir(&role).unwrap();
        let name = role.to_string_lossy().to_string();
        let prefix = name.strip_suffix("role").unwrap();
        let shown = added_screen(false).replace(r"C:\run\role", &format!("{prefix}\n---\nrole"));
        let rows: Vec<Value> = shown.lines().map(|line| json!(line)).collect();
        assert!(!added_by_run(
            &screen_text(&rows),
            &["--add-dir".to_owned(), name.clone()]
        ));
        let shown = added_screen(false).replace(r"C:\run\role", &format!("{prefix}\n\nrole"));
        let rows: Vec<Value> = shown.lines().map(|line| json!(line)).collect();
        assert!(!added_by_run(
            &screen_text(&rows),
            &["--add-dir".to_owned(), name]
        ));
    }

    #[test]
    fn safety_added_blank_or_border_only_screen_does_not_count_as_drawn() {
        let rows = [json!(""), json!("   "), json!("────")];
        assert!(!has_screen_content(&screen_text(&rows)));
        assert!(has_screen_content("Adding a new directory:"));
    }

    #[test]
    fn a_quoted_added_folder_does_not_authorize_a_later_different_question() {
        let own = crate::door::made("added-trust-quoted-").unwrap();
        let name = own.path().to_string_lossy().to_string();
        let screen = format!(
            "{}\nAccessing workspace:\n/outside\nQuick safety check: Is this a project you created or one you trust?\n❯ No, exit\n  Yes, I trust this folder\nEnter to confirm · Esc to cancel",
            added_screen(false).replace(r"C:\run\role", &name).replace("❯ No, exit", "  No, exit")
        );
        let argv = vec!["--add-dir".to_owned(), name];
        assert!(!added_by_run(&screen, &argv));
    }

    #[test]
    fn a_wrapped_path_does_not_discard_an_extra_space() {
        assert!(!path_matches(&[r" C:\run", r"  \role"], r"C:\run\role"));
    }

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
