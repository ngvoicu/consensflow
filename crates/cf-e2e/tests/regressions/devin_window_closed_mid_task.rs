//! T-18, 2026-10-15, ConsensFlow 3.0.3 or 3.0.4 (which, the owner does not know) on
//! the owner's Windows machine: **a worker's window closed in the middle of its task,
//! and nothing said why.** B2 of `issues/field-reports-2026-10-15.md` in the brain.
//!
//! What the owner saw: T-18 paused, its worker's window closed. The worker was very
//! likely Devin. The pause note said "@…'s window closed" and no more, and there were
//! no logs: nothing told whether the window's program had crashed, exited by itself or
//! been ended, nor what its screen showed last. Resuming the task worked (its next
//! window had the trouble of B1).
//!
//! What should have happened: ConsensFlow cannot keep a harness from crashing under its
//! task, but it knows how the window's program ended and what its screen showed last:
//! the pane host sends both with the window's exit, and the failure of a launch quotes
//! them already. The pause note says them, so that the next report shows the cause:
//! `T-18 is paused: @…'s window closed (exit code 3); its screen ended with: "…"`.
//!
//! The stand-in Devin (`fake-devin`) works on its task for a while, as Devin does, and
//! then says its last words on its screen and ends with the code 3, in the middle of
//! the turn (`EXIT`: no answer, no end of the turn on the wire), as a harness that
//! crashes does. Which of the ways a window can end ended the owner's is not known;
//! this is the part of the report that is ConsensFlow's: what the chief, who gave the
//! task, is told of it. It runs wherever the rig does, Windows among them, where the
//! window's end is its program's exit (the pane's job object ends what is left) and
//! the screen is what the pseudoconsole passed on before it.

use cf_e2e::rig::{Project, Rig};
use serde_json::json;

use crate::devin_pause::write_roster;
use crate::{config, secs, Outcome, FAKE_DEVIN};

/// What the stand-in Devin says last on its screen, before its program ends.
const LAST_WORDS: &str = "fatal: the agent runtime went away";

/// The chief's note that says T-1 is paused, once there is one.
fn pause_note(project: &Project) -> cf_e2e::Result<Option<String>> {
    Ok(project
        .inbox("chief")?
        .iter()
        .filter(|message| message["kind"] == "note")
        .filter_map(|message| message["body"].as_str())
        .find(|body| body.starts_with("T-1 is paused:"))
        .map(str::to_owned))
}

#[test]
fn a_devin_window_that_ends_in_the_middle_of_its_task_pauses_it_with_how_it_ended_and_what_it_showed_last(
) -> Outcome {
    let rig = Rig::start(config().devin(FAKE_DEVIN))?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    project.add_member("devin-worker")?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier standard SLEEP 10 EXIT 3 {LAST_WORDS}"),
    )?;
    rig.wait_for("the Devin window to be at work on T-1", secs(60), || {
        Ok(project.lane("devin-worker")?["activity"]["state"] == "working")
    })?;
    let lane = project.lane("devin-worker")?;
    let handle = lane["participant"]["handle"]
        .as_str()
        .ok_or("the worker has no handle")?
        .to_owned();

    // Its program ends in the middle of the turn: the task is paused, not failed.
    rig.wait_for("T-1 to be paused", secs(60), || {
        Ok(project.task(1)?["state"] != "working")
    })?;
    assert_eq!(project.task(1)?["state"], "paused");

    // The chief is told how the window ended and what it showed last.
    rig.wait_for("the chief to be told T-1 is paused", secs(30), || {
        Ok(pause_note(&project)?.is_some())
    })?;
    let note = pause_note(&project)?.unwrap_or_default();
    let opening = format!(
        "T-1 is paused: @{handle}'s window closed (exit code 3); its screen ended with: \""
    );
    let closing = format!(
        "{LAST_WORDS}\". Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
    );
    assert!(
        note.starts_with(&opening) && note.ends_with(&closing),
        "the chief was told: {note}"
    );
    rig.close()?;
    Ok(())
}
