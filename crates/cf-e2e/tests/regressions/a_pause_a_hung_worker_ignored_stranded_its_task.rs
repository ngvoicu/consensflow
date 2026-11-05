//! A pause that a hung worker ignored stranded its task.
//!
//! Seen on zeewin on 2026-10-19, in the two-hour live long run of the 3.0.5
//! candidate (main with B5 and T-442's fix), Pi 1.3.0 on Windows under a Codex
//! chief. The worker's Pi was hung in a model request (its session's last write
//! was its turn's tool results, nine minutes before) when the run paused its task,
//! T-2: Pi ignored the pause's Escape and never settled, so its window read working
//! for good. The chief resumed T-2 16 s later; the resume's words waited, never
//! attempted, for the run's last 1 h 48 min, and 14 of 17 tasks stayed open behind
//! them. The daemon's own word for that end was that the window "ignored the
//! interrupt … To stop it now, cancel". The report is
//! `issues/pausing-a-pi-worker-mid-turn-strands-its-task.md` in consensflow-sme.
//!
//! What the chief should have seen, as decided since (T-1199, the owner's rule of
//! 2026-11-03: "pause should never close the window"): the window that ignored the pause is
//! kept, never closed; the task stays paused; the chief who gave it and the human are told
//! once that it did not stop, that ConsensFlow can see nothing it runs (its harness may be
//! hung), and that only the human can end it, by deleting its session. It is asked again
//! every minute. The resume's words wait for its turn to end, which a hung turn never does;
//! the human decides: paused again, its session deleted, the window ends.
//!
//! The stand-in worker's turn hangs as Pi's did (`HANG`: it takes no Escape at
//! all); a model request that never answers is the harness's own, so a turn that
//! ignores every key is the closest shape of it. Both orders are run: the field's, the
//! resume after the window is kept, and the resume during the rounds, before it.

use std::thread::sleep;

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The tier the worker was given, as a word the chief's brief names.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// The windows of the project's worker the daemon opened so far, oldest first.
fn workers_windows(rig: &Rig, project: &Project) -> Vec<Value> {
    let chief = format!("p{}-chief", project.id());
    rig.open_frames()
        .into_iter()
        .filter(|frame| frame["id"] != chief.as_str())
        .collect()
}

/// Whether the daemon closed `window` (its `pane.open`): it asked the pane host
/// to, and the host told it the window ended.
fn closed_by_the_daemon(rig: &Rig, window: &Value) -> bool {
    let this =
        |frame: &Value| frame["id"] == window["id"] && frame["generation"] == window["generation"];
    rig.kills().iter().any(this) && rig.exits().iter().any(this)
}

/// `cf` with `words`, run in the chief's window as its agent runs it.
fn cf(rig: &Rig, chief: &Value, words: &[&str]) -> cf_e2e::Result<()> {
    let ran = rig.cf_in_window(chief, words)?;
    if ran.code != Some(0) {
        return Err(Error::Daemon(format!("cf {}: {ran}", words.join(" "))));
    }
    Ok(())
}

/// The bodies of the notes that say a window did not stop: the human's, then
/// the chief's.
fn told(rig: &Rig, project: &Project) -> cf_e2e::Result<[Vec<String>; 2]> {
    let human = rig.page("inbox.get", json!({ "project": project.id() }))?["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let said = |messages: Vec<Value>| -> Vec<String> {
        messages
            .into_iter()
            .filter_map(|message| message["body"].as_str().map(str::to_owned))
            .filter(|body| body.contains("did not stop"))
            .collect()
    };
    Ok([said(human), said(project.inbox("chief")?)])
}

/// The bodies of the results the chief has been given or is to be given.
fn results(project: &Project) -> cf_e2e::Result<Vec<String>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .filter(|message| message["kind"] == "result")
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .collect())
}

#[test]
fn a_worker_hung_in_its_turn_is_kept_by_its_pause_told_once_and_its_resume_waits_for_the_human(
) -> Outcome {
    paused_then_resumed(false)
}

#[test]
fn a_worker_hung_in_its_turn_resumed_during_its_pauses_rounds_is_kept_and_its_words_wait_for_the_human(
) -> Outcome {
    paused_then_resumed(true)
}

/// The worker hangs in its turn on T-1 and the chief pauses T-1; `at_once`, the chief
/// resumes it at once, in the rounds of the pause; else once the window is kept. The
/// window is never closed, the chief and the human are told once, the resume's words wait,
/// and the human's delete-session ends the window.
fn paused_then_resumed(at_once: bool) -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} HANG 600 Reply with exactly: NEVER_OK",
            tier(&added)
        ),
    )?;
    rig.wait_for("the worker to be at work on T-1", secs(60), || {
        Ok(project.lane("worker")?["activity"]["state"] == "working")
    })?;
    let first = workers_windows(&rig, &project);
    let window = first
        .last()
        .ok_or_else(|| Error::Daemon("the worker has no window".to_owned()))?
        .clone();
    let handle = project.task(1)?["assignee"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let resume = ["task", "resume", "T-1", "Reply with exactly: RESUMED_OK"];

    // The chief pauses T-1, and the window ignores every key.
    cf(&rig, &chief, &["task", "pause", "T-1"])?;
    if at_once {
        cf(&rig, &chief, &resume)?;
    }
    rig.wait_for(
        "the chief and the human to be told the window is kept",
        secs(60),
        || {
            let [human, to_chief] = told(&rig, &project)?;
            Ok(!human.is_empty() && !to_chief.is_empty())
        },
    )?;
    // Past the minute it is asked again in, and a look or two.
    sleep(secs(70));
    assert!(
        !closed_by_the_daemon(&rig, &window),
        "a pause closes no window"
    );
    assert_eq!(
        workers_windows(&rig, &project).len(),
        first.len(),
        "no window opened again"
    );
    let [human, to_chief] = told(&rig, &project)?;
    assert!(
        human.len() == 1
            && human[0].contains("ConsensFlow can see nothing it runs")
            && human[0].contains(&format!("delete @{handle}'s session")),
        "the human was told once: {human:?}"
    );
    assert!(
        to_chief.len() == 1
            && to_chief[0].contains("ConsensFlow keeps the window open")
            && to_chief[0].contains("only the human can end it sooner, by deleting its session"),
        "the chief was told once: {to_chief:?}"
    );
    if !at_once {
        assert_eq!(project.task(1)?["state"], "paused");
        cf(&rig, &chief, &resume)?;
        sleep(secs(5));
    }
    // The resume's words wait for a turn that never ends: nothing is typed into it.
    assert_eq!(project.task(1)?["state"], "queued", "its words wait");
    assert!(results(&project)?.is_empty());

    // The human decides: a session whose task is not paused holds work, so the human
    // pauses it again (the delete's own refusal says so), and deleting the session ends the
    // window.
    let paused = rig.page("task.pause", json!({ "project": project.id(), "task": 1 }))?;
    assert_eq!(paused["ok"], true, "{paused}");
    let ended = rig.page(
        "session.end",
        json!({ "project": project.id(), "handle": handle }),
    )?;
    assert_eq!(ended["ok"], true, "{ended}");
    rig.wait_for("the window to end", secs(60), || {
        Ok(rig.exits().iter().any(|frame| {
            frame["id"] == window["id"] && frame["generation"] == window["generation"]
        }))
    })?;
    rig.close()?;
    Ok(())
}
