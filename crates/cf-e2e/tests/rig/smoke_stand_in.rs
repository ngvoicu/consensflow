//! The packaged smoke's stand-in harness on the checkout's daemon and pane host
//! (T-863). The smoke runs it only in a built app, which no gate builds: 3.0.6-alpha.3
//! took a window's first message off its command line, to paste once its prompt
//! shows (T-432, T-523), and the stand-in, which drew no prompt and read its brief
//! from its command line, was never handed one. Every gate passed, and the smoke
//! failed on the app. Here the board case of the smoke runs on the script itself,
//! as the app's page runs it: a project on it, `HANDOFF` typed into the chief's
//! window, which puts a task on the board, and the worker's result back with the
//! chief. What the daemon asks of a Claude window before it hands it a message is
//! asked of the stand-in here on every gate.

use std::path::Path;

use cf_e2e::files;
use cf_e2e::rig::{Config, Project, Rig};
use serde_json::{json, Value};

use crate::{secs, Outcome};

/// The flood is never asked for here: its size is the least the script takes.
const FLOOD: usize = 1;

/// The smoke's stand-in written as `claude` in `folder`, where the rig's windows
/// run it.
fn install(folder: &Path) -> cf_e2e::Result<std::path::PathBuf> {
    let claude = folder.join("claude");
    files::write_executable(&claude, cf_e2e::smoke_harness::script(FLOOD, FLOOD))?;
    Ok(claude)
}

/// The result of T-`number` for the chief, once there is one.
fn result_for(project: &Project, number: i64) -> cf_e2e::Result<Option<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "result" && message["taskNumber"] == number))
}

#[test]
fn the_smokes_stand_in_takes_its_brief_and_its_result_reaches_the_chief() -> Outcome {
    let folder = tempfile::tempdir()?;
    let claude = install(folder.path())?;
    let pids = folder.path().join("pids");
    let rig = Rig::start(
        Config::new(claude)
            .judged_by_outcome()
            .var("CFSMOKE_PIDFILE", pids.to_string_lossy())
            .var("CFSMOKE_TAG", "rig")
            .var("CFSMOKE_PASTE_READER", "/nonexistent"),
    )?;
    let project = Project::open(
        &rig,
        "chief",
        json!({ "staff": [{ "agent": "worker", "roles": ["worker"] }] }),
    )?;
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), secs(30))?;
    let id = chief["id"].as_str().unwrap_or_default().to_owned();
    rig.wait_for("the chief's window to draw its banner", secs(30), || {
        Ok(rig.screen(&id).contains("CFSMOKE-READY rig"))
    })?;
    // Typed as the page types it: the line, then the return.
    let typed = rig.host(
        "pane.input",
        json!({ "id": id, "generation": chief["generation"], "bytes": b"HANDOFF\r" }),
    )?;
    assert_eq!(typed["ok"], true, "{typed}");
    rig.wait_for("T-1's result to reach the chief", secs(60), || {
        Ok(result_for(&project, 1)?.is_some_and(|result| result["state"] == "delivered"))
    })?;
    assert_eq!(project.task(1)?["state"], "done");
    rig.close()?;
    Ok(())
}
