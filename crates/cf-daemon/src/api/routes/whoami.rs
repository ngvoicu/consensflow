//! `GET /api/whoami`: who the window is: its project, its participant, and the
//! task it has in progress.

use serde_json::{json, Value};

use super::{Answer, Caller, Context, Failure, Request};
use crate::api::views::{value, TaskSummary};

pub(super) async fn handle(
    context: &Context,
    caller: &Caller,
    _request: Request,
) -> Result<Answer, Failure> {
    let task = context
        .ledger
        .borrow()
        .active_task(caller.participant.id, false)?;
    Ok(Answer::ok(json!({
        "project": {
            "id": caller.project.id,
            "name": caller.project.name,
            "directory": caller.project.directory,
        },
        "participant": {
            "handle": caller.participant.handle,
            "role": caller.participant.role,
        },
        "task": match &task {
            Some(thread) => value(&TaskSummary::from(&thread.task))?,
            None => Value::Null,
        },
    })))
}

#[cfg(test)]
mod tests;
