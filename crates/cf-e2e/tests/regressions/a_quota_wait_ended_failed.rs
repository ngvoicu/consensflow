//! A worker out of quota mid-task ended failed, though it was told it waits.
//!
//! Seen by the owner of dip-agentos between 2026-10-17 and 19, on 3.0.4: T-17 and
//! T-20 ran into their harness's quota. The chief was told each "waits … out of
//! quota until 09:10, … it goes on by itself", and then each ended "failed: the
//! harness reported a failure", its work left uncommitted in its window. The
//! report is row 3 of `issues/field-reports-2026-10-19-dip-agentos.md` in
//! consensflow-sme; the project is not on this Mac, so its records were not read.
//!
//! Codex ends a turn its usage limit cut with `task_complete` whose error is
//! `usage_limit_exceeded` (Codex 0.163 names it so beside the `server_overloaded`
//! of a real rollout), and its rate limits need not say the limit was reached:
//! ConsensFlow read the turn as failed for some other reason, and failed the task.
//! A Codex worker is refused here mid-task in that shape (it works three seconds
//! on the brief first).
//!
//! What the owner should have seen: the task waits with its window, the chief told
//! so once; when the human says the member is back, the same window is given the
//! words that resume it and goes on; the chief is told it goes on, and is given the
//! result. The task never fails.

use std::path::PathBuf;

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::{config_with_codex, secs, Outcome};

/// The brief the worker's usage limit cuts, three seconds into its turn.
const BRIEF: &str = "SLEEP 3 QUOTA-OUT Reply with exactly: CODEX_OK";

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

/// The notes ConsensFlow wrote the chief, oldest first.
fn notes(project: &Project) -> cf_e2e::Result<Vec<Value>> {
    let mut notes: Vec<Value> = project
        .inbox("chief")?
        .into_iter()
        .filter(|m| m["kind"] == "note" && m.get("sender") == Some(&Value::Null))
        .collect();
    notes.sort_by_key(|m| m["id"].as_i64().unwrap_or_default());
    Ok(notes)
}

/// The notes ConsensFlow wrote the chief that begin with `start`.
fn notes_starting(project: &Project, start: &str) -> cf_e2e::Result<Vec<Value>> {
    Ok(notes(project)?
        .into_iter()
        .filter(|m| {
            m["body"]
                .as_str()
                .is_some_and(|body| body.starts_with(start))
        })
        .collect())
}

/// The account the worker's harness refuses while its file is there.
fn account() -> cf_e2e::Result<(TempDir, PathBuf)> {
    let folder = tempfile::Builder::new()
        .prefix("cf-account-")
        .tempdir()
        .map_err(|source| cf_e2e::Error::File {
            action: "make a folder in",
            path: std::env::temp_dir(),
            source,
        })?;
    let file = folder.path().join("refused");
    files::write(&file, "")?;
    Ok((folder, file))
}

#[test]
fn a_codex_worker_cut_by_its_usage_limit_mid_task_waits_with_its_window_and_goes_on_there_never_failed(
) -> Outcome {
    let (_folder, refused) = account()?;
    let rig = Rig::start(
        config_with_codex()
            .var("CF_TEST_QUOTA_OUT", "worker")
            .var("CF_TEST_QUOTA_FILE", refused.to_string_lossy()),
    )?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"].as_str().unwrap_or("standard");
    rig.tell(project.id(), &format!("DISPATCH --tier {tier} {BRIEF}"))?;

    rig.wait_for("T-1 to wait, or to fail", secs(90), || {
        let state = project.task(1)?["state"].clone();
        Ok(state == "paused" || state == "failed")
    })?;
    let held = project.task(1)?;
    assert_eq!(
        held["state"],
        "paused",
        "the usage limit is a wait, not a failure: {held} {:?}",
        notes(&project)?
    );
    let session = held["assignee"].as_str().unwrap_or_default().to_owned();
    assert!(session.starts_with("worker-"), "{held}");
    let waits = format!("T-1 waits with @{session}: out of quota until ");
    assert_eq!(
        notes_starting(&project, &waits)?.len(),
        1,
        "{:?}",
        notes(&project)?
    );
    rig.wait_for(
        "the chief to be given the note that T-1 waits",
        secs(60),
        || Ok(notes_starting(&project, &waits)?[0]["state"] == "delivered"),
    )?;

    // The human logs the harness into an account with quota, and says so.
    let _ = std::fs::remove_file(&refused);
    let back = rig.page(
        "member.back",
        json!({ "project": project.id(), "participant": "worker" }),
    )?;
    assert_eq!(back["ok"], true, "{back}");
    // The stand-in remembers nothing of its brief: it answers the words that resume it.
    rig.wait_for("the result to reach the chief", secs(90), || {
        Ok(project
            .inbox("chief")?
            .iter()
            .any(|m| m["kind"] == "result" && m["state"] == "delivered"))
    })?;
    let done = project.task(1)?;
    assert_eq!(
        done["assignee"].as_str(),
        Some(session.as_str()),
        "the same window went on"
    );
    let inbox = project.inbox(&session)?;
    assert!(
        inbox
            .iter()
            .any(|m| m["body"] == "Resumed: Go on where you stopped." && m["state"] == "delivered"),
        "{inbox:?}"
    );
    assert_eq!(
        notes_starting(&project, "T-1 goes on")?.len(),
        1,
        "{:?}",
        notes(&project)?
    );
    assert_eq!(notes_starting(&project, &waits)?.len(), 1, "it waited once");
    let told = notes(&project)?;
    assert!(
        told.iter()
            .all(|note| !note["body"].as_str().unwrap_or_default().contains("failed")),
        "{told:?}"
    );
    assert_ne!(done["state"], "failed", "{done}");
    rig.close()?;
    Ok(())
}
