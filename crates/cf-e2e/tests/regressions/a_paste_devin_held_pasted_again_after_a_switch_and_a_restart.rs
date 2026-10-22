//! A paste a Devin chief held queued went in again after a switch and a restart.
//!
//! Found in review, not in use: the review of 6c8fc595 (the Devin arrival branch,
//! 2026-10-22) and Calliope's look at its third round (T-275), reading the engine on that
//! branch; not seen by the owner, and no issue in consensflow-sme holds it. Devin takes
//! what it is handed in a turn into its agent's queue: its TUI writes the history row at
//! the Enter, and its record shows the message only when the turn takes it in. A result
//! was pasted into a chief's turn that had said nothing yet, the human ran `/new`, and
//! the app quit while the turn went on. The next start settled what was on its way by
//! the record of the conversation the window showed then, the new one, which never
//! holds the paste; and where the conversation it went into had not taken it in yet, by
//! a record that did not show it either. It gave the result back, and pasted it again
//! into the new conversation, while Devin's queue in the old one held it.
//!
//! What should have happened: the result goes into the chief's window once. The ledger
//! keeps the conversation each paste went into, and the start reads that one's record;
//! where its record does not show the paste, Devin's store says whether its TUI took it
//! (its history row), and one it took is delivered.
//!
//! The chief is the stand-in Devin (`fake-devin`), whose `/new` shows the new conversation
//! at once while the turn at work and its queue go on in the one left (Devin's waits for
//! the turn, which no case can time). The human types to the chief a turn whose agent
//! says nothing for two minutes (`THINK`); a worker of the stand-in Claude answers its
//! task, and the daemon pastes the result into that silence once it reads the window at
//! rest. The human runs `/new`, the daemon follows the window, and the app quits, daemon
//! first, with the result on its way. Then:
//!
//! - the conversation left ran the result while no daemon looked: its record holds it
//!   (written into the store here);
//! - the conversation left had not taken it in: its record does not hold it, and the
//!   store's history does.
//!
//! The app starts again on the same home, and the human resumes the project. What proves
//! it is the daemon's own word for its pastes (`delivery.begun`), and the result
//! delivered.

use cf_e2e::devin_store;
use cf_e2e::files;
use cf_e2e::rig::{Config, Project, Rig};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::devin_notes_twice::store_of;
use crate::{config, secs, Outcome, FAKE_DEVIN};

/// What the worker answers its task with.
const WORDS: &str = "RESULT_HELD";

/// What the chief's conversation left did with the result before the app started again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Left {
    /// Its turn took the result in: its record holds it.
    Ran,
    /// Its turn had not taken the result in: only the store's history has it.
    Queued,
}

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

fn a_rig() -> Config {
    config().devin(FAKE_DEVIN)
}

/// The daemon's events of `kind` about the message `id`.
fn of_message(events: &[Value], kind: &str, id: i64) -> usize {
    events
        .iter()
        .filter(|event| event["kind"] == kind && event["data"]["message"] == id)
        .count()
}

/// The worker's result, once the daemon has made it: its message's id.
fn result_id(project: &Project) -> cf_e2e::Result<Option<i64>> {
    Ok(project
        .inbox("chief")?
        .into_iter()
        .find(|message| message["kind"] == "result" && message["body"] == WORDS)
        .and_then(|message| message["id"].as_i64()))
}

/// What the first run of the app left: the project, the result's id, Devin's store, the
/// session the result went into and its words as the TUI submitted them, and the rig's
/// root to start again on.
struct Quit {
    project: i64,
    id: i64,
    store: std::path::PathBuf,
    left: String,
    words: String,
    root: std::path::PathBuf,
}

/// The chief's window is given the result into a silent turn, the human switches it to a
/// new conversation, and the app quits, daemon first.
fn pasted_switched_and_quit(mut rig: Rig) -> cf_e2e::Result<Quit> {
    write_roster(&rig)?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .ok_or_else(|| Error::Daemon(format!("the worker has no tier: {added}")))?
        .to_owned();
    rig.tell_within(
        project.id(),
        "THINK 120 Reply with exactly: BUSY",
        secs(180),
    )?;
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), secs(30))?;
    let brief = format!("SLEEP 2 Reply with exactly: {WORDS}");
    let given = rig.cf_in_window(&chief, ["task", "add", "--tier", &tier, brief.as_str()])?;
    assert_eq!(given.code, Some(0), "{}", given.output());

    // The daemon pastes the result into the silence, and Devin's TUI takes it: its
    // history row, at the Enter, in the conversation the window shows.
    rig.wait_for("the worker's result", secs(120), || {
        Ok(result_id(&project)?.is_some())
    })?;
    let id = result_id(&project)?.ok_or_else(|| Error::Daemon("no result".to_owned()))?;
    let marker = format!("[ConsensFlow m-{id} ");
    let store = store_of(&rig);
    rig.wait_for("Devin's TUI to take the result", secs(120), || {
        Ok(devin_store::history(&store)
            .unwrap_or_default()
            .iter()
            .any(|(_, words)| words.starts_with(&marker)))
    })?;
    let (left, words) = devin_store::history(&store)?
        .into_iter()
        .find(|(_, words)| words.starts_with(&marker))
        .ok_or_else(|| Error::Daemon("no history row of the result".to_owned()))?;
    assert!(
        !devin_store::messages_of(&store, &left)?
            .iter()
            .any(|message| message.text.starts_with(&marker)),
        "the turn took the result in at once: its record holds it already"
    );

    // The human opens a new conversation, and the daemon follows the window to it.
    rig.tell(project.id(), "/new")?;
    rig.wait_for("the daemon to follow the window", secs(60), || {
        Ok(rig.events().iter().any(|event| {
            event["kind"] == "conversation.followed"
                && event["data"]["nativeSession"]
                    .as_str()
                    .is_some_and(|shown| shown != left)
        }))
    })?;
    let on_its_way = project
        .inbox("chief")?
        .into_iter()
        .any(|message| message["id"] == id && message["state"] == "delivering");
    assert!(on_its_way, "the result was settled before the app quit");
    assert_eq!(of_message(&rig.events(), "delivery.begun", id), 1);

    let project = project.id();
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let root = rig.close_keeping_the_root()?;
    Ok(Quit {
        project,
        id,
        store,
        left,
        words,
        root,
    })
}

/// The case: what the conversation left did is `left`; then the app starts again, the
/// human resumes the project, and the result went in once.
fn goes_in_once(ran: Left) -> Outcome {
    let quit = pasted_switched_and_quit(Rig::start(a_rig())?)?;
    let id = quit.id;
    // Written before the daemon starts, which settles what was on its way at once.
    if ran == Left::Ran {
        devin_store::append_user(&quit.store, &quit.left, &quit.words)?;
    }
    let rig = Rig::start(a_rig().existing_root(quit.root))?;
    let resumed = rig.page("project.resume", json!({ "project": quit.project }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    let project = Project::new(&rig, quit.project);
    // A result given back is delivered once it is pasted again into the window opened
    // again.
    rig.wait_for("the result to be delivered", secs(180), || {
        Ok(project
            .inbox("chief")?
            .into_iter()
            .any(|message| message["id"] == id && message["state"] == "delivered"))
    })?;
    rig.wait_for(
        "the chief's window to be open again and idle",
        secs(180),
        || Ok(project.lane("chief")?["activity"]["state"] == "idle"),
    )?;
    // The daemon's passes are a second apart.
    std::thread::sleep(secs(10));
    let events = rig.events();
    let said: Vec<&Value> = events
        .iter()
        .filter(|event| event["data"]["message"] == id)
        .collect();
    assert_eq!(
        of_message(&events, "delivery.begun", id),
        1,
        "the result (m-{id}) was pasted again after the restart: {said:?}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_result_the_conversation_left_ran_while_the_app_was_down_goes_into_a_devin_chief_once(
) -> Outcome {
    goes_in_once(Left::Ran)
}

#[test]
fn a_result_devin_held_queued_in_the_conversation_left_goes_into_a_devin_chief_once() -> Outcome {
    goes_in_once(Left::Queued)
}
