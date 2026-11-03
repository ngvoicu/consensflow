//! A worker's whole write-up was lost from the board because a question of its was still out
//! when its turn ended.
//!
//! Seen on this board, 2026-10-28, ConsensFlow 3.0.6 (T-837; the triage T-1201 found it again
//! as T-846, T-1204). The worker asked the chief a question that needed no answer (`cf ask`:
//! "disk notice, no answer needed") and went on in the same turn; its task read `waiting`.
//! The chief answered three minutes later; the answer waited for the window's rest. The turn
//! ended with a 5.8 KB write-up, then the answer went in, and the turn it began ended with
//! five lines ("I'd already finished… My result above still stands"). Those five lines were
//! the task's result; the write-up was in the window's record and nowhere on the board, and
//! the chief took it out of the record by hand.
//!
//! What the owner should have seen: a result that begins with the write-up, labelled as written
//! before the answer to the question went in, and ends with what the worker said last. While
//! the question is out the task waits and nothing is recorded; a worker that asks and stops
//! (the common case) has the reply alone for its result, as it always had.
//!
//! The worker is the stand-in, whose `ASKING <question> THEN <line>` runs the real `cf ask`,
//! writes the call and the output `cf ask` printed as Claude Code records them, and goes on
//! with its line; the chief is the stand-in too, which runs `cf answer` as the case tells it,
//! while the worker's turn goes on, so that the answer waits for the window's rest as the
//! field's did.

use std::error::Error;

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::a_workers_still_going_was_given_as_its_result::{tier, told};
use crate::{config, secs, Outcome};

/// What the worker's turn ends with: the write-up, in one line, as the stand-in says one.
const WRITE_UP: &str = "T-837: the builds disk filled while two trees built. Fixes: builds-disk.sh frees a tree's folder. Reds: none left. Open points: the notice should name the tree.";

/// A board whose worker is given T-1 with `line`: the project, and the worker's tier.
fn given<'a>(rig: &'a Rig, line: &str) -> cf_e2e::Result<Project<'a>> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added: Value = project.add_member("worker")?;
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {} {line}", tier(&added)),
    )?;
    Ok(project)
}

/// The handle of the window that works on T-1.
fn worker_of(project: &Project) -> Result<String, Box<dyn Error>> {
    Ok(project.task(1)?["assignee"]
        .as_str()
        .ok_or("T-1 has no assignee")?
        .to_owned())
}

/// The worker's question, once it is on the board.
fn the_question(rig: &Rig, project: &Project) -> Result<Value, Box<dyn Error>> {
    let find = || -> cf_e2e::Result<Option<Value>> {
        Ok(project
            .inbox("chief")?
            .into_iter()
            .find(|message| message["kind"] == "question"))
    };
    rig.wait_for("the worker's question", secs(60), || Ok(find()?.is_some()))?;
    Ok(find()?.ok_or("gone")?)
}

#[test]
fn a_write_up_the_turn_ended_with_while_the_question_was_out_is_in_the_result_before_the_reply(
) -> Outcome {
    let rig = Rig::start(config())?;
    // The worker asks, works for a while (the chief answers meanwhile), and ends its turn with
    // its write-up.
    let project = given(
        &rig,
        &format!("ASKING Disk notice, no answer needed THEN PACED 3 4 {WRITE_UP}"),
    )?;
    let question = the_question(&rig, &project)?;
    let id = question["id"].as_i64().ok_or("the question has no id")?;
    assert_eq!(project.task(1)?["state"], "waiting");
    let worker = worker_of(&project)?;
    rig.tell(project.id(), &format!("CF answer m-{id} :: Noted."))?;
    rig.wait_for("the answer to be on the board", secs(60), || {
        Ok(project
            .inbox(&worker)?
            .iter()
            .any(|message| message["kind"] == "answer"))
    })?;
    assert_eq!(
        project.task(1)?["state"],
        "waiting",
        "the answer waits for the window's rest: the order of T-837"
    );

    rig.wait_for("T-1 to be done", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let results: Vec<String> = told(&project)?
        .into_iter()
        .filter(|(kind, _)| kind == "result")
        .map(|(_, body)| body)
        .collect();
    assert_eq!(results.len(), 1, "{results:?}");
    let head = format!(
        "Written before the answer to its question m-{id} went in:\n\n{WRITE_UP}\n\nWhat it said last:\n\nnoted: "
    );
    assert!(
        results[0].starts_with(&head),
        "the write-up, then the reply: {results:?}"
    );
    rig.close()?;
    Ok(())
}

/// The common case, on the real daemon: a worker that asks and ends its turn on the question.
/// Its task waits, nothing is recorded, and the reply to the answer is the result, as it was.
#[test]
fn a_worker_that_asks_and_stops_has_the_reply_alone_for_its_result() -> Outcome {
    let rig = Rig::start(config())?;
    let project = given(
        &rig,
        "ASKING Which grammar? THEN SAY Asked. Waiting for the answer.",
    )?;
    let question = the_question(&rig, &project)?;
    let id = question["id"].as_i64().ok_or("the question has no id")?;
    let worker = worker_of(&project)?;
    rig.wait_for("the worker's turn to end", secs(60), || {
        Ok(project.lane(&worker)?["activity"]["state"] == "idle")
    })?;
    assert_eq!(project.task(1)?["state"], "waiting", "not done");
    assert_eq!(
        told(&project)?
            .iter()
            .map(|(kind, _)| kind.as_str())
            .collect::<Vec<_>>(),
        ["question"],
        "nothing is recorded or told for a turn that ended with its question out"
    );

    rig.tell(project.id(), &format!("CF answer m-{id} :: The second."))?;
    rig.wait_for("T-1 to be done", secs(120), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    let results: Vec<String> = told(&project)?
        .into_iter()
        .filter(|(kind, _)| kind == "result")
        .map(|(_, body)| body)
        .collect();
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(
        results[0].starts_with("noted: "),
        "the reply alone, no write-up in front of it: {results:?}"
    );
    rig.close()?;
    Ok(())
}
