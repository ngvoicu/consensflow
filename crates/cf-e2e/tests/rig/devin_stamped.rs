//! A Devin window that writes its wire log as Devin 3000.11 does reads as at work for as
//! long as its turn is (T-660, from T-656's reading of the live suites `notes`,
//! `wake-ups` and `stops`, 2026-10-26). Devin 3000.11 stamps every record of a turn's work
//! with its time, as it stamps the history a window opened again replays, logs no request
//! of its TUI's, and says itself when a turn is at work (`working`) and when it waits
//! (`blocked`). Read by its stamps, a turn at work was a replay: through a minute's
//! command or a long answer the board read the window at rest, a pause pressed no Escape
//! and pasted its question into the turn, and the Stop of the live suite was never reached.
//! The stand-in Devin writes that shape where `FAKE_DEVIN_STAMPS` says so.
//!
//! - A turn that says nothing for 45 s, past the 30 s the adapter holds a prompt for, is at
//!   work throughout, and its answer is the task's result.
//! - A pause stops such a turn with Escape, and the chief's question goes in after it.
//! - A window opened again on its conversation, which replays its last turn stamped and
//!   with no end, is at rest: the note that its turn was cut goes in, once.

use std::thread::sleep;
use std::time::Instant;

use cf_e2e::files;
use cf_e2e::live::trace::at_ms;
use cf_e2e::rig::{Config, Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The stand-in for Devin, built for these tests.
const FAKE_DEVIN: &str = env!("CARGO_BIN_EXE_fake-devin");

/// A rig whose Devin windows write their wire logs as Devin 3000.11 does.
fn stamped_rig() -> Config {
    config().devin(FAKE_DEVIN).var("FAKE_DEVIN_STAMPS", "1")
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

/// A project of a stamped rig, with its Devin worker, given `task`; waited for until the
/// task is at work: the brief went in.
fn given<'a>(rig: &'a Rig, task: &str) -> cf_e2e::Result<Project<'a>> {
    write_roster(rig)?;
    let project = Project::open(rig, "chief", json!({}))?;
    project.add_member("devin-worker")?;
    rig.tell(project.id(), &format!("DISPATCH --tier standard {task}"))?;
    rig.wait_for("T-1 to be at work", secs(60), || {
        Ok(project.task(1)?["state"] == "working")
    })?;
    Ok(project)
}

/// What the worker's window is doing, as the board says.
fn activity(project: &Project) -> cf_e2e::Result<String> {
    Ok(project.lane("devin-worker")?["activity"]["state"]
        .as_str()
        .unwrap_or("not there")
        .to_owned())
}

/// When the first of the daemon's events of `kind` that `matching` takes was written.
fn when_of(events: &[Value], kind: &str, matching: impl Fn(&Value) -> bool) -> Option<i64> {
    events
        .iter()
        .filter(|event| event["kind"] == kind && matching(event))
        .find_map(at_ms)
}

#[test]
fn a_devin_turn_stamped_as_devin_3000_11_stamps_it_is_at_work_through_a_silent_stretch_and_its_answer_is_the_result(
) -> Outcome {
    let rig = Rig::start(stamped_rig())?;
    let project = given(&rig, "SLEEP 45 Reply with exactly: T1_DONE")?;
    let begun = Instant::now();
    // Every look from a few seconds in to a few seconds before the turn's end finds the
    // window at work: past the adapter's 30 s for a prompt, too.
    let mut seen = Vec::new();
    while begun.elapsed() < secs(40) {
        let state = activity(&project)?;
        if begun.elapsed() > secs(3) {
            seen.push((begun.elapsed().as_secs(), state));
        }
        sleep(secs(1));
    }
    let resting: Vec<&(u64, String)> = seen
        .iter()
        .filter(|(_, state)| state != "working")
        .collect();
    assert!(
        resting.is_empty(),
        "the window read so while its turn was at work: {resting:?}"
    );
    rig.wait_for("T-1 to be done", secs(60), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let results = project.inbox("chief")?;
    assert!(
        results.iter().any(|message| message["kind"] == "result"
            && message["body"]
                .as_str()
                .is_some_and(|body| body.contains("T1_DONE"))),
        "its answer is the result: {results:?}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_pause_stops_a_devin_turn_stamped_as_devin_3000_11_stamps_it_and_the_chiefs_question_goes_in_after(
) -> Outcome {
    let rig = Rig::start(stamped_rig())?;
    let project = given(&rig, "SLEEP 120 Reply with exactly: T1_DONE")?;
    let pane = project.lane("devin-worker")?["pane"]["id"]
        .as_str()
        .ok_or("the worker has no window")?
        .to_owned();
    // Past the adapter's 30 s for a prompt: only the wire's word holds the turn at work.
    sleep(secs(35));
    rig.tell(project.id(), "CF tell T-1 :: Reply with exactly: ANSWERED")?;
    let question = |events: &[Value]| -> Option<Value> {
        events
            .iter()
            .find(|event| event["kind"] == "message.sent" && event["data"]["kind"] == "question")
            .map(|event| event["data"]["message"].clone())
    };
    rig.wait_for(
        "the chief's question to go into the window",
        secs(60),
        || {
            let events = rig.events();
            Ok(question(&events).is_some_and(|message| {
                when_of(&events, "delivery.confirmed", |event| {
                    event["data"]["message"] == message
                })
                .is_some()
            }))
        },
    )?;
    let events = rig.events();
    let message = question(&events).ok_or("the chief asked nothing")?;
    let paused = when_of(&events, "task.state", |event| {
        event["data"]["to"] == "paused"
    })
    .ok_or("the daemon never paused T-1")?;
    // The window was stopped, as Devin is: two Escapes close together.
    let presses = rig.presses(&pane, &[27]);
    assert!(
        presses.len() >= 2,
        "the daemon pressed Escape at {presses:?}"
    );
    assert!(
        rig.screen(&pane).contains("Interrupted"),
        "its turn was interrupted: {}",
        rig.screen(&pane)
    );
    // And the question was pasted after the stop, not into the turn it stopped.
    let begun = when_of(&events, "delivery.begun", |event| {
        event["data"]["message"] == message
    })
    .ok_or("the question was never pasted")?;
    assert!(
        begun >= presses[0],
        "the question was pasted {} ms before the first Escape",
        presses[0] - begun
    );
    assert!(
        begun - paused < 15_000,
        "pasted {} ms after the pause",
        begun - paused
    );
    assert_eq!(project.task(1)?["state"], "paused");
    rig.close()?;
    Ok(())
}

#[test]
fn a_devin_window_opened_again_on_a_turn_it_replays_stamped_is_at_rest_and_takes_the_note_once(
) -> Outcome {
    let rig = Rig::start(stamped_rig())?;
    let project = given(&rig, "SLEEP 600 Reply with exactly: T1_DONE")?;
    let id = project.id();
    sleep(secs(3));
    // The app quits with the turn at work, and starts again on the same home; the human
    // resumes the project, and the window opened again replays its last turn.
    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(stamped_rig().existing_root(root))?;
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let project = Project::new(&rig, id);
    let worker = project.lane("devin-worker")?["participant"]["handle"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let mut note = Value::Null;
    rig.wait_for("the note that the turn was cut to go in", secs(90), || {
        note = project
            .inbox(&worker)?
            .into_iter()
            .find(|message| {
                message["body"]
                    .as_str()
                    .is_some_and(|body| body.contains("Go on where you stopped."))
            })
            .unwrap_or_default();
        Ok(note["state"] == "delivered")
    })?;
    assert_eq!(note["attempts"], 1, "given once: {note}");
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.close()?;
    Ok(())
}
