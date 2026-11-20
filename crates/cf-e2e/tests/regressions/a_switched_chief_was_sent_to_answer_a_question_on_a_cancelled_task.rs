//! A Switch chief's handoff sent the new chief to answer a question on a task
//! that was cancelled four hours before.
//!
//! Seen by the owner on 2026-10-21, in their `sites` project (the 3.0.6 batch). A
//! worker asked the chief a question on T-9 (m-2486). The chief answered it (m-2487)
//! and cancelled T-9 at once, and the cancel took the answer's delivery back: m-2487
//! read `cancelled`. Four hours later the human switched the chief from Codex to
//! Claude Code, and the handoff (m-2594) listed the question under "What waits on you
//! now", with `cf inbox read m-2486, then cf answer m-2486`. The new chief was sent to
//! answer a question nobody waited for. The report is
//! `issues/a-switch-chief-note-lists-a-question-on-a-cancelled-task.md` in consensflow-sme
//! (T-157 on the board).
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

use crate::handoff_questions::{asked, cancelled_question_handoff, handoff_after_a_switch};
use crate::{config, Outcome};
use cf_e2e::rig::Rig;

#[test]
fn a_question_whose_answer_a_cancel_took_back_is_not_in_the_handoff_of_a_switched_chief() -> Outcome
{
    cancelled_question_handoff(false)
}

#[test]
fn a_tracked_question_whose_answer_a_cancel_returned_is_not_in_the_switched_chiefs_handoff(
) -> Outcome {
    cancelled_question_handoff(true)
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
        cf_e2e::rig::names_command(&handoff, &format!("answer m-{question}")),
        "with the command that answers it: {handoff}"
    );
    rig.close()?;
    Ok(())
}
