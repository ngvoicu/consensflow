//! Calliope's review of Muse Code's first phase (T-767, 2026-10-27, P1, on
//! `wt/muse-harness` at b33444e, before 3.0.7-alpha.1): **a Muse member was free to the
//! scheduler, took pooled tasks and failed them.**
//!
//! What would have been seen: Muse Code is listed like every harness, and a saved Muse
//! agent could join a staff. The daemon built an adapter for every harness there is,
//! Muse's among them, though Muse's refuses every launch (no window opens on it until
//! its window is built, MUS-08). So the scheduler took a fresh Muse worker for free,
//! ranked it first (it had taken nothing), and gave it the tier's next pooled task,
//! which failed at once: "the launch failed: ConsensFlow cannot open a Muse Code window
//! yet", and so did every pooled task of that tier after it. A chief switched onto
//! Muse was accepted, and its window never came up.
//!
//! What should be seen (the chief's brief for T-789): a harness whose windows
//! ConsensFlow does not open yet is one it has no adapter for. A Muse agent is refused
//! at the door, as a member and as a chief, in the words the engine says of any
//! harness it cannot open, and no window is made on it. The stand-in `muse` on the
//! PATH makes Muse installed, as detection finds it; it is never run.

use std::fs;
use std::path::Path;

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::{config, Outcome};

/// What the engine says of a harness it opens no window of.
const WORDS: &str = "ConsensFlow cannot open muse windows";

/// A stand-in `muse` in `folder`, which writes down that it was run.
fn stand_in(folder: &Path) -> cf_e2e::Result {
    let muse = folder.join("muse");
    let ran = folder.join("muse-ran");
    files::write(
        &muse,
        format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 0\n", ran.display()),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&muse, fs::Permissions::from_mode(0o755))
            .map_err(|error| cf_e2e::Error::Daemon(error.to_string()))?;
    }
    Ok(())
}

/// The roster: a chief on the stand-in Claude, and an agent on Muse.
fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake", "workTier": "standard" },
            { "id": "musey", "kind": "muse", "model": "muse-spark-1.3", "workTier": "standard" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// The words a refused page operation says.
fn refusal(answer: &Value) -> String {
    assert_ne!(answer["ok"], true, "accepted: {answer}");
    answer.to_string()
}

#[test]
fn a_muse_agent_is_refused_as_a_member_and_as_a_chief_and_no_window_is_made_on_it() -> Outcome {
    let bin = tempfile::tempdir().map_err(|error| cf_e2e::Error::Daemon(error.to_string()))?;
    stand_in(bin.path())?;
    let rig = Rig::start(config().also_on_path(bin.path()))?;
    write_roster(&rig)?;
    // A project with a Muse chief, or a Muse member in its staff, is not opened.
    let chief = rig.page(
        "project.open",
        json!({ "directory": rig.workspace(), "agent": "musey" }),
    )?;
    assert!(refusal(&chief).contains(WORDS), "{chief}");
    let staffed = rig.page(
        "project.open",
        json!({
            "directory": rig.workspace(),
            "agent": "chief",
            "staff": [{ "agent": "musey", "roles": ["worker"] }],
        }),
    )?;
    assert!(refusal(&staffed).contains(WORDS), "{staffed}");
    // Opened on Claude, its staff takes no Muse member, and its chief no switch onto Muse.
    let project = Project::open(&rig, "chief", json!({}))?;
    let member = rig.page(
        "member.add",
        json!({ "project": project.id(), "agent": "musey" }),
    )?;
    assert!(refusal(&member).contains(WORDS), "{member}");
    let switch = rig.page(
        "chief.switch",
        json!({ "project": project.id(), "agent": "musey" }),
    )?;
    assert!(refusal(&switch).contains(WORDS), "{switch}");
    // So no pooled task can reach a Muse member: the staff holds none.
    let board = project.board()?;
    let on_muse = board["lanes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|lane| lane.to_string().contains("musey"))
        .count();
    assert_eq!(on_muse, 0, "{board}");
    assert!(
        !bin.path().join("muse-ran").exists(),
        "the stand-in Muse was run"
    );
    rig.close()?;
    Ok(())
}
