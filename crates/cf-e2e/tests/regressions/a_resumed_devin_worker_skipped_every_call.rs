//! The field report B1 of 2026-10-15, ConsensFlow 3.0.4 on a Windows machine:
//! **a resumed Devin worker had every call skipped.**
//!
//! What the owner saw: a Devin worker's window closed with the app while it was on
//! its task; the human resumed the project, and the worker, given the note that its
//! turn was cut, had every write, shell and `cf` call come back "Tool execution was
//! rejected: User skipped this tool call", and only reading worked. The report is the
//! issue of B1 in consensflow-sme.
//!
//! Why (`cargo xtask live devin-resume`, Devin 3000.11.92 on the Mac, the same day):
//! Devin's `--resume` of a session whose first turn the quit cut drops
//! `--permission-mode dangerous` and brings the window back in its normal mode, where
//! the turn of the prompt file (how the daemon gives a resumed worker the note) has
//! each call refused at once, in those words, with no key pressed and no hook of
//! anybody's.
//!
//! What should have happened: the window comes back in full permission, and the
//! worker goes on with its task, its calls running; its result is what it did.
//!
//! The stand-in Devin (`fake-devin`) does what Devin did: a session resumed after a
//! quit cut its first turn comes back in the normal mode, whatever the flag says, and
//! the prompt file's calls are refused; `/bypass`, Devin's command, switches it. The
//! task's brief has the turn work long before its tool makes a file (`SLEEP`, `FILE`),
//! so the quit cuts it before any call; the note has the stand-in go on with the turn
//! the quit cut, as a model does. The quit is the app's: the daemon and the pane host
//! end, and the pane host ends every window. The app starts again on the same home,
//! and the human resumes the project. The case is the same on every platform: the
//! stand-in's tool makes a file.

use std::path::Path;

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use serde_json::json;

use crate::devin_pause::write_roster;
use crate::{config, secs, Outcome, FAKE_DEVIN};

/// What Devin says of a call it refused for its permission.
const SKIPPED: &str = "User skipped this tool call";

/// The file the worker's tool makes, in the project's folder, and what it holds.
const MADE: &str = "resumed.txt";
const MADE_TEXT: &str = "made by the turn's tool\n";

#[test]
fn a_devin_worker_cut_in_its_first_turn_and_resumed_goes_on_with_its_calls_running() -> Outcome {
    let rig = Rig::start(config().devin(FAKE_DEVIN))?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    project.add_member("devin-worker")?;
    rig.tell(
        id,
        &format!("DISPATCH --tier standard SLEEP 600 FILE {MADE} Reply with exactly: T1_DONE"),
    )?;
    rig.wait_for("the Devin worker to be at work on T-1", secs(60), || {
        Ok(project.lane("devin-worker")?["activity"]["state"] == "working")
    })?;
    // Its first turn is in the middle of its work, and has called no tool yet.
    std::thread::sleep(secs(2));
    let workspace = rig.workspace().to_path_buf();
    assert!(!workspace.join(MADE).exists(), "no call before the quit");

    // The app quits, with the worker's turn at work, and starts again on the same home.
    let root = rig.close_keeping_the_root()?;
    let config = config().devin(FAKE_DEVIN);
    let rig = Rig::start(config.existing_root(root))?;
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let project = Project::new(&rig, id);

    rig.wait_for("the worker to answer T-1 again", secs(90), || {
        Ok(project.task(1)?["state"] != "working")
    })?;
    let lane = project.lane("devin-worker")?;
    let screen = lane["pane"]["id"]
        .as_str()
        .map(|pane| rig.screen(pane))
        .unwrap_or_default();
    let made = files::read_string(&workspace.join(MADE)).unwrap_or_default();
    assert!(
        made == MADE_TEXT && !screen.contains(SKIPPED),
        "the worker's resumed turn did not run its call ({MADE} {}); its screen: {}",
        if Path::new(&workspace.join(MADE)).exists() {
            "holds something else"
        } else {
            "was never made"
        },
        screen.trim_end()
    );
    let state = project.task(1)?["state"].clone();
    assert!(state == "done" || state == "accepted", "T-1 is {state}");
    rig.close()?;
    Ok(())
}
