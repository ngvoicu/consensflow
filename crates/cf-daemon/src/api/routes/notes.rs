//! `POST /api/notes`: a note, from the chief to the human or from a member to
//! whoever gave it its task (the task it held when the request arrived, where its window was
//! handed another task with another decider before the body came). It reads its body first, and wakes the dispatcher
//! once it is written.

use cf_ledger::NewNote;
use serde_json::{json, Value};

use super::questions::refuse_cancelled;
use super::{Answer, Caller, Context, Failure, Request};
use crate::api::views::{value, MessageSummary};

pub(super) async fn handle(
    context: &Context,
    caller: &Caller,
    mut request: Request,
) -> Result<Answer, Failure> {
    // The task in hand when the request arrived: a note whose body comes after its window
    // was handed another task is filed on this one where the two have different deciders.
    let arrived = context
        .ledger
        .borrow()
        .task_in_hand(caller.participant.id)
        .ok()
        .flatten();
    let body = request.json().await?;
    let active = context
        .ledger
        .borrow()
        .task_in_hand(caller.participant.id)?;
    let active = match (arrived, active) {
        (Some(arrived), Some(now))
            if arrived.task.id != now.task.id
                && deciders_differ(context, caller.project.id, &arrived, &now)? =>
        {
            Some(arrived)
        }
        (_, active) => active,
    };
    let chief = caller.participant.seat().coordinates();
    if active.is_none() && !chief {
        refuse_cancelled(context, caller)?;
    }
    // The chief's note goes to the human, whatever task it is on (its own
    // step's requester is itself); a member's to whoever gave its task.
    let to = if chief {
        "human"
    } else {
        active
            .as_ref()
            .map_or("chief", |thread| thread.task.requester.as_str())
    };
    if body.get("to").and_then(Value::as_str) == Some("human") && to != "human" {
        return Err(Failure::refuse(
            403,
            "not-the-chief",
            format!("only the chief notes the human; without --human, your note goes to @{to}"),
        ));
    }
    let noted = context.ledger.borrow_mut().note(
        caller.project.id,
        &NewNote {
            from: Some(caller.participant.handle.clone()),
            to: to.to_owned(),
            task: active.as_ref().map(|thread| thread.task.number),
            // Whatever is no text is no words, which the ledger refuses first.
            body: body
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        },
    )?;
    (context.kick)();
    Ok(Answer::created(
        json!({ "message": value(&MessageSummary::from(&noted))? }),
    ))
}

/// Whether two tasks have different deciders: with one chief every decider is the chief.
fn deciders_differ(
    context: &Context,
    project: i64,
    one: &cf_ledger::TaskThread,
    other: &cf_ledger::TaskThread,
) -> Result<bool, Failure> {
    let ledger = context.ledger.borrow();
    Ok(ledger.task_decider(project, one.task.number)?
        != ledger.task_decider(project, other.task.number)?)
}

#[cfg(test)]
mod tests;
