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
//! - A turn whose agent says nothing for 38 s before its first word, which past the 30 s the
//!   adapter holds a prompt for (`begun_on_the_store`) only Devin's word that it works holds
//!   at work, and then works 10 s in silence, is at work throughout, and its answer is the
//!   task's result. (Integration B round 2, T-800: with 20 s, B's store held the rest of the
//!   turn by its unanswered call, and the case no longer told the wire's word apart.)
//! - A pause stops a turn whose agent says nothing past those 30 s with Escape, and the
//!   chief's question goes in after it.
//! - A window opened again on its conversation, which replays its last turn stamped and
//!   with no end, is at rest: the note that its turn was cut goes in, once.

use std::thread::sleep;
use std::time::Instant;

use cf_e2e::live::trace::at_ms;
use cf_e2e::rig::{Config, Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The stand-in for Devin, built for these tests.
const FAKE_DEVIN: &str = env!("CARGO_BIN_EXE_fake-devin");

/// The stand-in's word for Devin 3000.11's wire log.
const DEVIN_3000_11: &str = "1";

/// The stand-in's word for Devin 3000.12.43's: stamped as 3000.11's, with no word of
/// Devin's on a turn, and a window opened again on a turn a quit cut naming no
/// conversation until it is typed to (T-1417, three live runs of 2026-11-11).
const DEVIN_3000_12: &str = "3000.12";

/// A rig whose Devin windows write their wire logs as the Devin `shape` names does.
fn stamped_rig(shape: &str) -> Config {
    config().devin(FAKE_DEVIN).var("FAKE_DEVIN_STAMPS", shape)
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
    rig.stand_ins(&roster)
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
    at_work_through_a_silent_stretch(DEVIN_3000_11)
}

#[test]
#[ignore = "red on main: Devin 3000.12.43 writes no working/blocked (T-1417)"]
fn a_devin_3000_12_turn_is_at_work_through_a_silent_stretch_and_its_answer_is_the_result() -> Outcome
{
    at_work_through_a_silent_stretch(DEVIN_3000_12)
}

/// A turn whose agent says nothing for 38 s is at work throughout, and its answer is the
/// task's result, on the wire log of the Devin `shape` names.
fn at_work_through_a_silent_stretch(shape: &str) -> Outcome {
    let rig = Rig::start(stamped_rig(shape))?;
    let project = given(&rig, "THINK 38 SLEEP 10 Reply with exactly: T1_DONE")?;
    let begun = Instant::now();
    // Every look from a few seconds in to a few seconds before the turn's end finds the
    // window at work: past the adapter's 30 s for a prompt, too.
    let mut seen = Vec::new();
    while begun.elapsed() < secs(44) {
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
    a_pause_stops_the_turn(DEVIN_3000_11)
}

#[test]
#[ignore = "red on main: Devin 3000.12.43 writes no working/blocked (T-1417)"]
fn a_pause_stops_a_devin_3000_12_turn_and_the_chiefs_question_goes_in_after() -> Outcome {
    a_pause_stops_the_turn(DEVIN_3000_12)
}

/// A pause stops a turn whose agent says nothing past the adapter's 30 s with Escape, and
/// the chief's question goes in after, on the wire log of the Devin `shape` names.
fn a_pause_stops_the_turn(shape: &str) -> Outcome {
    let rig = Rig::start(stamped_rig(shape))?;
    let project = given(&rig, "THINK 120 Reply with exactly: T1_DONE")?;
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
    opened_again_takes_the_note_once(DEVIN_3000_11)
}

/// Devin 3000.12.43 opened again on a turn a quit cut says nothing of its mode until it is
/// typed to (T-1417, run 3): the resume the daemon opens carries its note in the launch,
/// which Devin answers, so its window is not held.
#[test]
fn a_devin_3000_12_window_opened_again_on_a_turn_a_quit_cut_takes_the_note_once() -> Outcome {
    opened_again_takes_the_note_once(DEVIN_3000_12)
}

/// A window opened again on its conversation, whose last turn a quit cut, is at rest and
/// takes the note that its turn was cut once, on the wire log of the Devin `shape` names.
fn opened_again_takes_the_note_once(shape: &str) -> Outcome {
    let rig = Rig::start(stamped_rig(shape))?;
    let project = given(&rig, "SLEEP 600 Reply with exactly: T1_DONE")?;
    let id = project.id();
    sleep(secs(3));
    // The app quits with the turn at work, and starts again on the same home; the human
    // resumes the project, and the window opened again replays its last turn.
    let root = rig.close_keeping_the_root()?;
    let rig = Rig::start(stamped_rig(shape).existing_root(root))?;
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
