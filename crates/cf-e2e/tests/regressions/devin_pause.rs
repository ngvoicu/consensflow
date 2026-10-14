//! T-5 of project 7, 2026-10-12, ConsensFlow 3.0.1 on the owner's Windows machine:
//! **a pause did not stop a Devin window.**
//!
//! What the owner saw: the worker, a Devin window, was working on T-5 (it had been
//! since 10:08:41). At 10:08:56 the chief paused T-5 (`cf tell`: the pause and a
//! question, m-143). The daemon pressed the harness's interrupt (Devin takes two
//! Escapes, 150 ms apart, and one more a second later), again three seconds after,
//! three rounds in all; at 10:09:05 it told the human and the chief that the window
//! had ignored the stop, and the window went on with its earlier turn until it
//! ended by itself, at 10:10:05. m-143 went into the window at 10:10:07, 70 s
//! after the chief sent it.
//!
//! What should have happened: Escape twice stops a Devin turn, so the first round
//! ends it; the daemon reads the window at rest, the stop is paid, the chief's
//! question goes into the window at once, and the human is told nothing of an
//! ignored stop.
//!
//! The stand-in Devin (`fake-devin`) takes Escape only as Devin does: one Escape
//! stops nothing, and two within half a second stop the turn at work. It writes
//! Devin's store and wire log as the daemon reads them, so the daemon's Devin adapter
//! runs on it whole: its launch, its look at a turn, the interrupt it asks for
//! (`presses: 2`), what it makes of the cancel. What this proves is the daemon's
//! side of the stop (the keys it asks Devin's adapter for, how it presses them and
//! how it reads the turn that ends): the part of the bug that is in code. The
//! reported failure itself was on Windows, where nothing in the code that writes the
//! keys can tell why Devin did not take them (`pty::keys` of `cf-panes` is the test
//! that asks the pseudoconsole what it makes of them, and `cargo xtask live stops
//! --case midturn devin` what a real Devin does), so the closest shape of it is this:
//! the same chain, on the stand-in, on every platform the matrix runs it on.

use std::thread::sleep;

use cf_e2e::files;
use cf_e2e::live::trace::at_ms;
use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome, FAKE_DEVIN};

/// What the human is told when a window ignored every round of a stop.
const IGNORED: &str = "did not stop for T-";

/// The roster of the case: the stand-in Claude is the chief, and a Devin (its
/// stand-in) the worker.
pub(crate) fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "devin-worker", "kind": "devin", "model": "fake", "workTier": "standard" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// When the first of the daemon's events of `kind` that `matching` takes was written,
/// in milliseconds since the epoch.
fn when_of(events: &[Value], kind: &str, matching: impl Fn(&Value) -> bool) -> Option<i64> {
    events
        .iter()
        .filter(|event| event["kind"] == kind && matching(event))
        .find_map(at_ms)
}

/// When the question the chief put to the window went into it: the first question
/// there is, once its delivery was confirmed.
fn delivered(events: &[Value]) -> Option<i64> {
    let question = events
        .iter()
        .find(|event| event["kind"] == "message.sent" && event["data"]["kind"] == "question")?
        .get("data")?
        .get("message")?
        .clone();
    when_of(events, "delivery.confirmed", |event| {
        event["data"]["message"] == question
    })
}

#[test]
fn a_pause_stops_a_devin_window_that_takes_escape_twice_within_the_rounds_and_the_chiefs_question_goes_in(
) -> Outcome {
    let rig = Rig::start(config().devin(FAKE_DEVIN))?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    project.add_member("devin-worker")?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard SLEEP 120 Reply with exactly: T5_DONE",
    )?;
    let working = |project: &Project| -> cf_e2e::Result<bool> {
        Ok(project.lane("devin-worker")?["activity"]["state"] == "working")
    };
    rig.wait_for("the Devin window to be at work on T-1", secs(60), || {
        working(&project)
    })?;
    let lane = project.lane("devin-worker")?;
    let handle = lane["participant"]["handle"]
        .as_str()
        .ok_or("the worker has no handle")?
        .to_owned();
    let pane = lane["pane"]["id"]
        .as_str()
        .ok_or("the worker has no window")?
        .to_owned();
    // The turn has been going a moment: it is in the middle of its work.
    sleep(secs(2));

    // The chief pauses T-1 and puts a question to it (`cf tell`).
    rig.tell(project.id(), "CF tell T-1 :: Reply with exactly: ANSWERED")?;
    rig.wait_for(
        "the chief's question to go into the window",
        secs(60),
        || {
            let events = rig.events();
            Ok(delivered(&events).is_some())
        },
    )?;

    let events = rig.events();
    let paused = when_of(&events, "task.state", |event| {
        event["data"]["to"] == "paused"
    })
    .ok_or("the daemon never paused T-1")?;
    // The question went in soon after the pause, and not when the turn it stopped would have ended.
    let asked = delivered(&events).ok_or("the question was never delivered")?;
    assert!(
        asked - paused < 15_000,
        "the question went into the window {} ms after the pause",
        asked - paused
    );
    // The window was read at rest after the pause, and the stop was paid there:
    // within the rounds, which the human hears of no ignored stop below. How many
    // seconds that took is the machine's (CI's macOS read it at rest 9.2 s after).
    when_of(&events, "window.activity", |event| {
        event["participant"] == handle.as_str()
            && event["state"] == "idle"
            && at_ms(event).is_some_and(|at| at > paused)
    })
    .ok_or("the daemon never read the Devin window at rest")?;

    // Devin takes two Escapes, which the daemon presses 150 ms apart, and one more a second later.
    let presses = rig.presses(&pane, &[27]);
    assert!(presses.len() >= 2, "the daemon pressed {presses:?}");
    assert!(
        presses[1] - presses[0] < 400,
        "the first two Escapes of the first round were {} ms apart",
        presses[1] - presses[0]
    );
    // And the window took them as Devin does: its turn was interrupted, which it says.
    assert!(
        rig.screen(&pane).contains("Interrupted"),
        "its screen: {}",
        rig.screen(&pane)
    );

    // No word of an ignored stop, to the human or to the chief.
    let human = rig.page("inbox.get", json!({ "project": project.id() }))?;
    let mut inboxes: Vec<Value> = human["messages"].as_array().cloned().unwrap_or_default();
    inboxes.extend(project.inbox("chief")?);
    let told: Vec<&Value> = inboxes
        .iter()
        .filter(|message| {
            message["body"]
                .as_str()
                .is_some_and(|body| body.contains(IGNORED))
        })
        .collect();
    assert!(
        told.is_empty(),
        "the human and the chief were told: {told:?}"
    );
    assert_eq!(project.task(1)?["state"], "paused");
    rig.close()?;
    Ok(())
}
