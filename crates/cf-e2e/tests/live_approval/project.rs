//! A project of the run: its folder, and the opening of it with its staff and
//! the gate on, as the app opens one and as the checkbox of its Staff dialog
//! turns the gate on. (The trust Claude Code asks of the folder is
//! [`cf_e2e::live::window::trust_claude`]'s.)

use std::path::{Path, PathBuf};

use cf_e2e::live::{self, text};
use cf_e2e::rig::{Project, Rig};
use cf_e2e::{files, Error, Result};
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
