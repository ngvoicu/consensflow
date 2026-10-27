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
    // within the rounds, which what the stop closed below shows. How many
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

    // The window stopped, so the stop closed nothing: no close was asked of its pane and
    // none is in the ledger, the lane is on the window it was, the board says nothing of an
    // ignored stop, and the task is paused by the stop its pause asked. What happened is
    // read, never a note's words, which T-551 moved to whoever gave the task and so blinded
    // a check of the human's (T-664, B's round 1: the window was closed with its plant of a
    // cancelled Devin turn read as unended in, and the case passed). A failure says what the
    // daemon wrote from just before the pause on, and the keys it pressed: what the stop read
    // is there, after the fact.
    let events = rig.events();
    let since = |event: &Value| at_ms(event).is_some_and(|at| at > paused - 3_000);
    let written: Vec<String> = events
        .iter()
        .filter(|event| since(event))
        .map(|event| format!("{:+} ms {event}", at_ms(event).unwrap_or(0) - paused))
        .collect();
    let what = format!(
        "the daemon wrote, from 3 s before the pause:\n{}\nand pressed Escape at {:?}",
        written.join("\n"),
        presses.iter().map(|at| at - paused).collect::<Vec<_>>()
    );
    let closes: Vec<Value> = rig
        .kills()
        .into_iter()
        .filter(|kill| kill["id"] == pane.as_str())
        .collect();
    assert!(
        closes.is_empty(),
        "the daemon closed the window, {closes:?}; {what}"
    );
    let ended: Vec<&Value> = events
        .iter()
        .filter(|event| since(event))
        .filter(|event| {
            matches!(
                event["kind"].as_str(),
                Some("session.ended" | "conversation.ended")
            )
        })
        .collect();
    assert!(ended.is_empty(), "the ledger ended {ended:?}; {what}");
    let lane = project.lane("devin-worker")?;
    assert_eq!(
        lane["pane"]["id"],
        pane.as_str(),
        "the lane: {lane}; {what}"
    );
    assert!(lane["unstopped"].is_null(), "the lane: {lane}; {what}");
    let stopped = when_of(&events, "task.state", |event| {
        event["data"]["task"] == 1
            && event["data"]["to"] == "paused"
            && event["data"]["stop"].is_i64()
    });
    assert_eq!(
        stopped,
        Some(paused),
        "the pause asked no stop of T-1; {what}"
    );
    assert_eq!(project.task(1)?["state"], "paused", "{what}");
    rig.close()?;
    Ok(())
}
