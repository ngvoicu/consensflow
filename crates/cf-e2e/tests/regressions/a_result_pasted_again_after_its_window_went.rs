//! A result a chief's window took was pasted into it again after the window went.
//!
//! Found on 2026-10-16 reading the delivery code for B5 of the field reports of the day
//! before ("a result delivered twice", on the owner's Windows machine, whose windows
//! also closed in the middle of their tasks, B2); not seen by the owner as such, and no
//! issue in consensflow-sme holds it yet. A paste is confirmed when a look finds its
//! header in the window's record. A window that ended after its harness took the paste
//! and before that look (its program crashed, or was closed), or as the paste was handed
//! over, had the paste given back to its queue: the human's Resume brought the window
//! back on its conversation, which held the paste, and the paste went into it again. A
//! start settled the same paste by the record (`settle_in_flight`); a window's end did
//! not look.
//!
//! What should have happened: the chief reads the result once. Where the window's record
//! shows the paste as it goes, it was given, and is confirmed; it goes again only where
//! the record does not show it.
//!
//! The chief is the stand-in Devin, whose window ends as its agent takes the result
//! (`FAKE_DEVIN_CRASH`): its row is in the store and its prompt on the wire, and the
//! window is gone before the daemon's next look; the window the Resume opens on the same
//! session goes on. The result is a worker's, the stand-in Claude's. What the chief was
//! given is read from Devin's store, as the daemon reads it.
//!
//! Which way the window's end reaches the daemon is the timing's: its exit, with the paste
//! on its way (`Dispatcher::exited`), or the window gone as the paste is handed over
//! (`deliver`); on the code before the fix it went the first way. Both are the engine's
//! tests too (`window_went_with_a_paste`), which hold each whatever the timing. And a look
//! of the daemon's may fall between the row and the exit (a few milliseconds, once a
//! second), which confirms the paste before the window goes: a run on the code before the
//! fix may then pass, never one on the code after it fail.

use cf_e2e::devin_store;
use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::devin_notes_twice::store_of;
use crate::resume_of_a_chief_with_no_turn::windows_of;
use crate::{config, secs, Outcome, FAKE_DEVIN};

/// What the worker answers, and what the chief's window ends at.
const RESULT: &str = "RESULT_ONE";

/// The roster: a Devin (the stand-in) is the chief, and the stand-in Claude the worker.
fn write_roster(rig: &Rig) -> cf_e2e::Result {
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "devin", "model": "fake" },
            { "id": "worker", "kind": "claude-code", "model": "fake" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))
}

/// The result of T-1, as the chief's inbox holds it.
fn result(project: &Project) -> cf_e2e::Result<Option<Value>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "result" && message["body"] == RESULT))
}

/// How many messages the chief was given that hold the header of the message `id`.
fn given(rig: &Rig, id: i64) -> cf_e2e::Result<usize> {
    let marker = format!("[ConsensFlow m-{id} ");
    Ok(devin_store::messages(&store_of(rig))?
        .iter()
        .filter(|message| message.role == "user")
        .map(|message| message.times(&marker))
        .sum())
}

#[test]
fn a_result_a_devin_chiefs_window_took_as_it_went_is_not_pasted_again_into_the_window_the_resume_opens(
) -> Outcome {
    let rig = Rig::start(config().devin(FAKE_DEVIN).var("FAKE_DEVIN_CRASH", RESULT))?;
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .ok_or_else(|| Error::Daemon(format!("the worker has no tier: {added}")))?
        .to_owned();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), secs(30))?;
    let brief = format!("Reply with exactly: {RESULT}");
    let given_task = rig.cf_in_window(&chief, ["task", "add", "--tier", &tier, &brief])?;
    assert_eq!(given_task.code, Some(0), "{}", given_task.output());

    // The result is pasted into the chief's window, whose agent takes it and goes.
    rig.wait_for(
        "the chief's window to end as it takes the result",
        secs(120),
        || {
            Ok(rig
                .exits()
                .iter()
                .any(|exit| exit["id"] == chief["id"] && exit["generation"] == chief["generation"]))
        },
    )?;
    let id = result(&project)?
        .and_then(|message| message["id"].as_i64())
        .ok_or_else(|| Error::Daemon("the chief has no result".to_owned()))?;
    assert_eq!(
        given(&rig, id)?,
        1,
        "its window took the result before it went"
    );
    // What the window's end made of the paste is settled by now: the record showed the
    // paste as it went, so the ledger holds it delivered, and nothing waits to be pasted
    // into the window the resume opens. (The code before the fix gave it back to the
    // queue here.)
    rig.wait_for("the paste to be settled", secs(30), || {
        Ok(result(&project)?.is_some_and(|message| message["state"] != "delivering"))
    })?;
    let settled = result(&project)?.unwrap_or_default();
    assert_eq!(
        settled["state"], "delivered",
        "the window's end gave the paste back to the queue: {settled}"
    );

    // The human resumes the project: the chief's window comes back on its session.
    let resumed = rig.page("project.resume", json!({ "project": project.id() }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    rig.wait_for("the chief's window to be opened again", secs(60), || {
        Ok(windows_of(&rig, &project).len() >= 2)
    })?;
    rig.wait_for("the chief's window to be at rest", secs(120), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })?;

    let settled = result(&project)?.unwrap_or_default();
    assert_eq!(settled["state"], "delivered", "{settled}");
    let times = given(&rig, id)?;
    assert_eq!(
        times, 1,
        "the result (m-{id}) went into the chief's window {times} times; the paste was given back as the window went: {:?}",
        rig.events()
            .iter()
            .filter(|event| event["data"]["message"] == id)
            .map(|event| (event["kind"].clone(), event["data"]["reason"].clone()))
            .collect::<Vec<_>>()
    );
    rig.close()?;
    Ok(())
}
