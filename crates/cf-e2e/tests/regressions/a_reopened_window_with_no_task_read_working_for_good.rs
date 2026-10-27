//! T-722, 2026-10-26, ConsensFlow 3.0.6-alpha.2 on the owner's Mac, a Devin tester on
//! `gpt-6-1-sol` (Devin 3000.11.205): **a window opened again with no task read
//! "Working" for good.**
//!
//! What the owner saw: the tester had finished its task, which was accepted, and its
//! window had closed. About an hour later its window was opened again, with no task,
//! from the board's view button. The ledger's only word on it was `window.activity`
//! working, five seconds after the open; the board showed "Working" on its row for
//! fifty minutes and more, while Devin, at its prompt, used ten seconds of CPU. The
//! false "Working" made the owner think the member was busy when they wanted to remove
//! it, and what waits for a window at rest (a delivery, a stop, a removal) waits for
//! this one for good. The report is `issues/a-reopened-window-with-no-task-reads-
//! working-forever.md` in consensflow-sme.
//!
//! Why: Devin's reader reads a turn as over only when its store marks the last answer
//! `finish_reason: stop`, or the wire log of the launch that ran the turn says it
//! completed. The GPT models' sessions never carry the first (every one of them on the
//! machine ends so: the owner's session ended on an answer with no tool call and no
//! finish reason), and the second is a file of the launch's folder, which the daemon
//! sweeps as it starts. So a finished conversation opened again read as an answer
//! still being written: not at rest, whatever Devin did, and no process was in it.
//! Resumed, a harness waits at its prompt and works on nothing until it is given a
//! message (the other four's readers already take what the record held at the open as
//! the old process's, `LeftOpen`, where it was in flight).
//!
//! What should have happened: the window opened with no message reads at rest, and
//! reads at work as soon as it has a turn of its own, whatever its record held.
//!
//! The case is that shape on the rig: the stand-in Devin (`fake-devin`) answers T-1 and
//! its window closes; its store is then as Devin stores a GPT model's session (no
//! `finish_reason` on any message) and the launch's folder is gone, as the daemon's
//! start leaves it; the human opens the window. The control is what must not be lost
//! to the fix: the human types a turn of the stand-in's that takes a while, and the
//! window reads at work for as long as it runs, and at rest after.

use std::fs;
use std::path::Path;

use cf_e2e::rig::{Project, Rig};
use cf_e2e::{Error, Result};
use rusqlite::Connection;
use serde_json::{json, Value};

use crate::devin_notes_twice::store_of;
use crate::devin_pause::write_roster;
use crate::{config, secs, Outcome, FAKE_DEVIN};

/// The words of the stand-in's turn the human types, which it takes a while to answer.
const LONG_TURN: &str = "SLEEP 25 Reply with exactly: LONG_TURN_DONE";

/// How long a window that reads at work must have read at rest before the control, for
/// the case to say it stayed so (the daemon looks at a window once a second).
const STAYS_AT_REST: u64 = 6;

/// Devin's store as a GPT model's session leaves it: its messages carry no
/// `finish_reason`.
fn as_a_gpt_models_session_ends(store: &Path) -> Result<()> {
    let db = Connection::open(store).map_err(|error| Error::Daemon(error.to_string()))?;
    db.busy_timeout(secs(5))
        .map_err(|error| Error::Daemon(error.to_string()))?;
    let changed = db
        .execute(
            "UPDATE message_nodes
             SET chat_message = json_remove(chat_message, '$.metadata.finish_reason')
             WHERE json_extract(chat_message, '$.metadata.finish_reason') IS NOT NULL",
            [],
        )
        .map_err(|error| Error::Daemon(error.to_string()))?;
    if changed == 0 {
        return Err(Error::Daemon(
            "the stand-in's store had no finish_reason to take away".to_owned(),
        ));
    }
    Ok(())
}

/// The state the board says the worker's window is in, and its pane, where it has one.
fn lane(project: &Project, member: &str) -> Result<Value> {
    project.lane(member)
}

fn activity(lane: &Value) -> &str {
    lane["activity"]["state"].as_str().unwrap_or_default()
}

#[test]
fn a_finished_devin_worker_opened_again_with_no_task_reads_at_rest_whatever_its_record_cannot_say(
) -> Outcome {
    let rig = Rig::start(config().devin(FAKE_DEVIN))?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    project.add_member("devin-worker")?;
    rig.tell(
        project.id(),
        "DISPATCH --tier standard Reply with exactly: T1_DONE",
    )?;
    rig.wait_for("T-1 to be answered", secs(90), || {
        let state = project.task(1)?["state"].clone();
        Ok(state == "done" || state == "accepted")
    })?;
    // The window closes once its task is done: nobody works in it.
    rig.wait_for("the worker's window to close", secs(60), || {
        Ok(!lane(&project, "devin-worker")?["pane"].is_object())
    })?;
    let handle = lane(&project, "devin-worker")?["participant"]["handle"]
        .as_str()
        .ok_or("the worker has no handle")?
        .to_owned();

    // What a GPT model's session is, and what a daemon that started since has left
    // of the launch that ran it.
    let store = store_of(&rig);
    assert!(
        store.is_file(),
        "the stand-in kept no store at {}",
        store.display()
    );
    as_a_gpt_models_session_ends(&store)?;
    let launches = rig.home().join("integrations").join("devin");
    if launches.is_dir() {
        fs::remove_dir_all(&launches)?;
    }

    // The human opens its window, with no task.
    let opened = rig.page(
        "session.open",
        json!({ "project": project.id(), "handle": handle }),
    )?;
    assert_eq!(opened["ok"], true, "{opened}");
    rig.wait_for("the reopened window to be up", secs(60), || {
        Ok(lane(&project, &handle)?["pane"].is_object())
    })?;
    // What the board read of it, each change: the report's was "working" for good.
    let mut read: Vec<String> = Vec::new();
    rig.wait_for("the reopened window to read at rest", secs(45), || {
        let now = lane(&project, &handle)?;
        if read.last().map(String::as_str) != Some(activity(&now)) {
            read.push(activity(&now).to_owned());
        }
        Ok(activity(&now) == "idle")
    })
    .map_err(|error| Error::Daemon(format!("{error}; the board read it {read:?}")))?;
    // It is not a look between two turns' worth of reading: it stays so.
    for _ in 0..STAYS_AT_REST {
        std::thread::sleep(secs(1));
        let now = lane(&project, &handle)?;
        assert_eq!(
            activity(&now),
            "idle",
            "the window has been given nothing: {now}"
        );
    }

    // The control: the human types a turn of its own, which takes a while. The window
    // reads at work for as long as it runs, and at rest once it has answered.
    let pane = lane(&project, &handle)?["pane"].clone();
    rig.types(
        &pane,
        format!("\u{1b}[200~{LONG_TURN}\u{1b}[201~\r").as_bytes(),
    )?;
    rig.wait_for(
        "the window to read at work on the turn typed",
        secs(30),
        || Ok(activity(&lane(&project, &handle)?) == "working"),
    )?;
    std::thread::sleep(secs(5));
    assert_eq!(
        activity(&lane(&project, &handle)?),
        "working",
        "a turn of 25 s is at work after 5"
    );
    rig.wait_for(
        "the window to read at rest once it answered",
        secs(60),
        || Ok(activity(&lane(&project, &handle)?) == "idle"),
    )?;
    rig.close()?;
    Ok(())
}
