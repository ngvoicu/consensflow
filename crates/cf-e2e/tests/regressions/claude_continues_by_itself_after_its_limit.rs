//! Claude Code continues by itself once its limit resets, beside ConsensFlow's resume.
//!
//! Found in the review of the quota wait (T-88, 2026-10-21), from this Mac's Claude
//! transcripts: from 2.1.278, a refused window writes "Usage limit reached ·
//! continuing automatically at …", and 36 to 94 s after the named reset it sends
//! itself a meta prompt ("Your claude.ai usage limit has reset. Continue the task
//! you were working on.") and works on. ConsensFlow resumes a held task too, with
//! "Resumed: Go on where you stopped.", on its first pass after the hold. The two
//! may race: Claude first (its account had quota again before the hold's end), or
//! ConsensFlow first and Claude's timer after it.
//!
//! The stand-in Claude continues by itself as Claude does (`CF_TEST_AUTO_CONTINUE_MS`
//! after its account has quota again), and each case asserts what the owner should
//! see: the task never fails, its result reaches the chief, and nothing of it is
//! left in flight. What the cases found (T-95):
//! - Claude first: the turn it continued by itself got through, so the member was
//!   back and the held task resumed with the words that resume it, pasted after
//!   that turn; their answer was the result, and the report of the turn Claude
//!   continued by itself reached the chief in no result. The chief's rule (T-95):
//!   that turn is the task going on, with no words pasted, and its answer is the
//!   result.
//! - ConsensFlow first: its words resume the task, their answer is the one
//!   result, and the task is over before Claude's timer fires; nothing is doubled
//!   in what the chief is given.

use std::path::PathBuf;

use cf_e2e::files;
use cf_e2e::rig::{write_staff, Project, Rig, OPEN};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::{config, secs, Outcome};

/// What the stand-in's meta prompt says, which its answer quotes.
const CONTINUED: &str = "Your claude.ai usage limit has reset.";

/// The brief the worker's provider refuses while its account is the refused one.
const BRIEF: &str = "QUOTA-OUT Reply with exactly: WORKER_OK";

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

/// A project whose worker is refused on its brief and continues by itself `after`
/// ms once its account has quota again; its task held, and the chief given the
/// note that it waits.
fn held<'a>(rig: &'a Rig) -> cf_e2e::Result<Project<'a>> {
    write_staff(rig)?;
    let project = Project::open_with_staff(rig, "chief", None, &[("worker", "worker")])?;
    let tier = project.tiers()?.remove("worker").unwrap_or_default();
    rig.tell(project.id(), &format!("DISPATCH --tier {tier} {BRIEF}"))?;
    rig.wait_for("T-1 to wait", secs(60), || {
        Ok(project.task(1)?["state"] == "paused")
    })?;
    rig.wait_for(
        "the chief to be given the note that T-1 waits",
        secs(60),
        || {
            Ok(project.inbox("chief")?.iter().any(|m| {
                m["kind"] == "note"
                    && m["state"] == "delivered"
                    && m["body"]
                        .as_str()
                        .unwrap_or_default()
                        .starts_with("T-1 waits")
            }))
        },
    )?;
    Ok(project)
}

/// The results the chief was given, oldest first.
fn results(project: &Project) -> cf_e2e::Result<Vec<Value>> {
    let mut results: Vec<Value> = project
        .inbox("chief")?
        .into_iter()
        .filter(|m| m["kind"] == "result" && m["state"] == "delivered")
        .collect();
    results.sort_by_key(|m| m["id"].as_i64().unwrap_or_default());
    Ok(results)
}

/// A project whose one account is spent for every window on it, the chief's too, with
/// T-1 given by the chief's own `cf` (no turn of the chief's) and refused, held, and
/// the note that it waits given the chief, whose turn on it is refused as well.
fn held_on_a_spent_account<'a>(rig: &'a Rig) -> cf_e2e::Result<Project<'a>> {
    write_staff(rig)?;
    let project = Project::open_with_staff(rig, "chief", None, &[("worker", "worker")])?;
    let tier = project.tiers()?.remove("worker").unwrap_or_default();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let ran = rig.cf_in_window(&chief, ["task", "add", "--tier", &tier, BRIEF])?;
    if ran.code != Some(0) {
        return Err(cf_e2e::Error::Daemon(format!("cf task add: {ran}")));
    }
    rig.wait_for("T-1 to wait", secs(60), || {
        Ok(project.task(1)?["state"] == "paused")
    })?;
    rig.wait_for("the chief to be out of quota too", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "out")
    })?;
    Ok(project)
}

#[test]
fn claude_continuing_before_consensflow_resumes_the_task_its_report_is_the_result() -> Outcome {
    // Claude Code's windows share one account, and a turn of any of them that gets
    // through brings the others back (T-295): the chief's answer to the note that T-1
    // waits brought the worker back, and ConsensFlow resumed it first (the merge with
    // quota-queued, T-531). Claude goes first where no other window on the account
    // gets a turn through before it: the account is spent for every window, the
    // chief's too, and the worker's timer is the one that fires.
    let (_folder, refused) = account()?;
    let rig = Rig::start(
        config()
            .var("CF_TEST_QUOTA_SPENT", "worker,chief")
            .var("CF_TEST_QUOTA_FILE", refused.to_string_lossy())
            .var("CF_TEST_AUTO_CONTINUE_MS", "worker 1000"),
    )?;
    let project = held_on_a_spent_account(&rig)?;
    // The account has quota again long before the reset the refusal named: Claude
    // continues by itself, and ConsensFlow has not resumed the task.
    let _ = std::fs::remove_file(&refused);
    rig.wait_for("a result to reach the chief", secs(90), || {
        Ok(!results(&project)?.is_empty())
    })?;
    std::thread::sleep(std::time::Duration::from_secs(5));
    let task = project.task(1)?;
    assert_ne!(task["state"], "failed", "{task}");
    assert!(
        task["state"] != "working" && task["state"] != "queued",
        "nothing left in flight: {task}"
    );
    let given = results(&project)?;
    assert_eq!(given.len(), 1, "one result: {given:?}");
    assert!(
        given[0]["body"]
            .as_str()
            .unwrap_or_default()
            .contains(CONTINUED),
        "the turn Claude continued by itself is the report: {given:?}"
    );
    let session = task["assignee"].as_str().unwrap_or_default();
    let resumed: Vec<Value> = project
        .inbox(session)?
        .into_iter()
        .filter(|m| {
            m["body"]
                .as_str()
                .unwrap_or_default()
                .starts_with("Resumed:")
        })
        .collect();
    assert!(
        resumed.iter().all(|m| m["state"] != "delivered"),
        "no words went in after its own turn: {resumed:?}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn consensflow_resuming_before_claude_continues_the_task_ends_once_with_its_result() -> Outcome {
    let (_folder, refused) = account()?;
    let rig = Rig::start(
        config()
            .var("CF_TEST_QUOTA_OUT", "worker")
            .var("CF_TEST_QUOTA_FILE", refused.to_string_lossy())
            .var("CF_TEST_AUTO_CONTINUE_MS", "8000"),
    )?;
    let project = held(&rig)?;
    // The human says the member is back: ConsensFlow resumes at once, and Claude's
    // own timer fires eight seconds later.
    let _ = std::fs::remove_file(&refused);
    let back = rig.page(
        "member.back",
        json!({ "project": project.id(), "participant": "worker" }),
    )?;
    assert_eq!(back["ok"], true, "{back}");
    rig.wait_for("a result to reach the chief", secs(90), || {
        Ok(!results(&project)?.is_empty())
    })?;
    // Past Claude's timer, and a pass or two.
    std::thread::sleep(std::time::Duration::from_secs(15));
    let task = project.task(1)?;
    assert_ne!(task["state"], "failed", "{task}");
    assert!(
        task["state"] != "working" && task["state"] != "queued",
        "nothing left in flight: {task}"
    );
    let given = results(&project)?;
    assert_eq!(given.len(), 1, "one result: {given:?}");
    rig.close()?;
    Ok(())
}
