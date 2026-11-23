//! `POST /api/questions`: a member's question for the chief, which the member's
//! task waits on. The chief asks the human in its own terminal, and is refused
//! here before the body is read. The door waiting for the answer is
//! [`super::door`]. A window the pause has not stopped yet is still on its
//! task, and its question is about it, born with its door shut when the turn
//! that asks is an old one.

use cf_ledger::NewQuestion;
use serde_json::{json, Value};

use super::{Answer, Caller, Context, Failure, Request};
use crate::api::views::{value, MessageSummary};

pub(super) async fn ask(
    context: &Context,
    caller: &Caller,
    mut request: Request,
) -> Result<Answer, Failure> {
    // The human works with the chief in its terminal and is asked there.
    if caller.participant.role == "chief" {
        return Err(Failure::refuse(
            403,
            "ask-in-your-terminal",
            "ask the human here in your terminal: they read and answer you there",
        ));
    }
    let body = request.json().await?;
    // The window can close while the request body is in flight. A newly opened
    // window of the same session must not inherit this request.
    if context.credentials.resolve(request.bearer()).is_none() {
        return Err(Failure::refuse(
            409,
            "question-window-closed",
            "this window closed before its question could be placed",
        ));
    }
    let asked = context.ledger.borrow_mut().ask_from_window(
        caller.project.id,
        caller.participant.id,
        &NewQuestion {
            from: Some(caller.participant.handle.clone()),
            to: "chief".to_owned(),
            body: body.get("body").and_then(Value::as_str).map(str::to_owned),
            task: None,
            questions: body.get("questions").cloned(),
            urgent: false,
        },
        caller.question_holding,
        caller.question_launch_task,
        cf_ledger::QuestionStamp {
            // When the asking process began, on the machine's own clock.
            asked_at: body
                .get(cf_proto::questions::ASKED_AT)
                .and_then(Value::as_i64)
                .filter(|at| *at > 0),
            idle_generation: caller.question_idle_generation,
        },
    )?;
    (context.kick)();
    Ok(Answer::created(
        json!({ "message": value(&MessageSummary::from(&asked))? }),
    ))
}

/// A member's window with no task in progress, whose task was cancelled under
/// it: whatever it still sends about that task goes nowhere, and it is told
/// why. Its result is refused by the ledger, as any result for a task that is
/// not working.
pub(super) fn refuse_cancelled(context: &Context, caller: &Caller) -> Result<(), Failure> {
    let last = context.ledger.borrow().last_task(caller.participant.id)?;
    match last {
        Some(thread) if thread.task.state == "cancelled" => Err(Failure::refuse(
            409,
            "task-cancelled",
            format!(
                "T-{} is cancelled: nothing more of it goes to @{}",
                thread.task.number, thread.task.requester
            ),
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
