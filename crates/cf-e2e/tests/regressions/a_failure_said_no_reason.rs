//! A worker's failed turn failed its task with no reason.
//!
//! Seen by the owner of dip-agentos between 2026-10-17 and 19, on 3.0.4: each
//! failure said only "the harness reported a failure", and neither the chief nor
//! the human could tell a crash from a refusal from a bad model name. The report
//! is row 4 of `issues/field-reports-2026-10-19-dip-agentos.md` in consensflow-sme;
//! the project is not on this Mac, so its records were not read.
//!
//! Codex ends a turn its model could not serve with `task_complete` whose error
//! says why (`server_overloaded` in a real rollout, with the error's words). A Codex
//! worker fails here in that shape, three seconds into its brief, with words of
//! the stand-in's own.
//!
//! What the owner should have seen: T-1 fails, and the note that says so, to the
//! chief, quotes what Codex said.

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::{config_with_codex, secs, Outcome};

/// The brief whose turn fails, three seconds in.
const BRIEF: &str = "SLEEP 3 MODEL-BUSY Reply with exactly: CODEX_OK";

/// What the stand-in Codex says of the turn it failed.
const SAID: &str = "The stand-in model is busy. Pick another model.";

/// The roster of the case: the chief on the stand-in Claude, the worker on Codex.
fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": "codex", "model": "fake", "workTier": "standard" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// The notes ConsensFlow wrote the chief that say T-1 failed.
fn failed_notes(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter(|m| m["kind"] == "note" && m.get("sender") == Some(&Value::Null))
        .filter_map(|m| m["body"].as_str().map(str::to_owned))
        .filter(|body| body.starts_with("T-1 failed: "))
        .collect())
}

#[test]
fn a_codex_worker_whose_turn_failed_fails_its_task_with_what_codex_said() -> Outcome {
    let rig = Rig::start(config_with_codex())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or("standard");
    rig.tell(project.id(), &format!("DISPATCH --tier {tier} {BRIEF}"))?;

    rig.wait_for("T-1 to fail", secs(90), || {
        Ok(project.task(1)?["state"] == "failed")
    })?;
    let task = project.task(1)?;
    let session = task["assignee"].as_str().unwrap_or_default().to_owned();
    assert!(session.starts_with("worker-"), "{task}");
    rig.wait_for("the chief to be told T-1 failed", secs(30), || {
        Ok(!failed_notes(&project)?.is_empty())
    })?;
    assert_eq!(
        failed_notes(&project)?,
        [format!(
            "T-1 failed: @{session}'s harness reported a failure; it said: \"{SAID}\". Reopen it with: cf task reopen T-1 \"…\""
        )]
    );
    rig.close()?;
    Ok(())
}
