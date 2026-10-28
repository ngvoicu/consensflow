//! A Devin worker's answer from the chief goes in after Devin compacted the turn that
//! asked (T-655, from the live board's T-602 of 2026-10-26, read in T-652). The worker
//! asks the chief with `cf ask`, and its task waits; Devin's async compactor then
//! begins a chain of its own below a summary, the turn's steps copied below it, and the
//! user's message the turn answers is left outside the main chain. The turn's answer is
//! stored as Devin 3000.11 stores one, with no `finish_reason`, in a model request of its
//! own (`metadata.request_id`), and the wire ends the turn naming that request
//! (`turnRequestId`, `stats.requestId`) beside the cut message's client id. Read by the
//! cut message's client id alone, the answer was never complete and the window never at
//! rest: the chief's answer stayed queued with no attempt, for good.
//!
//! The answer is given once, into the same window (no Escape, no window closed or opened
//! again), its receipt proves it arrived, and the task goes from waiting to working.

use cf_e2e::files;
use cf_e2e::rig::{Config, Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The stand-in for Devin, built for these tests.
const FAKE_DEVIN: &str = env!("CARGO_BIN_EXE_fake-devin");

/// A rig whose Devin windows are the stand-in's.
fn devin_rig() -> Config {
    config().devin(FAKE_DEVIN)
}

/// The roster of the case: the chief on the stand-in `claude`, and a Devin worker.
fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "devin-worker", "kind": "devin", "model": "fake", "workTier": "standard" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// The messages of `participant`'s inbox of the kind `kind`.
fn of_kind(project: &Project, participant: &str, kind: &str) -> cf_e2e::Result<Vec<Value>> {
    Ok(project
        .inbox(participant)?
        .into_iter()
        .filter(|message| message["kind"] == kind)
        .collect())
}

/// The moves of the tasks' states the ledger logged, each from and to, oldest first.
fn task_moves(rig: &Rig) -> Vec<(Value, Value)> {
    rig.events()
        .into_iter()
        .filter(|event| event["kind"] == "task.state")
        .map(|event| (event["data"]["from"].clone(), event["data"]["to"].clone()))
        .collect()
}

#[test]
fn a_devin_workers_answer_goes_in_once_into_the_same_window_after_devin_compacted_the_turn_that_asked(
) -> Outcome {
    answered_once("COMPACT ")
}

/// The case's control: the same turn, which Devin did not compact.
#[test]
fn a_devin_workers_answer_goes_in_once_into_the_same_window_after_a_turn_that_asked() -> Outcome {
    answered_once("")
}

/// The worker's turn asks the chief, compacted before it answers where `compact` is the
/// stand-in's `COMPACT `: the chief's answer goes in once, into the same window, and the
/// task goes on.
fn answered_once(compact: &str) -> Outcome {
    let rig = Rig::start(devin_rig())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    project.add_member("devin-worker")?;
    // The turn works long enough for its brief to be proved arrived, then asks, and is
    // compacted before it answers: T-602's turn had run 81 steps when it asked. The
    // stand-in chief answers a question that says `REPLY <words>` with `cf answer`, and
    // the answer is a turn that works long enough to look at the window while it does.
    rig.tell(
        id,
        &format!(
            "DISPATCH --tier standard SLEEP 5 {compact}CF ask :: Which colour? REPLY SLEEP 20 Reply with exactly: BLUE_DONE"
        ),
    )?;
    rig.wait_for("the worker's question to reach the chief", secs(60), || {
        Ok(!of_kind(&project, "chief", "question")?.is_empty())
    })?;
    let brief = project.task(1)?["messages"][0].clone();
    assert_eq!(
        brief["state"], "delivered",
        "the brief arrived first: {brief}"
    );
    let worker = project.lane("devin-worker")?["participant"]["handle"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let pane = format!("p{id}-{worker}");
    rig.wait_for("the chief's answer to be on the board", secs(60), || {
        Ok(!of_kind(&project, &worker, "answer")?.is_empty())
    })?;

    let answer = || -> cf_e2e::Result<Value> {
        Ok(of_kind(&project, &worker, "answer")?
            .into_iter()
            .next()
            .unwrap_or_default())
    };
    let delivered = rig.wait_for(
        "the chief's answer to be given to the worker",
        secs(60),
        || Ok(answer()?["state"] == "delivered"),
    );
    if let Err(failed) = delivered {
        let lane = project.lane("devin-worker")?;
        return Err(format!(
            "{failed}\nthe answer: {}\nT-1: {}\nthe worker's lane: {}",
            answer()?,
            project.task(1)?,
            lane["activity"],
        )
        .into());
    }
    let answer = answer()?;
    assert_eq!(answer["attempts"], 1, "given once: {answer}");
    assert!(
        answer["receipt"].is_object()
            && !answer["receipt"].as_object().is_some_and(|r| r.is_empty()),
        "its receipt proves it arrived: {answer}"
    );
    rig.wait_for("T-1 to go on once its answer is in", secs(60), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    let moved = task_moves(&rig);
    assert!(
        moved.contains(&(json!("waiting"), json!("working"))),
        "T-1 went from waiting to working: {moved:?}"
    );
    // The same window took it: opened once, never closed, never pressed.
    let opened = rig
        .open_frames()
        .iter()
        .filter(|frame| frame["id"] == pane.as_str())
        .count();
    assert_eq!(opened, 1, "the worker's window was opened once");
    assert!(
        rig.kills().iter().all(|kill| kill["id"] != pane.as_str()),
        "the worker's window was not closed: {:?}",
        rig.kills()
    );
    assert!(
        rig.exits().iter().all(|exit| exit["id"] != pane.as_str()),
        "the worker's window did not end: {:?}",
        rig.exits()
    );
    assert_eq!(rig.keys_pressed(&pane, &[27]), 0, "no Escape");
    // And the answer to it is the task's result.
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.close()?;
    Ok(())
}

#[test]
fn a_devin_workers_result_is_taken_after_devin_compacted_the_turn_that_gives_it() -> Outcome {
    done_after("SLEEP 5 COMPACT Reply with exactly: T1_DONE")
}

/// The compaction comes before the daemon has looked at the window since it gave the
/// brief: the brief is proved arrived by the user's message the compaction cut out.
#[test]
fn a_devin_workers_brief_is_proved_arrived_though_devin_compacted_its_turn_before_the_daemon_looked(
) -> Outcome {
    done_after("COMPACT Reply with exactly: T1_DONE")
}

/// A Devin worker given the task `task`, a turn Devin compacts before it answers: the
/// brief is proved arrived, and its answer is the task's result.
fn done_after(task: &str) -> Outcome {
    let rig = Rig::start(devin_rig())?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    project.add_member("devin-worker")?;
    rig.tell(project.id(), &format!("DISPATCH --tier standard {task}"))?;
    let done = rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    });
    if let Err(failed) = done {
        return Err(format!(
            "{failed}\nT-1: {}\nthe worker's lane: {}",
            project.task(1)?,
            project.lane("devin-worker")?["activity"]
        )
        .into());
    }
    let task = project.task(1)?;
    let brief = &task["messages"][0];
    assert_eq!(brief["state"], "delivered", "{brief}");
    assert_eq!(brief["attempts"], 1, "given once: {brief}");
    let result = of_kind(&project, "chief", "result")?;
    assert!(
        result.iter().any(|result| result["body"]
            .as_str()
            .is_some_and(|body| body.contains("T1_DONE"))),
        "its answer is the result: {result:?}"
    );
    rig.close()?;
    Ok(())
}
