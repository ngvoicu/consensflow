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
//! What the chief should have seen: the window that ignored the pause is closed,
//! the task stays paused, and the notes say why; the resume opens the window again
//! on its conversation, and the worker goes on with the resume's words.
//!
//! The stand-in worker's turn hangs as Pi's did (`HANG`: it takes no Escape at
//! all); a model request that never answers is the harness's own, so a turn that
//! ignores every key is the closest shape of it. The stand-in is Claude's, whose
//! window comes back on its conversation with `--resume`, as Pi's does with its
//! session. Both orders are run: the field's, the resume after the close, and the
//! resume during the rounds, before it.

use std::thread::sleep;

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::resume_of_a_chief_with_no_turn::{after, resumed_on};
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

/// Waits for the worker's window to come back: one more opened than `first`.
fn comes_back(rig: &Rig, project: &Project, first: &[Value]) -> cf_e2e::Result<()> {
    rig.wait_for("the worker's window to come back", secs(60), || {
        Ok(workers_windows(rig, project).len() == first.len() + 1)
    })
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
fn a_worker_hung_in_its_turn_is_closed_by_its_pause_and_goes_on_in_the_window_its_resume_opens(
) -> Outcome {
    paused_then_resumed(false)
}

#[test]
fn a_worker_hung_in_its_turn_resumed_before_its_pause_closed_it_goes_on_in_the_window_that_opens_again(
) -> Outcome {
    paused_then_resumed(true)
}

/// The worker hangs in its turn on T-1 and the chief pauses T-1; `at_once`, the
/// chief resumes it at once, in the rounds of the pause; else once the window is
/// closed. The worker goes on in the window that opens again, and its answer is
/// T-1's result.
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
    let session = first
        .first()
        .and_then(|window| after(window, "--session-id"))
        .ok_or("the worker's window has no session")?
        .to_owned();
    let window = first.last().ok_or("the worker has no window")?.clone();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let resume = ["task", "resume", "T-1", "Reply with exactly: RESUMED_OK"];

    // The chief pauses T-1, and the window ignores every key.
    cf(&rig, &chief, &["task", "pause", "T-1"])?;
    if at_once {
        // Resumed in the rounds, the window comes back by itself once it is closed: the
        // case looks at the close only after that, as a loaded runner's look may come.
        cf(&rig, &chief, &resume)?;
        comes_back(&rig, &project, &first)?;
    }
    // What the host was asked and told stays, however soon a window opens after it;
    // the notes come once the host has ended the window, which its exit can come before.
    rig.wait_for(
        "the daemon to have closed the window that ignored the pause, and the notes that say so",
        secs(60),
        || {
            let [human, to_chief] = told(&rig, &project)?;
            Ok(closed_by_the_daemon(&rig, &window) && human.is_empty() && !to_chief.is_empty())
        },
    )?;
    if at_once {
        assert_ne!(project.task(1)?["state"], "paused", "resumed, it stays so");
    } else {
        assert_eq!(project.task(1)?["state"], "paused");
    }
    let [human, to_chief] = told(&rig, &project)?;
    // One note of the close, to the chief who gave the task, and none of its own to
    // the human: two of one event said it twice (T-502's, 2026-10-25).
    assert!(human.is_empty(), "the human was told: {human:?}");
    assert!(
        to_chief.len() == 1 && to_chief[0].contains("so ConsensFlow closed it"),
        "the chief was told: {to_chief:?}"
    );
    if !at_once {
        // Paused, nothing opens it again.
        sleep(secs(3));
        assert_eq!(workers_windows(&rig, &project).len(), first.len());
        assert_eq!(project.task(1)?["state"], "paused");
        cf(&rig, &chief, &resume)?;
        comes_back(&rig, &project, &first)?;
    }

    // What the chief should have seen: the window opens again, after the one closed,
    // on its conversation, and the worker goes on with the resume's words.
    let again = workers_windows(&rig, &project).remove(first.len());
    assert!(
        again["generation"].as_u64() > window["generation"].as_u64(),
        "the window came back after the one closed: {} then {}",
        window["generation"],
        again["generation"]
    );
    assert_eq!(
        resumed_on(&again),
        Some(session.as_str()),
        "the window came back on its conversation: {:?}",
        again["argv"]
    );
    rig.wait_for("the worker to answer", secs(60), || {
        Ok(rig.transcript(&session).contains("\"text\":\"RESUMED_OK\""))
    })?;
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the result to reach the chief", secs(30), || {
        Ok(!results(&project)?.is_empty())
    })?;
    assert_eq!(results(&project)?, ["RESUMED_OK"]);
    rig.close()?;
    Ok(())
}
