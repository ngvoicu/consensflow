//! A Switch chief's handoff sent the new chief to answer a question on a task
//! that was cancelled four hours before.
//!
//! Seen by the owner on 2026-10-21, in their `sites` project (the 3.0.6 batch). A
//! worker asked the chief a question on T-9 (m-2486). The chief answered it (m-2487)
//! and cancelled T-9 at once, and the cancel took the answer's delivery back: m-2487
//! read `cancelled`. Four hours later the human switched the chief from Codex to
//! Claude Code, and the handoff (m-2594) listed the question under "What waits on you
//! now", with `cf inbox read m-2486, then cf answer m-2486`. The new chief was sent to
//! answer a question nobody waited for. The report is T-157 on the board.
//!
//! What the owner should have seen: the handoff lists what waits on the chief, and a
//! question on a task that is over does not. A question on a task that still waits is
//! listed as it always was.
//!
//! The cases are that shape on the rig. A stand-in worker asks through `cf ask`, as the
//! worker role tells it to; the chief answers with `cf answer` and cancels with `cf task
//! cancel` straight after, as the field's chief did, from its own window; the worker's
//! window is at work on a turn of the human's while they do, so the answer is still in
//! the queue when the cancel takes it back (the case checks that it reads `cancelled`
//! before it goes on, so a lost race fails there and not as a pass); the human switches
//! the chief to the other agent of the staff, and the new chief's first message is what
//! its record holds.

use std::error::Error;

use cf_e2e::rig::{Project, Rig};
use serde_json::{json, Value};

use crate::resume_of_a_chief_with_no_turn::{after, wait_for_the_chief, windows_of};
use crate::{config, secs, Outcome};

/// What the worker puts to the chief, on the task it was given.
const QUESTION: &str = "Which grammar?";

/// A board with T-1 given to a stand-in worker, which asked the chief `QUESTION`
/// through `cf ask`, and the chief was given it. The project, the question's id and the
/// worker's session.
fn asked(rig: &Rig) -> Result<(Project<'_>, i64, String), Box<dyn Error>> {
    let project = Project::open(rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    wait_for_the_chief(rig, &project)?;
    rig.tell(
        project.id(),
        &format!(
            "DISPATCH --tier {} CF ask :: {QUESTION}",
            added["member"]["tier"].as_str().unwrap_or_default()
        ),
    )?;
    rig.wait_for("the question to be given to the chief", secs(60), || {
        Ok(project
            .inbox("chief")?
            .iter()
            .any(|m| m["kind"] == "question" && m["state"] == "delivered"))
    })?;
    let question = project
        .inbox("chief")?
        .into_iter()
        .find(|m| m["kind"] == "question")
        .ok_or("no question in the chief's inbox")?;
    let session = project.lane("worker")?["participant"]["handle"]
        .as_str()
        .ok_or("the worker has no session")?
        .to_owned();
    assert_eq!(question["taskNumber"], 1, "{question}");
    assert_eq!(question["sender"], session.as_str(), "{question}");
    let id = question["id"].as_i64().ok_or("the question has no id")?;
    Ok((project, id, session))
}

/// The human switches the chief to the staff's other agent, and the new chief's window
/// has been told it is the chief: that message, as its record holds it.
fn handoff_after_a_switch(rig: &Rig, project: &Project) -> Result<String, Box<dyn Error>> {
    wait_for_the_chief(rig, project)?;
    let switched = rig.page(
        "chief.switch",
        json!({ "project": project.id(), "agent": "worker" }),
    )?;
    assert_eq!(switched["ok"], true, "{switched}");
    rig.wait_for("a second chief window to be opened", secs(60), || {
        Ok(windows_of(rig, project).len() == 2)
    })?;
    let second = windows_of(rig, project).remove(1);
    let session = after(&second, "--session-id")
        .ok_or("the new chief's window has no session")?
        .to_owned();
    rig.wait_for("the new chief to be told it is the chief", secs(60), || {
        Ok(rig.transcript(&session).contains("You are the chief now"))
    })?;
    Ok(rig.transcript(&session))
}

#[test]
fn a_question_whose_answer_a_cancel_took_back_is_not_in_the_handoff_of_a_switched_chief() -> Outcome
{
    let rig = Rig::start(config())?;
    let (project, question, session) = asked(&rig)?;
    let chief = windows_of(&rig, &project)
        .into_iter()
        .next()
        .ok_or("the chief has no window")?;

    // The worker's window is at work on a turn of the human's, so what is put to it
    // waits in its queue.
    let frame = rig.open_frame(&format!("p{}-{session}", project.id()), secs(30))?;
    let typed = rig.host(
        "pane.input",
        json!({
            "id": frame["id"],
            "generation": frame["generation"],
            "bytes": "SLEEP 90 resting\r".as_bytes(),
        }),
    )?;
    assert_eq!(typed["ok"], true, "{typed}");
    rig.wait_for("the worker's window to be at work", secs(30), || {
        Ok(project.lane(&session)?["activity"]["state"] != "idle")
    })?;

    // The chief answers and cancels at once, as the field's chief did.
    let answered = rig.cf_in_window(
        &chief,
        ["answer", &format!("m-{question}"), "Use the new grammar"],
    )?;
    assert_eq!(answered.code, Some(0), "{answered}");
    let cancelled = rig.cf_in_window(&chief, ["task", "cancel", "T-1"])?;
    assert_eq!(cancelled.code, Some(0), "{cancelled}");

    // The shape of the field (m-2486, m-2487): the question was delivered and stays so;
    // its answer reads `cancelled`, and its task is cancelled.
    let answer = project
        .inbox(&session)?
        .into_iter()
        .find(|m| m["kind"] == "answer")
        .ok_or("the chief's answer is in nobody's inbox")?;
    assert_eq!(
        answer["state"], "cancelled",
        "the setup needs the answer taken back by the cancel: {answer}"
    );
    assert_eq!(project.task(1)?["state"], "cancelled");
    let asked = project
        .inbox("chief")?
        .into_iter()
        .find(|m| m["id"] == question)
        .unwrap_or(Value::Null);
    assert_eq!(asked["state"], "delivered", "{asked}");

    let handoff = handoff_after_a_switch(&rig, &project)?;
    assert!(
        handoff.contains("Nothing on the board waits on you."),
        "the handoff says nothing waits: {handoff}"
    );
    assert!(
        !handoff.contains(&format!("cf answer m-{question}")),
        "the new chief is not sent to answer m-{question}: {handoff}"
    );
    rig.close()?;
    Ok(())
}

#[test]
fn a_question_on_a_task_that_still_waits_is_in_the_handoff_of_a_switched_chief() -> Outcome {
    let rig = Rig::start(config())?;
    let (project, question, session) = asked(&rig)?;
    assert_eq!(project.task(1)?["state"], "waiting");

    let handoff = handoff_after_a_switch(&rig, &project)?;
    assert!(
        handoff.contains("What waits on you now:"),
        "the handoff lists what waits: {handoff}"
    );
    assert!(
        handoff.contains(&format!("@{session} asks on T-1: ")),
        "the question is listed: {handoff}"
    );
    assert!(
        handoff.contains(&format!("cf answer m-{question}")),
        "with the command that answers it: {handoff}"
    );
    rig.close()?;
    Ok(())
}
