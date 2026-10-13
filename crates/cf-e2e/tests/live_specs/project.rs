//! A project of the run: its folder, the opening of it with its staff (the stand-in
//! worker alone), and what the human sets for its specs on its page.

use std::path::{Path, PathBuf};

use cf_e2e::live::agents;
use cf_e2e::live::members::{stand_in_worker, STAND_IN_WORKER};
use cf_e2e::rig::{Project, Rig};
use cf_e2e::{files, Result};
use serde_json::{json, Value};

use crate::plan::{Chosen, Planned};

/// The tier of every agent of the run: a task names its seat's pool, so the tier
/// tells nothing apart, and one that is not critical asks for no purpose.
pub const TIER: &str = "standard";

/// What every folder of the run tells its agents. The bench's rule is theirs (do
/// what is asked and nothing more); the workers are the stand-in, which does no
/// work and answers a task with the words after `Reply with exactly:` in its brief,
/// so the briefs end with them and the results say that the task is done; the
/// questions of a skill's interview are asked in plain text, in a message, which
/// the human answers in one; and there is nothing to research. It says nothing of
/// specs, which a project with them off is to be told nothing of.
pub const BRIEF: &str = "This folder is an automated ConsensFlow test. Do exactly what the human asks and nothing more, and reply with one line when a turn is over.\n\
\n\
The workers here are scripted: a worker does no work and answers a task with exactly the words after `Reply with exactly:` in its brief. So end every brief you put on the board with the line `Reply with exactly: DONE` (a task that comes from a numbered item of a plan has that item's code after it: `Reply with exactly: DONE-<code>`), and accept a result that says so.\n\
\n\
When you need to ask the human something, ask in plain text, all your questions in one message: do not use a question dialog. Nothing in this project needs research: where a skill asks for some, say there is none and go on.\n";

/// What a project's specs are set to, which the human does on the page
/// (`project.specs`) once the project is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The specs folder, relative to the project's folder, with slashes.
    pub folder: String,
    /// The variant the chief plans with.
    pub variant: Chosen,
    /// Where the chief's harness has that variant installed for its user, when it
    /// has (the adapter's answer: the file of its `SKILL.md`).
    pub installed_at: Option<String>,
}

impl Settings {
    /// What the page asks of the daemon to turn them on for the project.
    pub fn request(&self, project: i64) -> Value {
        json!({
            "project": project, "on": true,
            "folder": self.folder, "variant": self.variant.name,
        })
    }

    /// What the daemon says the project's specs are, once they are.
    pub fn view(&self) -> Value {
        json!({ "on": true, "folder": self.folder, "variant": self.variant.name })
    }
}

/// The agents of the run's roster: the chief's harness on its cheap model, and
/// the stand-in worker.
pub fn roster(planned: &Planned) -> Vec<Value> {
    let mut worker = stand_in_worker();
    if let Some(fields) = worker.as_object_mut() {
        fields.insert("workTier".to_owned(), json!(TIER));
    }
    vec![agents::row(planned.chief, TIER), worker]
}

/// The folder of the project `planned`, in the run's workspace: a git repository
/// with a README, and [`BRIEF`] where Claude Code reads it (`CLAUDE.md`) and the
/// other harnesses do (`AGENTS.md`).
pub fn folder(rig: &Rig, planned: &Planned) -> Result<PathBuf> {
    let folder = rig
        .workspace()
        .join(format!("p{}-{}", planned.number, planned.chief.name()));
    cf_e2e::live::git_repository(&folder)?;
    files::write(
        &folder.join("README.md"),
        format!(
            "# Specs test, project {}\n\nA demo project: its chief is {}.\n",
            planned.number,
            planned.chief.name()
        ),
    )?;
    for name in ["AGENTS.md", "CLAUDE.md"] {
        files::write(&folder.join(name), BRIEF)?;
    }
    Ok(folder)
}

/// Opens the project as the New project dialog does: the chief on its agent, and
/// the stand-in worker on the staff, so that the chief's first text lists it. Its
/// specs are off, as every project's are until the human says otherwise.
pub fn open<'a>(rig: &'a Rig, planned: &Planned, folder: &Path) -> Result<Project<'a>> {
    Project::open(
        rig,
        &agents::id(planned.chief),
        json!({
            "directory": folder,
            "staff": [{ "agent": STAND_IN_WORKER, "roles": ["worker"] }],
        }),
    )
}

#[cfg(test)]
#[path = "project/tests.rs"]
mod tests;
