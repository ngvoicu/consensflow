//! A project of the run: its folder, the trust Claude Code asks of it, and the
//! opening of it with its staff and the gate on, as the app opens one and as
//! the checkbox of its Staff dialog turns the gate on.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::{self, text, Log};
use cf_e2e::rig::{Project, Rig};
use cf_e2e::{files, pattern, Error, Result};
use regex::Regex;
use serde_json::{json, Value};

use crate::agents::{self, DESIGNER};
use crate::plan::Planned;

/// What every folder of the run holds for the agents to read, the bench's: they
/// do what each message asks and nothing more, and a message that asks nothing
/// is answered in a line, so that no delivery waits for an agent exploring.
pub const BRIEF: &str = "This folder is an automated ConsensFlow bench. Do exactly what each \
message asks and nothing more. When a ConsensFlow message arrives, reply with one line that \
names it and run no tools unless the message tells you to run a command.\n";

/// The folder of the project `planned`, in the run's workspace: a git
/// repository with a README, and [`BRIEF`] where Claude Code reads it
/// (`CLAUDE.md`) and the other harnesses do (`AGENTS.md`).
pub fn folder(rig: &Rig, planned: &Planned) -> Result<PathBuf> {
    let folder = rig.workspace().join(format!(
        "p{}-{}",
        planned.number,
        planned.seats.chief.name()
    ));
    live::git_repository(&folder)?;
    files::write(
        &folder.join("README.md"),
        format!(
            "# Approval bench, project {}\n\nA demo project: its chief is {}.\n",
            planned.number,
            planned.seats.chief.name()
        ),
    )?;
    for name in ["AGENTS.md", "CLAUDE.md"] {
        files::write(&folder.join(name), BRIEF)?;
    }
    Ok(folder)
}

/// Opens the project: the chief on its agent, the staff in their seats (the
/// image designer too, where the plan has one), then the gate on, by the call
/// the Staff dialog's checkbox makes. The gate is read back from the board.
pub fn open<'a>(rig: &'a Rig, planned: &Planned, folder: &Path) -> Result<Project<'a>> {
    let seats = &planned.seats;
    let mut staff = vec![
        json!({ "agent": agents::id(seats.worker), "roles": ["worker"] }),
        json!({ "agent": agents::id(seats.advisor), "roles": ["advisor"] }),
        json!({ "agent": agents::id(seats.reviewer), "roles": ["reviewer"] }),
    ];
    if planned.designer {
        staff.push(json!({ "agent": DESIGNER, "roles": ["designer"] }));
    }
    let project = Project::open(
        rig,
        &agents::id(seats.chief),
        json!({ "directory": folder, "staff": staff }),
    )?;
    let gated = rig.page(
        "project.gate",
        json!({ "project": project.id(), "gate": true }),
    )?;
    if gated["ok"] != true {
        return Err(Error::Daemon(format!("project.gate was refused: {gated}")));
    }
    if project.board()?["project"]["gate"] != true {
        return Err(Error::Daemon(
            "the board does not say the project needs the human's approval".to_owned(),
        ));
    }
    Ok(project)
}

/// How long a Claude window has to draw its first screen, and to draw what
/// follows an answer.
const DRAWS: Duration = Duration::from_secs(60);

/// How long a window's screen holds still before it counts as drawn.
const STILL: Duration = Duration::from_secs(3);

const DOWN: [u8; 3] = [27, 91, 66];
const ENTER: [u8; 1] = [13];

/// Claude's question about a folder it has not seen, and the choice it is on.
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

/// Trusts `folder` for Claude Code, once, as a person would. Claude asks
/// whether to trust a folder it has not seen before an interactive session
/// starts, and nobody can answer that in an unattended window: this opens
/// Claude there in a window of the run's pane host, moves to "Yes, I trust
/// this folder", confirms it with Enter once the ❯ is on it (the question opens
/// on "No, exit", where Enter alone would quit), lets Claude write that down,
/// and closes the window. What came of it, in a word. `claude` is the program
/// the run's Claude windows start through (the wrapper that shuts out the MCP
/// servers and the browser), so that this one, unattended on the human's own
/// setup, reaches none of them either.
pub fn trust_claude(rig: &Rig, log: &Log, claude: &Path, folder: &Path) -> Result<&'static str> {
    let id = format!(
        "trust-{}",
        folder
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    );
    let pane = json!({ "id": id, "generation": 1 });
    let opened = rig.host(
        "pane.open",
        json!({
            "id": id, "generation": 1, "cwd": folder, "argv": [claude], "env": {},
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
        shown.iter().map(text).collect::<Vec<_>>().join("\n")
    };
    let kills = || {
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
        kills();
        return Err(Error::Daemon(format!(
            "Claude drew no screen in {} s in {}; its screen: {shown}",
            DRAWS.as_secs(),
            folder.display()
        )));
    }
    if !asks_to_trust(&screen()) {
        kills();
        return Ok("already trusted");
    }
    log.say(format!(
        "Claude asks to trust {}: the human says yes",
        folder.display()
    ));
    for presses in 0..=3 {
        if chosen(&screen()).as_deref() == Some("Yes, I trust this folder") {
            break;
        }
        if presses == 3 {
            let shown = screen();
            kills();
            return Err(Error::Daemon(format!(
                "could not choose \"Yes, I trust this folder\"; its screen: {shown}"
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
    kills();
    Ok("trusted")
}

/// What a lane of the board says of the member it is, for a report: the
/// handle, the agent and the harness.
pub fn member(lane: &Value) -> String {
    let who = &lane["participant"];
    format!(
        "{} ({} on {})",
        text(&who["handle"]),
        text(&who["agent"]),
        text(&who["harness"])
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_asks_about_a_folder_it_has_not_seen_and_says_which_choice_it_is_on() {
        let screen = "Quick safety check: Is this a project you created or one you trust?\n\
                      ❯ 1. Yes, I trust this folder\n  2. No, exit";
        assert!(asks_to_trust(screen));
        assert_eq!(chosen(screen).as_deref(), Some("Yes, I trust this folder"));
        let on_no =
            "Do you trust the files in this folder?\n❯ No, exit\n  Yes, I trust this folder";
        assert_eq!(chosen(on_no).as_deref(), Some("No, exit"));
        assert!(!asks_to_trust("Welcome back! Try \"fix lint errors\""));
        assert_eq!(chosen("nothing chosen"), None);
        // The newest choice is the one shown last.
        assert_eq!(
            chosen("❯ No, exit\n...\n❯ Yes, I trust this folder").as_deref(),
            Some("Yes, I trust this folder")
        );
    }

    #[test]
    fn a_member_is_named_by_its_handle_agent_and_harness() {
        let lane = json!({ "participant": { "handle": "live-pi-a-b", "agent": "live-pi", "harness": "pi" } });
        assert_eq!(member(&lane), "live-pi-a-b (live-pi on pi)");
    }

    #[test]
    fn every_agent_of_the_folder_reads_the_same_brief() {
        assert!(BRIEF.contains("Do exactly what each message asks and nothing more."));
        assert!(BRIEF.ends_with('\n'));
    }
}
