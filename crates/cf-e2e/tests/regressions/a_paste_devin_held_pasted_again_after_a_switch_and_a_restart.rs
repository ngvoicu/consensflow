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
//! What should have happened: the ledger keeps the conversation each paste went into,
//! and the start reads that one's record. A result it shows went in, and goes in no more.
//! One it does not show, though Devin's store says its TUI took it (its history row), is
//! sent again, its header saying it was sent again after a restart and may be there
//! already: the window that held it queued died with the app's quit, most likely losing
//! it, and a result confirmed and lost is one the chief never reads, where one read twice
//! is seen as such (the chief's call on T-280).
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
//! On the installed Devin, 3000.12.43 (T-1423), the window reads at work from the human's
//! message said back to its turn's `cause`, so the daemon can no longer paste into the silence of
//! that turn: it waits for the turn's end, and the result is not held in anything. What brings the
//! danger about there is the gap before the message is said back (0.2 to 2.1 s live): the paste
//! goes in behind the human's message, which the wire has not said yet, and Devin holds it queued
//! in the turn. The stand-in holds the turn in that gap until the paste has come
//! (`FAKE_DEVIN_GAP`), the case asserts the paste came before the message was said back, and
//! the rest (the switch, the quit, the start, what the daemon asks of which conversation's
//! record) is as it is: the lines it guards are the ledger's session of each paste and the
//! start's read of that session's record, neither of which reads the wire's turn. A quit inside
//! a turn and a switch of conversation are the other two ways in, and the case holds both.
//!
//! The app starts again on the same home, and the human resumes the project. What proves
//! it is the daemon's own word for its pastes (`delivery.begun`), the result delivered,
//! and what the chief's store holds of it.

use cf_e2e::devin_store;
use cf_e2e::live::trace::at_ms;
use cf_e2e::rig::{Config, Project, Rig};
use cf_e2e::Error;
use serde_json::{json, Value};

use crate::devin_notes_twice::{said_back_at, store_of};
use crate::{devin_config, installed_devin, on_the_installed_devin, secs, Outcome};

/// What the worker answers its task with.
const WORDS: &str = "RESULT_HELD";

/// What the header of a result sent again after a restart says.
const SENT_AGAIN: &str = "sent again after a restart, it may be there already";

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
    rig.stand_ins(&roster)
}

fn a_rig() -> Config {
    devin_config()
}

/// The human's message the first run's gap holds on the installed Devin: its word.
const HELD_IN_THE_GAP: &str = "THINK";

/// [`a_rig`] for the first run of the app, and on the installed Devin the gap: the human's
/// turn is not said back until the daemon's paste has come in behind it.
fn a_rig_in_the_gap() -> Config {
    if installed_devin() {
        a_rig().var("FAKE_DEVIN_GAP", HELD_IN_THE_GAP)
    } else {
        a_rig()
    }
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

/// What the first run of the app left on its way: the project, the result's id, Devin's
/// store, and the session the result went into and its words as the TUI submitted them.
struct Pasted {
    project: i64,
    id: i64,
    store: std::path::PathBuf,
    left: String,
    words: String,
}

/// The chief's window of `rig` is given the result into a silent turn, and the human
/// switches it to a new conversation, the result still on its way.
fn pasted_and_switched(rig: &Rig) -> cf_e2e::Result<Pasted> {
    write_roster(rig)?;
    let project = Project::open(rig, "chief", json!({}))?;
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
    let store = store_of(rig);
    rig.wait_for("Devin's TUI to take the result", secs(120), || {
        Ok(devin_store::history(&store)
            .unwrap_or_default()
            .iter()
            .any(|(_, words)| words.starts_with(&marker)))
    })?;
    // On the installed Devin the window reads at work from the human's message said back to
    // its turn's end, so the paste goes in only where it is not said back yet: the gap.
    if installed_devin() {
        rig.wait_for("the human's turn to be said back", secs(60), || {
            Ok(said_back_at(rig, HELD_IN_THE_GAP).is_some())
        })?;
        let pasted = rig
            .events()
            .iter()
            .filter(|event| event["kind"] == "delivery.begun" && event["data"]["message"] == id)
            .find_map(at_ms);
        let said = said_back_at(rig, HELD_IN_THE_GAP);
        assert!(
            matches!((pasted, said), (Some(pasted), Some(said)) if pasted < said),
            "the paste of m-{id} did not come before the human's turn was said back, so it is no paste in the gap: pasted {pasted:?}, said back {said:?}"
        );
    }
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

    Ok(Pasted {
        project: project.id(),
        id,
        store,
        left,
        words,
    })
}

/// The case: what the conversation left did is `left`; then the app starts again, and
/// the human resumes the project. A result the conversation left ran went in once; one
/// it held queued goes in once more, saying it was sent again.
fn after_the_restart(ran: Left) -> Outcome {
    let mut rig = Rig::start(a_rig_in_the_gap())?;
    let quit = pasted_and_switched(&rig)?;
    let id = quit.id;
    // The app quits, daemon first, with the result on its way.
    rig.kill_daemon();
    rig.wait_for_daemon_exit(secs(10))?;
    let root = rig.close_keeping_the_root()?;
    // Written before the daemon starts, which settles what was on its way at once.
    if ran == Left::Ran {
        devin_store::append_user(&quit.store, &quit.left, &quit.words)?;
    }
    let rig = Rig::start(a_rig().existing_root(root))?;
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
    let pastes = if ran == Left::Ran { 1 } else { 2 };
    assert_eq!(
        of_message(&events, "delivery.begun", id),
        pastes,
        "the result (m-{id}) was not pasted {pastes} times in all: {said:?}"
    );
    let marker = format!("[ConsensFlow m-{id} ");
    let headers: Vec<String> = devin_store::messages(&quit.store)?
        .into_iter()
        .filter(|message| message.role == "user" && message.text.starts_with(&marker))
        .filter_map(|message| message.text.lines().next().map(str::to_owned))
        .collect();
    let sent_again = headers
        .iter()
        .filter(|header| header.contains(SENT_AGAIN))
        .count();
    assert_eq!(
        sent_again,
        pastes - 1,
        "the result (m-{id}) went again saying so only where the conversation left held it queued: {headers:?}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_result_the_conversation_left_ran_while_the_app_was_down_goes_into_a_devin_chief_once(
) -> Outcome {
    after_the_restart(Left::Ran)
}

#[test]
fn a_result_devin_held_queued_in_the_conversation_left_goes_again_into_a_devin_chief_saying_so(
) -> Outcome {
    after_the_restart(Left::Queued)
}

// The same on the wire log of Devin 3000.12.43, the installed Devin (T-1423), where the
// window reads at work through the human's turn: the result can be pasted into it only in
// the gap before the turn is said back (0.2 to 2.1 s live), and the stand-in holds the turn in
// that gap until the paste has come (`FAKE_DEVIN_GAP`). What the switch, the quit and the
// restart do to a paste Devin holds queued, and what the app does with it after, is the
// case as it is.
#[test]
fn a_result_the_conversation_left_ran_while_the_app_was_down_goes_into_a_devin_chief_once_on_devin_3000_12(
) -> Outcome {
    on_the_installed_devin(
        a_result_the_conversation_left_ran_while_the_app_was_down_goes_into_a_devin_chief_once,
    )
}

#[test]
fn a_result_devin_held_queued_in_the_conversation_left_goes_again_into_a_devin_chief_saying_so_on_devin_3000_12(
) -> Outcome {
    on_the_installed_devin(
        a_result_devin_held_queued_in_the_conversation_left_goes_again_into_a_devin_chief_saying_so,
    )
}
