//! A resumed Devin window's first message goes in its prompt file behind `/bypass`
//! (B1: Devin's `--resume` of a session the app's quit cut can come back in its normal
//! mode), and how Devin stores a prompt it ran under that command is not known: the
//! command with the prompt, or the prompt alone (the reviews of 2026-10-17). The
//! daemon reads either as the paste, its header first (`cf-harness`'s Devin record
//! reader), so a member's note that its turn was cut is received once and the member
//! goes on with its task. Read as stored with the command, the note was never received,
//! and its launch was given up when its time was out. (A chief's first message goes
//! behind `/bypass` only as its handoff given again on its conversation, which the
//! reader's own tests hold: the stand-in records a first message as it takes it.) The
//! stand-in Devin stores the command with the prompt where `FAKE_DEVIN_STORES_COMMANDS`
//! says so.

use cf_e2e::files;
use cf_e2e::rig::{Config, Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// The stand-in for Devin, built for these tests.
const FAKE_DEVIN: &str = env!("CARGO_BIN_EXE_fake-devin");

/// A rig whose Devin windows are the stand-in's, storing a prompt it ran under `/bypass`
/// with the command where `stores`.
fn rig_storing(stores: bool) -> Config {
    let config = config().devin(FAKE_DEVIN);
    if stores {
        config.var("FAKE_DEVIN_STORES_COMMANDS", "1")
    } else {
        config
    }
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

/// The message of `participant`'s inbox whose words hold `words`, as it is now.
fn message_with(
    project: &Project,
    participant: &str,
    words: &str,
) -> cf_e2e::Result<Option<Value>> {
    Ok(project.inbox(participant)?.into_iter().find(|message| {
        message["body"]
            .as_str()
            .is_some_and(|body| body.contains(words))
    }))
}

#[test]
fn a_resumed_devin_workers_note_that_its_turn_was_cut_is_received_once_however_devin_stores_its_bypass(
) -> Outcome {
    for stores in [true, false] {
        let rig = Rig::start(rig_storing(stores))?;
        write_roster(&rig)?;
        let project = Project::open(&rig, "chief", json!({}))?;
        let id = project.id();
        project.add_member("devin-worker")?;
        rig.tell(
            id,
            "DISPATCH --tier standard SLEEP 600 Reply with exactly: T1_DONE",
        )?;
        rig.wait_for("the Devin worker to be at work on T-1", secs(60), || {
            Ok(project.lane("devin-worker")?["activity"]["state"] == "working")
        })?;
        std::thread::sleep(secs(2));

        // The app quits with the worker's turn at work, and starts again on the same
        // home; the human resumes the project.
        let root = rig.close_keeping_the_root()?;
        let rig = Rig::start(rig_storing(stores).existing_root(root))?;
        let resumed = rig.page("project.resume", json!({ "project": id }))?;
        assert_eq!(resumed["ok"], true, "{resumed}");
        let project = Project::new(&rig, id);
        // A member's handle is its agent's name and a word of its own.
        let worker = project.lane("devin-worker")?["participant"]["handle"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let mut received = None;
        rig.wait_for(
            &format!("the worker's note to be received (the command stored with it: {stores})"),
            secs(90),
            || {
                received = message_with(&project, &worker, "Go on where you stopped.")?
                    .filter(|message| message["state"] == "delivered");
                Ok(received.is_some())
            },
        )?;
        let note = received.unwrap_or_default();
        assert_eq!(note["attempts"], 1, "given once, in the launch: {note}");
        rig.wait_for("the worker to answer T-1", secs(90), || {
            Ok(project.task(1)?["state"] != "working")
        })?;
        let state = project.task(1)?["state"].clone();
        assert!(
            state == "done" || state == "accepted",
            "T-1 is {state} (the command stored with its note: {stores})"
        );
        rig.close()?;
    }
    Ok(())
}
