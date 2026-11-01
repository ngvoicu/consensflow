//! An install whose ledger and agents name a harness this build does not run.
//!
//! ConsensFlow 3.0.7 took a harness out (T-566, the removal's RMOC-19) with no
//! code for the state an older build left of it (the owner's rule: none for old state).
//! What such an install holds is the harness's word: in the ledger, on its chief, on a
//! member with a task, and on the conversations whose transcripts were copied; in
//! `agents.json`, on agents of the human's own. Whatever the word, the build reads it as
//! it reads any kind it does not run.
//!
//! What should be: the app starts and the board opens; History reads the transcripts;
//! the member's task is paused, with the reason and the way out (cancel and add it again,
//! or finish it: no resume opens its window), and the member is said not to be free: it
//! runs on a harness whose windows ConsensFlow cannot open, and is to leave the staff;
//! Switch chief moves the project off the chief's harness and the new chief works; the
//! agents of the human's own on that harness are unsupported and hidden.
//!
//! The case makes that install on the rig: a project of the stand-in Claude, a chief
//! that answered and a worker at work on T-1, closed with its home kept. Then its words
//! are the gone harness's (this file holds the only one), and a new rig starts on it.

use std::path::Path;

use cf_e2e::files;
use cf_e2e::rig::{chief_body, Project, Rig};
use rusqlite::Connection;
use serde_json::{json, Value};

use crate::idle_compaction_read_as_a_cut_turn::{
    history, is_said, items, told, wait_for_the_turn_to_end,
};
use crate::resume_of_a_chief_with_no_turn::{after, wait_for_the_chief, windows_of};
use crate::{config, secs, Outcome};

/// The harness an older build ran, which this one does not: the field's own word, as a 3.0.6 ledger writes it.
const GONE: &str = "opencode";

/// The way out a pause note gives for T-1, whose member's harness has no adapter.
const WAY_OUT: &str = "Its member runs on a harness whose windows ConsensFlow cannot open: cancel it with cf task cancel T-1 and add it again with cf task add once a member on another harness is on the staff, or finish it with cf task finish T-1";

/// What the chief was told before the upgrade, and after it.
const BEFORE: &str = "Reply with exactly: BEFORE_THE_UPGRADE";
const AFTER: &str = "Reply with exactly: AFTER_THE_SWITCH";

/// The bodies of the notes among `messages`.
fn notes(messages: &Value) -> Vec<String> {
    messages
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["kind"] == "note")
        .filter_map(|message| message["body"].as_str().map(str::to_owned))
        .collect()
}

/// The tier the worker was given.
fn tier(added: &Value) -> &str {
    added["member"]["tier"].as_str().unwrap_or_default()
}

/// The home under `root` as an older build that ran [`GONE`] left it: the chief, the
/// worker and their conversations on it, and the agents of the human's own that ran them.
fn written_by_an_older_build(root: &Path) -> Outcome {
    let home = root.join("consensflow");
    let ledger = Connection::open(home.join("consensflow.db"))?;
    // Every participant that runs on a harness: the chief, the worker and its session.
    let moved = ledger.execute(
        "UPDATE participant SET harness = ?1 WHERE harness IS NOT NULL",
        [GONE],
    )?;
    assert_eq!(moved, 3, "the chief, the worker and the worker's session");
    ledger.execute(
        "UPDATE conversation SET harness = ?1 WHERE participant_id IN \
         (SELECT id FROM participant WHERE harness = ?1)",
        [GONE],
    )?;
    ledger.close().map_err(|(_, failed)| failed)?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": GONE, "model": "fake-chief" },
            { "id": "worker", "kind": GONE, "model": "fake" },
            { "id": "next", "kind": "claude-code", "model": "fake-chief" },
        ],
    });
    files::write(&home.join("agents.json"), format!("{roster:#}\n"))?;
    Ok(())
}

#[test]
fn an_install_of_a_harness_the_build_does_not_run_opens_reads_its_history_and_moves_off_it(
) -> Outcome {
    let mut rig = Rig::start(config())?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "chief", "kind": "claude-code", "model": "fake-chief" },
            { "id": "worker", "kind": "claude-code", "model": "fake" },
            { "id": "next", "kind": "claude-code", "model": "fake-chief" },
        ],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster:#}\n"))?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let id = project.id();
    let added = project.add_member("worker")?;
    let session = told(&rig, &project, BEFORE)?;
    wait_for_the_turn_to_end(&rig, &session, BEFORE)?;
    rig.wait_for("the chief's answer to be copied", secs(60), || {
        Ok(items(&history(&rig, &project)?)
            .iter()
            .any(|item| is_said(item, "assistant", "BEFORE_THE_UPGRADE")))
    })?;
    rig.tell(
        id,
        &format!(
            "DISPATCH --tier {} HANG 600 Reply with exactly: NEVER_OK",
            tier(&added)
        ),
    )?;
    rig.wait_for("the worker to be at work on T-1", secs(60), || {
        Ok(project.lane("worker")?["activity"]["state"] == "working")
    })?;
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let root = rig.close_keeping_the_root()?;
    written_by_an_older_build(&root)?;

    let rig = Rig::start(config().existing_root(root))?;
    let project = Project::new(&rig, id);
    // The board opens, and the project resumes; the chief's window cannot open, and the
    // human is told why and what to do.
    assert_eq!(project.board()?["project"]["id"], id);
    let resumed = rig.page("project.resume", json!({ "project": id }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let cannot = format!("ConsensFlow cannot open {GONE} windows");
    rig.wait_for(
        "the human to be told the chief could not start",
        secs(60),
        || {
            Ok(
                notes(&rig.page("inbox.get", json!({ "project": id }))?["messages"])
                    .iter()
                    .any(|note| {
                        note.starts_with("The chief could not start") && note.contains(&cannot)
                    }),
            )
        },
    )?;
    // History reads what was copied while the chief's window lived, all of one
    // conversation, and names no harness (CHF-11).
    let read = history(&rig, &project)?;
    let copied = items(&read);
    let mut conversations: Vec<&Value> = copied.iter().map(|item| &item["conversation"]).collect();
    conversations.dedup();
    assert_eq!(conversations.len(), 1, "{read}");
    assert!(!read.to_string().contains(GONE), "{read}");
    assert!(
        copied
            .iter()
            .any(|item| is_said(item, "assistant", "BEFORE_THE_UPGRADE")),
        "{read}"
    );
    // T-1's window cannot come back: the task is paused, not left working, and the
    // chief is told why and the way out, which no resume is.
    rig.wait_for("T-1 to be paused", secs(60), || {
        Ok(project.task(1)?["state"] == "paused")
    })?;
    let told_why = |messages: &Value| {
        notes(messages).iter().any(|note| {
            note.starts_with("T-1 is paused")
                && note.contains("window is gone")
                && note.contains(WAY_OUT)
                && !note.contains("cf task resume")
        })
    };
    let task = project.task(1)?;
    assert!(told_why(&task["messages"]), "{task}");
    assert!(
        task["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|message| message["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains(&cannot))),
        "{task}"
    );
    // The agents of the human's own on it are kept, unsupported and hidden.
    let agents = rig.page("agents.list", json!({}))?;
    for name in ["chief", "worker"] {
        let agent = agents["agents"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|agent| agent["name"] == name)
            .ok_or("an agent of the human's own is gone")?;
        assert_eq!(
            (&agent["harness"], &agent["unsupported"], &agent["hidden"]),
            (&json!(GONE), &json!(true), &json!(true)),
            "{agent}"
        );
    }

    // Switch chief takes the project off the harness, and the new chief works.
    let switched = rig.page(
        "chief.switch",
        chief_body(&rig, "next", json!({ "project": id }))?,
    )?;
    assert_eq!(switched["ok"], true, "{switched}");
    rig.wait_for("the new chief's window", secs(60), || {
        Ok(!windows_of(&rig, &project).is_empty())
    })?;
    wait_for_the_chief(&rig, &project)?;
    let window = windows_of(&rig, &project).remove(0);
    let session = after(&window, "--session-id")
        .ok_or("the new chief's window has no session")?
        .to_owned();
    rig.tell(id, AFTER)?;
    wait_for_the_turn_to_end(&rig, &session, AFTER)?;
    assert!(
        told_why(&json!(project.inbox("chief")?)),
        "the new chief is told of T-1"
    );
    // A task for the worker's tier waits: the member is not free, and the chief is told
    // it runs on a harness whose windows ConsensFlow cannot open.
    rig.tell(id, "DISPATCH --tier light Reply with exactly: NEVER_RUN")?;
    let not_free = format!(
        "@worker runs on {GONE}, whose windows ConsensFlow cannot open (remove @worker from the staff and add a member on another harness)"
    );
    rig.wait_for("T-2 to wait for a free worker", secs(60), || {
        Ok(notes(&json!(project.inbox("chief")?))
            .iter()
            .any(|note| note.starts_with("T-2 waits for a free") && note.contains(&not_free)))
    })?;
    assert_eq!(project.task(2)?["state"], "open");
    rig.close()?;
    Ok(())
}
