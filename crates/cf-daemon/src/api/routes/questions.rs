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
    let holding = caller.question_holding;
    if let Some((task, number)) = caller.question_launch_task {
        let ledger = context.ledger.borrow();
        let belongs = match holding {
            Some(h) if h.task_id == task => ledger.question_holding_matches(
                caller.participant.id,
                Some(task),
                Some(h.first_brief),
            )?,
            _ => false,
        };
        if !belongs {
            if let Some(current) = ledger.task(caller.project.id, number)? {
                return Err(ended_task(&current.task, caller));
            }
            return Err(Failure::refuse(
                409,
                "task-ended",
                format!("T-{number} is no longer held by this window"),
            ));
        }
    }
    if let Some(holding) = holding {
        let ledger = context.ledger.borrow();
        if !ledger.question_holding_matches(
            caller.participant.id,
            Some(holding.task_id),
            Some(holding.first_brief),
        )? {
            if let Some(current) = ledger.task(caller.project.id, holding.task_number)? {
                return Err(ended_task(&current.task, caller));
            }
            return Err(Failure::refuse(
                409,
                "task-ended",
                format!("T-{} is no longer held by this window", holding.task_number),
            ));
        }
    }
    let asked = context.ledger.borrow_mut().ask(
        caller.project.id,
        &NewQuestion {
            from: Some(caller.participant.handle.clone()),
            to: "chief".to_owned(),
            // Whatever is no text is no words: the ledger refuses it as it
            // refuses a blank one, once the question is being put.
            body: body.get("body").and_then(Value::as_str).map(str::to_owned),
            task: holding.map(|holding| holding.task_number),
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

fn ended_task(task: &cf_ledger::TaskView, caller: &Caller) -> Failure {
    let code = if task.state == "cancelled" {
        "task-cancelled"
    } else if task.assignee.as_deref() != Some(caller.participant.handle.as_str()) {
        "question-moved"
    } else {
        "task-ended"
    };
    let place = if task.deleted_at.is_some() {
        "was deleted".to_owned()
    } else if matches!(task.state.as_str(), "accepted" | "cancelled") {
        format!("is {}", task.state)
    } else if let Some(member) = task.assignee.as_ref() {
        format!("is {} with @{member}", task.state)
    } else if let Some(pool) = &task.pool {
        format!("is {} for the {pool} pool", task.state)
    } else {
        format!("is {} with no member", task.state)
    };
    Failure::refuse(
        409,
        code,
        format!(
            "T-{} {place}: this window cannot ask on it now",
            task.number
        ),
    )
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
