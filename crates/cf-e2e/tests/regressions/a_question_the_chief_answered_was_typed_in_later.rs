//! A question the chief had already read and answered was typed into its
//! window later, as if it were new.
//!
//! Seen on 2026-10-22 on this board (ConsensFlow 3.0.5). A member's question
//! (m-2927 from T-99, m-2931 from T-236, m-3103 from T-184) waited in the chief's
//! queue while the chief was in a long turn. The chief read it with `cf inbox read
//! m-…` and answered it with `cf answer m-…`, and the member got the answer. When the
//! chief's turn ended, the same question was typed into its terminal, headed
//! `[ConsensFlow m-… · T-… · question …]`, with "Run in your shell: cf answer m-…".
//! The report is `issues/a-question-the-chief-answered-is-still-typed-in-later.md` in
//! consensflow-sme (T-260 on the board).
//!
//! What the owner should have seen: what the chief read with `cf inbox read` is the
//! chief's, and is not typed in after; a question it answered is never typed in.
//! Nor is a result it read there: it has it, and the board shows it read.
//!
//! The cases are that shape on the rig. The stand-in chief hands out the work, and
//! the human then gives it a long turn of their own; the stand-in worker asks through
//! `cf ask`, as the worker role tells it to, or finishes, while that turn goes on (the
//! case checks that what it sent is still queued, so a lost race fails there and not
//! as a pass). The chief runs `cf inbox read` and `cf answer` from its own window
//! while its turn goes on, as the field's chief did. When the turn ends, a later
//! message to the chief (the worker's next result) is pasted, and so the chief's
//! record shows whatever came before it.

use std::error::Error;

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::resume_of_a_chief_with_no_turn::{after, wait_for_the_chief, windows_of};
use crate::{config, secs, Outcome};

/// What the worker puts to the chief.
const QUESTION: &str = "Which grammar?";

/// A board with a stand-in worker, and the chief's window: the project, the
/// worker's tier, the chief's window and its session.
fn staffed(rig: &Rig) -> Result<(Project<'_>, String, Value, String), Box<dyn Error>> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    wait_for_the_chief(rig, &project)?;
    let tier = added["member"]["tier"]
        .as_str()
        .ok_or("the worker has no tier")?
        .to_owned();
    let chief = windows_of(rig, &project)
        .into_iter()
        .next()
        .ok_or("the chief has no window")?;
    let session = after(&chief, "--session-id")
        .ok_or("the chief's window has no session")?
        .to_owned();
    Ok((project, tier, chief, session))
}

/// The chief hands out what `dispatches` says, one task to a line, and the human
/// then gives it a turn of their own that lasts `seconds`.
fn handed_out_then_busy(
    rig: &Rig,
    project: &Project,
    dispatches: &str,
    seconds: u64,
) -> Result<(), Box<dyn Error>> {
    rig.tell(project.id(), dispatches)?;
    rig.wait_for("the work to be on the board", secs(60), || {
        Ok(!project.task(1)?.is_null())
    })?;
    rig.tell(project.id(), &format!("SLEEP {seconds} resting"))?;
    rig.wait_for("the chief's window to be at work", secs(30), || {
        Ok(project.lane("chief")?["activity"]["state"] != "idle")
    })?;
    Ok(())
}

/// The first message of `kind` in `handle`'s inbox once one is there (in whatever
/// state), as the ledger has it then.
fn first_of(
    rig: &Rig,
    project: &Project,
    handle: &str,
    kind: &str,
) -> Result<Value, Box<dyn Error>> {
    let find = || -> cf_e2e::Result<Option<Value>> {
        Ok(project
            .inbox(handle)?
            .into_iter()
            .filter(|m| m["kind"] == kind)
            .min_by_key(|m| m["id"].as_i64()))
    };
    rig.wait_for(&format!("a {kind} for @{handle}"), secs(90), || {
        Ok(find()?.is_some())
    })?;
    Ok(find()?.ok_or("gone")?)
}

/// The message `id` in `handle`'s inbox as the ledger has it now; null when it
/// is not there.
fn now(project: &Project, handle: &str, id: i64) -> cf_e2e::Result<Value> {
    Ok(project
        .inbox(handle)?
        .into_iter()
        .find(|m| m["id"] == id)
        .unwrap_or(Value::Null))
}

/// `cf` in the chief's window, as it would type it: it ran.
fn chief_runs(rig: &Rig, chief: &Value, words: &[&str]) -> Result<String, Box<dyn Error>> {
    let ran = rig.cf_in_window(chief, words.iter().copied())?;
    assert_eq!(ran.code, Some(0), "{ran}");
    Ok(ran.stdout)
}

/// Waits until message `id` was pasted into the chief's window, once its turn
/// ended: what was still queued before it was pasted before it.
fn pasted_to_the_chief(rig: &Rig, project: &Project, id: i64) -> Result<(), Box<dyn Error>> {
    rig.wait_for(
        &format!("m-{id} to be pasted into the chief"),
        secs(180),
        || {
            Ok(matches!(
                now(project, "chief", id)?["state"].as_str(),
                Some("delivered" | "read")
            ))
        },
    )?;
    wait_for_the_chief(rig, project)?;
    Ok(())
}

#[test]
fn a_question_the_chief_read_and_answered_in_its_turn_is_not_typed_in_when_the_turn_ends() -> Outcome
{
    let rig = Rig::start(config())?;
    let (project, tier, chief, session) = staffed(&rig)?;
    handed_out_then_busy(
        &rig,
        &project,
        &format!("DISPATCH --tier {tier} SLEEP 10 CF ask :: {QUESTION}"),
        60,
    )?;

    // The worker asks while the chief's turn goes on: the question waits.
    let question = first_of(&rig, &project, "chief", "question")?;
    let id = question["id"].as_i64().ok_or("the question has no id")?;
    assert_eq!(
        question["state"], "queued",
        "the setup needs it queued: {question}"
    );
    assert_ne!(
        project.lane("chief")?["activity"]["state"],
        "idle",
        "the setup needs the chief in its turn"
    );

    // The chief reads it in its shell and answers it, as the field's chief did.
    let read = chief_runs(&rig, &chief, &["inbox", "read", &format!("m-{id}")])?;
    assert!(read.contains(QUESTION), "{read}");
    chief_runs(
        &rig,
        &chief,
        &["answer", &format!("m-{id}"), "Use the new grammar"],
    )?;

    // The worker gets the answer, and its result comes after the question.
    let result = first_of(&rig, &project, "chief", "result")?;
    let after_it = result["id"].as_i64().ok_or("the result has no id")?;
    assert!(after_it > id);
    pasted_to_the_chief(&rig, &project, after_it)?;

    let record = rig.transcript(&session);
    assert!(
        !record.contains(&format!("[ConsensFlow m-{id} ")),
        "the question the chief answered is not typed in: {record}"
    );
    assert!(
        !record.contains(&format!("cf answer m-{id}")),
        "nor is the chief told to answer it: {record}"
    );
    let question = now(&project, "chief", id)?;
    assert_eq!(question["state"], "read", "{question}");
    rig.close()?;
    Ok(())
}

#[test]
fn a_result_the_chief_read_in_its_turn_is_not_typed_in_when_the_turn_ends() -> Outcome {
    let rig = Rig::start(config())?;
    let (project, tier, chief, session) = staffed(&rig)?;
    handed_out_then_busy(
        &rig,
        &project,
        &format!(
            "DISPATCH --tier {tier} SLEEP 10 Reply with exactly: PARSED\n\
             DISPATCH --tier {tier} SLEEP 15 Reply with exactly: LEXED"
        ),
        60,
    )?;

    // T-1's result comes while the chief's turn goes on: it waits.
    let result = first_of(&rig, &project, "chief", "result")?;
    let id = result["id"].as_i64().ok_or("the result has no id")?;
    assert_eq!(result["taskNumber"], 1, "{result}");
    assert_eq!(
        result["state"], "queued",
        "the setup needs it queued: {result}"
    );
    assert_ne!(
        project.lane("chief")?["activity"]["state"],
        "idle",
        "the setup needs the chief in its turn"
    );

    // The chief reads it in its shell, and decides nothing yet.
    let read = chief_runs(&rig, &chief, &["inbox", "read", &format!("m-{id}")])?;
    assert!(read.contains("PARSED"), "{read}");

    // T-2's result, which the chief did not read, comes after it, and is pasted.
    rig.wait_for("T-2's result", secs(120), || {
        Ok(project
            .inbox("chief")?
            .iter()
            .any(|m| m["kind"] == "result" && m["taskNumber"] == 2))
    })?;
    let second = project
        .inbox("chief")?
        .into_iter()
        .find(|m| m["kind"] == "result" && m["taskNumber"] == 2)
        .ok_or("no result of T-2")?;
    let second = second["id"].as_i64().ok_or("the result has no id")?;
    pasted_to_the_chief(&rig, &project, second)?;

    let record = rig.transcript(&session);
    assert!(
        !record.contains(&format!("[ConsensFlow m-{id} ")),
        "the result the chief read is not typed in: {record}"
    );
    assert!(
        record.contains(&format!("[ConsensFlow m-{second} ")),
        "the one it did not read is: {record}"
    );
    let result = now(&project, "chief", id)?;
    assert_eq!(result["state"], "read", "the board shows it read: {result}");
    assert_eq!(project.task(1)?["state"], "done", "a read is no decision");
    rig.close()?;
    Ok(())
}
