//! A project's wake-up calls, end to end through the real daemon and pane host
//! (`npm run test:integration`): the human turns them on for a minute, which is
//! the least a wake-up may wait, and the chief, a stand-in `claude` in a real
//! terminal, is given the default words once, by the usual queue, as a note
//! from the human. Its reply stays in its terminal: the board gains no task and
//! the human's inbox no message. A chief in the middle of a long turn is not
//! woken until the turn is over, and then once.
//!
//! The daemon's clock is the system's and the rig has no way to move it (the
//! interval is never shortened by anything but the human's own setting), so each
//! of these two cases waits out a real minute; the hours, the days, the quiet
//! board, the Close and the words the human has typed and not sent are the
//! engine's tests (`tests/dispatcher/wake_ups.rs`) and the ledger's
//! (`tests/wake_ups/`), which move a clock of their own. (The human's own typing
//! reaches a window by the desktop app's input alone: the pane host the rig
//! runs has no way to make a key the human's, so no case here can leave words
//! unsent in a window.)

use std::time::{Duration, Instant};

use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::{rig, secs, session_of, Outcome};

/// The words of a wake-up the human wrote none of their own for: the owner's.
const WORDS: &str =
    "Do you still have work to do? If tasks are left on the board or in your plan, carry on.";

/// What the stand-in answers a message with, after its first line.
const NOTED: &str = "noted: [ConsensFlow m-";

/// What the stand-in recorded of the conversation, a record at a time.
fn records(rig: &Rig, session: &str) -> Vec<Value> {
    rig.transcript(session)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}

/// What a conversation was given, the user's turns, oldest first.
fn given_texts(rig: &Rig, session: &str) -> Vec<String> {
    records(rig, session)
        .iter()
        .filter(|record| record["type"] == "user")
        .filter_map(|record| record["message"]["content"].as_str().map(str::to_owned))
        .collect()
}

/// How many of the messages a conversation was given say `words`.
fn given(rig: &Rig, session: &str, words: &str) -> usize {
    given_texts(rig, session)
        .iter()
        .filter(|text| text.contains(words))
        .count()
}

/// How many times the conversation's agent answered the header of a message.
fn answered_a_header(rig: &Rig, session: &str) -> usize {
    records(rig, session)
        .iter()
        .filter(|record| record["type"] == "assistant")
        .filter(|record| {
            record["message"]["content"]
                .as_array()
                .is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part["text"]
                            .as_str()
                            .is_some_and(|text| text.starts_with(NOTED))
                    })
                })
        })
        .count()
}

/// How many times the log says a wake-up was queued.
fn queued(rig: &Rig) -> usize {
    rig.events()
        .iter()
        .filter(|event| event["kind"] == "project.wake_up")
        .count()
}

/// Waits until the chief's turn ends.
fn until_the_chief_is_idle(rig: &Rig, project: &Project, within: Duration) -> cf_e2e::Result<()> {
    rig.wait_for("the chief to be idle", within, || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })
}

/// The project's wake-up calls, turned on a minute apart: the daemon's answer.
fn turned_on(rig: &Rig, project: i64) -> cf_e2e::Result<Value> {
    rig.page(
        "wakeup.set",
        json!({ "project": project, "on": true, "every": 1, "unit": "minutes" }),
    )
}

#[test]
fn a_wake_up_set_for_a_minute_reaches_the_idle_chief_once_and_its_reply_puts_nothing_on_the_board(
) -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let chief = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    let session = session_of(&chief);
    until_the_chief_is_idle(&rig, &project, secs(60))?;

    // Off until the human turns it on, with the owner's words.
    let read = rig.page("wakeup.get", json!({ "project": id }))?;
    assert_eq!(read["wakeUp"]["on"], false, "{read}");
    assert_eq!(read["wakeUp"]["text"], WORDS);
    let set = turned_on(&rig, id)?;
    assert_eq!(set["ok"], true, "{set}");
    assert_eq!(set["wakeUp"]["everyMs"], 60_000, "{set}");
    let began = Instant::now();

    // Nothing goes before the minute is up.
    std::thread::sleep(secs(30));
    assert_eq!(given(&rig, &session, WORDS), 0, "not yet");
    assert_eq!(queued(&rig), 0);

    rig.wait_for("the wake-up to reach the chief", secs(75), || {
        Ok(given(&rig, &session, WORDS) == 1)
    })?;
    assert!(
        began.elapsed() >= secs(59),
        "a minute after it was turned on, not before: {:?}",
        began.elapsed()
    );
    rig.wait_for("the chief to answer it", secs(30), || {
        Ok(answered_a_header(&rig, &session) >= 1)
    })?;
    until_the_chief_is_idle(&rig, &project, secs(30))?;
    rig.wait_for(
        "the wake-up to be confirmed by the chief's record",
        secs(30),
        || {
            Ok(project
                .inbox("chief")?
                .iter()
                .any(|message| message["body"] == WORDS && message["state"] == "delivered"))
        },
    )?;

    // One wake-up, a note from the human to the chief, through the usual queue.
    assert_eq!(queued(&rig), 1);
    let note = project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["body"] == WORDS)
        .expect("the wake-up is in the chief's inbox");
    assert_eq!(
        (note["kind"].as_str(), note["sender"].as_str()),
        (Some("note"), Some("human"))
    );
    let events = rig.events();
    let kinds: Vec<&str> = events
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .filter(|kind| {
            kind.starts_with("project.wake_up")
                || kind.starts_with("message.")
                || kind.starts_with("delivery.begun")
                || kind.starts_with("delivery.confirmed")
                || kind.starts_with("task.")
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "project.wake_up_setting",
            "message.sent",
            "project.wake_up",
            "delivery.begun",
            "delivery.confirmed"
        ],
        "the setting, the wake-up and its delivery: and nothing of the reply"
    );

    // The reply stays in the chief's terminal: no task, and nothing for the human.
    let board = project.board()?;
    assert!(
        board["lanes"].as_array().is_some_and(|lanes| lanes
            .iter()
            .all(|lane| lane["tasks"].as_array().is_some_and(Vec::is_empty))),
        "no task on the board: {board}"
    );
    let human = rig.page("inbox.get", json!({ "project": id }))?;
    assert_eq!(
        human["messages"],
        json!([]),
        "the human's inbox gained nothing"
    );
    assert_eq!(
        given(&rig, &session, WORDS),
        1,
        "the chief was given it once"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_chief_in_a_long_turn_is_not_woken_until_the_turn_is_over_and_then_once() -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let chief = rig.open_frame(&format!("p{id}-chief"), OPEN)?;
    let session = session_of(&chief);
    until_the_chief_is_idle(&rig, &project, secs(60))?;

    // The chief goes into a turn that outlasts the minute, and the calls are turned on.
    rig.tell(id, "SLEEP 85 Reply with exactly: WORKED")?;
    rig.wait_for("the chief to be at work", secs(30), || {
        Ok(project.lane("chief")?["activity"]["state"] == "working")
    })?;
    turned_on(&rig, id)?;
    let began = Instant::now();

    // The minute passes, and the chief is still in its turn: nothing goes in, and none is queued
    // to land stale right after it.
    std::thread::sleep(secs(75));
    assert_eq!(project.lane("chief")?["activity"]["state"], "working");
    assert_eq!(given(&rig, &session, WORDS), 0, "nothing goes into a turn");
    assert_eq!(queued(&rig), 0, "and none is queued");

    // The turn ends, and the wake-up follows it, once.
    rig.wait_for("the wake-up to follow the turn", secs(60), || {
        Ok(given(&rig, &session, WORDS) == 1)
    })?;
    assert!(began.elapsed() >= secs(75));
    let order = given_texts(&rig, &session);
    let at = |words: &str| order.iter().position(|text| text.contains(words));
    assert!(
        at("SLEEP 85") < at(WORDS),
        "the human's words first, then the wake-up: {order:?}"
    );
    assert_eq!(queued(&rig), 1);
    rig.close()?;
    Ok(())
}
