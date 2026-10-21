//! What the pane host said last of its windows as a rig closes: every exit it told
//! before it ended, read to its end, each with when the rig saw it, and the root kept.
//! The long run of the evals reads it after the close, so that a window that ended
//! between its last look and the close is taken with what its close ended.
//!
//! (The host tells no exit of the windows it reaps as it shuts down: its stream
//! threads send `pane.exit` after the bridge has closed. What it does tell before its
//! end is all a case can read.)

use cf_e2e::process::{signal, Signal};
use cf_e2e::rig::{Project, OPEN};
use serde_json::json;

use crate::{rig, secs, Outcome};

#[test]
fn the_last_word_of_a_closed_rig_holds_every_exit_the_pane_host_told() -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let chief = format!("p{}-chief", project.id());
    rig.open_frame(&chief, OPEN)?;
    rig.wait_for("the chief's window to run", secs(10), || {
        Ok(rig
            .window_pids()
            .iter()
            .any(|window| window["id"] == chief.as_str()))
    })?;
    let pid = rig
        .window_pids()
        .iter()
        .find(|window| window["id"] == chief.as_str())
        .and_then(|window| window["pid"].as_u64())
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or("the chief's window has no process")?;
    // The window ends, as a crash ends it, and its exit is told.
    signal(pid, Signal::Kill)?;
    rig.wait_for("the chief's window to exit", secs(10), || {
        Ok(rig.exits().iter().any(|exit| exit["id"] == chief.as_str()))
    })?;

    let last = rig.close_with_the_last_exits_within(secs(30))?;

    assert!(
        last.exits
            .iter()
            .any(|(exit, at)| exit["id"] == chief.as_str() && *at > 0),
        "{:?}",
        last.exits
    );
    assert!(last.root.is_dir(), "the root is kept");
    std::fs::remove_dir_all(&last.root)?;
    Ok(())
}
