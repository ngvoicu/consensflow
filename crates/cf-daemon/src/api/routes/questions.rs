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
    let active = caller.question_task.as_ref();
    if let Some(asked_on) = active {
        let now = context
            .ledger
            .borrow()
            .task_in_hand(caller.participant.id)?;
        let generation = |thread: &cf_ledger::TaskThread| {
            thread
                .messages
                .iter()
                .filter(|message| {
                    message.kind == "task" && message.recipient_id == caller.participant.id
                })
                .map(|message| message.id)
                .max()
        };
        if !now.as_ref().is_some_and(|thread| {
            thread.task.id == asked_on.task.id && generation(thread) == generation(asked_on)
        }) {
            let current = context
                .ledger
                .borrow()
                .task(caller.project.id, asked_on.task.number)?;
            let task = current
                .map(|thread| thread.task)
                .unwrap_or_else(|| asked_on.task.clone());
            let (code, words) = if task.state == "cancelled" {
                (
                    "task-cancelled",
                    format!(
                        "T-{} is cancelled: nothing more of it goes to @{}",
                        task.number, task.requester
                    ),
                )
            } else if task.assignee.as_deref() != Some(caller.participant.handle.as_str()) {
                (
                    "question-moved",
                    format!(
                        "T-{} moved to another window: this window cannot ask on it now",
                        task.number
                    ),
                )
            } else {
                (
                    "task-ended",
                    format!(
                        "T-{} is {}: this window cannot ask on it now",
                        task.number, task.state
                    ),
                )
            };
            return Err(Failure::refuse(409, code, words));
        }
        if caller.question_unstarted {
            return Err(Failure::refuse(
                409,
                "task-not-started",
                format!(
                    "T-{} has not reached this window: this turn cannot ask on it",
                    asked_on.task.number
                ),
            ));
        }
    } else {
        refuse_cancelled(context, caller)?;
    }
    let asked = context.ledger.borrow_mut().ask(
        caller.project.id,
        &NewQuestion {
            from: Some(caller.participant.handle.clone()),
            to: "chief".to_owned(),
            // Whatever is no text is no words: the ledger refuses it as it
            // refuses a blank one, once the question is being put.
            body: body.get("body").and_then(Value::as_str).map(str::to_owned),
            task: active.map(|thread| thread.task.number),
            // Present is not absent: `null` is a list of questions that is none.
            questions: body.get("questions").cloned(),
            urgent: false,
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
