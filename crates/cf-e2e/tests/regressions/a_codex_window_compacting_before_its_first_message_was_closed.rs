//! A Codex window that compacted its thread before it took its first message was closed
//! as one that never showed it, and what it was given reached nobody.
//!
//! Seen on this board on 2026-10-28, on 3.0.6-alpha.2 (T-859, T-901). The tester's
//! window ran Codex GPT-6.1-Sol at xhigh on a long thread; the app was reopened, the
//! window opened again on its thread, and the chief's tell was its first message. Codex
//! took it from the broker and began the turn (`task_started`), then compacted the
//! thread before it wrote the message: "• Compacting context (2m 57s • esc to
//! interrupt) / └ Making room to continue." Three minutes after the hand-over the
//! daemon failed the tell, "the window never showed its first message", and closed the
//! window, which cut the compaction. The chief learned of it from the failure note and
//! sent the words again by hand.
//!
//! What should have happened: a window whose record reads a turn at work, or whose
//! screen says it compacts, is at work, and its first message waits for it: the turn
//! that took it shows it once the compaction is done, and the window is never closed
//! for being slow. The stand-in Codex compacts as Codex 0.165 does at a turn's start
//! (`COMPACT <seconds>`, `fake_codex/agent.rs`): the turn begins, nothing more is written
//! for that long, then the compaction and the message. As in the field the window is
//! opened again on its thread, which has had a turn (Codex compacts only such a thread):
//! T-1 is done and its window closed, and T-2, a follow-up for the same window, is the
//! window's first message. The daemon is given 15 s for a first message, and the
//! compaction takes 30.

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::rollout;
use serde_json::{json, Value};

use crate::{config_with_codex, secs, Outcome};

/// How long the daemon gives a first message to show, in milliseconds.
const LAUNCH_MS: &str = "15000";

/// What the worker answers once its thread is compacted.
const ANSWER: &str = "COMPACTED_OK";

/// The roster of the case: the chief on the stand-in Claude, the worker on the
/// stand-in Codex.
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

/// The worker's windows the daemon asked for, in order.
fn worker_windows(rig: &Rig, project: &Project) -> Vec<Value> {
    let worker = format!("p{}-worker", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| {
            frame["id"]
                .as_str()
                .is_some_and(|id| id.starts_with(&worker))
        })
        .collect()
}

/// How many results the chief was sent.
fn results(project: &Project) -> cf_e2e::Result<usize> {
    Ok(project
        .inbox("chief")?
        .iter()
        .filter(|message| message["kind"] == "result")
        .count())
}

#[test]
fn a_codex_window_compacting_past_its_first_message_s_time_takes_it_once_it_is_done() -> Outcome {
    let rig = Rig::start(config_with_codex().var("CONSENSFLOW_LAUNCH_TIMEOUT_MS", LAUNCH_MS))?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or("standard")
        .to_owned();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    // T-1 gives the worker's thread a turn, and its window closes with it.
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} Reply with exactly: FIRST_OK"),
    )?;
    rig.wait_for("T-1's result", secs(90), || Ok(results(&project)? >= 1))?;
    rig.wait_for("the worker's first window to close", secs(60), || {
        let assignee = project.task(1)?["assignee"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        Ok(project.lane(&assignee)?["activity"]["state"] == "closed")
    })?;

    // T-2, a follow-up for the same window, opens it again on its thread with T-2 as its
    // first message, and the thread compacts for twice the time the daemon gives a first
    // message before it takes it.
    let ran = rig.cf_in_window(
        &chief,
        [
            "task",
            "add",
            "--after",
            "T-1",
            &format!("COMPACT 30 Reply with exactly: {ANSWER}"),
        ],
    )?;
    assert_eq!(ran.code, Some(0), "{ran}");
    rig.wait_for("T-2's result", secs(120), || Ok(results(&project)? >= 2))?;

    let task = project.task(2)?;
    assert_eq!(task["state"], "done", "{task}");
    let failed: Vec<_> = rig
        .events()
        .into_iter()
        .filter(|event| event["kind"] == "delivery.failed" || event["kind"] == "delivery.retried")
        .collect();
    assert!(
        failed.is_empty(),
        "nothing failed or went again: {failed:?}"
    );
    assert_eq!(
        worker_windows(&rig, &project).len(),
        2,
        "T-1's window and T-2's, which was kept"
    );
    let home = std::path::PathBuf::from(rig.var("CODEX_HOME").unwrap_or_default());
    let mut briefs = 0;
    for found in rollout::rollouts(&home) {
        briefs += rollout::said_by(&rollout::read(&found)?, "user")
            .iter()
            .filter(|said| said.contains(&format!("Reply with exactly: {ANSWER}")))
            .count();
    }
    assert_eq!(briefs, 1, "the thread took T-2 once");
    rig.close()?;
    Ok(())
}
