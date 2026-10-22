//! A message the chief took was typed into its window again when the Mac was loaded.
//!
//! Seen on 2026-10-22 on this board, ConsensFlow 3.0.5, at a load average of about 100
//! (nine worker builds and a landing gate). m-3093, T-243's brief to the chief, was
//! handed over at 14:21:45Z. The pane host then answered nothing in time for three
//! minutes: the daemon logged passes of 30017 ms and 37438 ms, each a snapshot at the
//! bridge's 30 s deadline, and a paste held at 14:24:33Z because "the window cannot be
//! read: deadline". The chief's own record and status read at rest throughout, and its
//! record showed nothing of the brief, which had not reached Claude. The arrival window
//! ran on through the stalls; at 14:24:03Z the daemon took the paste for lost
//! (`delivery.retried`), and at 14:24:42Z typed it again. Both copies reached the chief:
//! the first began its turn at 14:24:42.689Z, the second went into that turn's queue
//! 3 s later. The report is the issue
//! `issues/a-message-the-chief-took-is-typed-in-again-under-load.md` in consensflow-sme.
//!
//! What should have been seen: the brief in the chief's window once, whenever the host
//! came back.
//!
//! The case is that shape on the rig: the stand-in Claude chief hands a worker a task,
//! and the pane host stalls at the paste of the worker's result into the chief
//! (`Rig::stall_host_at_paste`) for longer than the paste's deadline, the daemon's
//! minute and the Enter's snapshot together: the paste and every frame between the
//! daemon and the host after it wait in the rig, then go on in the order they came.
//! The result must be pasted once, and be in the chief's record once.

use std::thread::sleep;
use std::time::Duration;

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::a_compact_typed_into_an_idle_chief_held_a_workers_result::tier;
use crate::resume_of_a_chief_with_no_turn::{after, wait_for_the_chief, windows_of};
use crate::{config, secs, Outcome};

/// How long the pane host answers nothing: past the paste's 30 s deadline, two of the
/// Enter's snapshots at their 30 s deadlines and the daemon's minute of the window's rest
/// after them, so that a daemon that pastes again without asking the host once more
/// does so inside it.
const STALL: Duration = Duration::from_secs(200);

/// Words only the paste of a result to the chief holds: what it decides with.
const RESULT_WORDS: &str = "cf task accept T-1";

/// The daemon's events of `kind` about the message `id`.
fn of_message<'a>(events: &'a [Value], kind: &str, id: i64) -> Vec<&'a Value> {
    events
        .iter()
        .filter(|event| event["kind"] == kind && event["data"]["message"] == id)
        .collect()
}

/// How many of the prompts in a Claude record hold `words`: what the window was given,
/// not the stand-in's replies, which quote the header they answer.
fn prompts_holding(record: &str, words: &str) -> usize {
    record
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| row["type"] == "user" && row.to_string().contains(words))
        .count()
}

#[test]
fn a_result_handed_to_a_claude_chief_while_its_pane_host_answers_nothing_is_pasted_once() -> Outcome
{
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let session = windows_of(&rig, &project)
        .first()
        .and_then(|first| after(first, "--session-id"))
        .ok_or("the chief's window has no session")?
        .to_owned();
    wait_for_the_chief(&rig, &project)?;
    rig.stall_host_at_paste(RESULT_WORDS, STALL);
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} Reply with exactly: WORKER_OK",
            tier(&added)
        ),
    )?;
    rig.wait_for("T-1 to be done", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for("the pane host's stall to be over", STALL + secs(60), || {
        Ok(rig.stall_over())
    })?;
    let id = project
        .inbox("chief")?
        .iter()
        .find(|message| message["kind"] == "result")
        .and_then(|message| message["id"].as_i64())
        .ok_or("the chief was given no result")?;
    let header = format!("[ConsensFlow m-{id} ");
    rig.wait_for(
        "the worker's result to be in the chief's record",
        secs(60),
        || Ok(rig.transcript(&session).contains(&header)),
    )?;
    // A second paste would follow the first within seconds of the stall's end.
    wait_for_the_chief(&rig, &project)?;
    sleep(secs(10));
    let events = rig.events();
    assert_eq!(
        of_message(&events, "delivery.begun", id).len(),
        1,
        "m-{id} was pasted more than once; the daemon took it for lost: {:?}",
        of_message(&events, "delivery.retried", id)
    );
    assert_eq!(
        prompts_holding(&rig.transcript(&session), &header),
        1,
        "the chief was given m-{id} more than once"
    );
    rig.close()?;
    Ok(())
}
