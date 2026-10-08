//! `GET /api/tasks`, the open tasks and each lane's, and `POST /api/tasks`, a
//! task given: the chief's alone, refused before its body is read. One task is
//! [`super::task`].

use cf_base::js;
use cf_ledger::NewTask;
use serde_json::{json, Map, Value};

use super::task::whole;
use super::{Answer, Caller, Context, Failure, Request};
use crate::api::views::{value, TaskSummary};

/// `GET /api/tasks`: the tasks no lane has (`open`: those waiting for a
/// member, those paused, called off or failed before any member had them, and
/// those of a member who left the staff), and each participant's lane with the
/// tasks on it.
pub(super) async fn list(
    context: &Context,
    caller: &Caller,
    _request: Request,
) -> Result<Answer, Failure> {
    let board = context.ledger.borrow().board(caller.project.id)?;
    let open = board
        .open
        .iter()
        .map(|card| value(&TaskSummary::from(card)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut lanes = Vec::new();
    for lane in &board.lanes {
        let tasks = lane
            .tasks
            .iter()
            .map(|card| value(&TaskSummary::from(card)))
            .collect::<Result<Vec<_>, _>>()?;
        lanes.push(json!({
            "handle": lane.participant.handle,
            "role": lane.participant.role,
            "tasks": tasks,
        }));
    }
    Ok(Answer::ok(json!({ "open": open, "lanes": lanes })))
}

/// `POST /api/tasks`. The board is the only channel between agents: no task
/// is given by name. A follow-up that needs the context of the window that did
/// T-n goes back to that window (`after`); the chief's own later step is its
/// own (`self`); everything else is fresh work for a tier of worker, advice
/// from a tier of advisor, a review from a tier of reviewer, or an image from
/// the designer.
pub(super) async fn create(
    context: &Context,
    caller: &Caller,
    mut request: Request,
) -> Result<Answer, Failure> {
    if caller.participant.role != "chief" {
        return Err(Failure::refuse(
            403,
            "not-a-coordinator",
            "members do not hand out tasks: ask your chief instead (cf ask)",
        ));
    }
    let body = request.json().await?;
    // `Number(body.after)`, read before anything is asked of the ledger.
    let after = match body.get("after") {
        Some(after) => Some(js::to_number(Some(after)).map_err(|_| {
            Failure::Internal("Cannot convert object to primitive value".to_owned())
        })?),
        None => None,
    };
    let given = NewTask::from_json(&given(caller, &body, after))?;
    // A follow-up on a task that is no whole number names none: the ledger
    // would say so once it had checked what it checks first, as `from_json` has.
    if let Some(number) = after.filter(|number| whole(*number).is_none()) {
        return Err(Failure::refuse(
            404,
            "unknown-task",
            format!(
                "no task T-{} in project {}",
                js::number_text(number),
                caller.project.id
            ),
        ));
    }
    let created = context
        .ledger
        .borrow_mut()
        .create_task(caller.project.id, &given)?;
    (context.kick)();
    // With human approval required, the brief waits for the human before it
    // moves; `cf` says so, and the chief knows a quiet board is a waiting one.
    // The gate is read again, as it is now.
    let gated = context
        .ledger
        .borrow()
        .project(caller.project.id)?
        .map(|project| project.gate)
        .ok_or_else(|| {
            Failure::Internal("Cannot read properties of null (reading 'gate')".to_owned())
        })?;
    let mut answer = Map::new();
    answer.insert("task".to_owned(), value(&TaskSummary::from(&created.task))?);
    answer.insert(
        "message".to_owned(),
        created
            .message
            .as_ref()
            .map_or(Value::Null, |message| json!(message.id)),
    );
    // The tier asked, when nobody on the staff holds it and the task went to
    // the nearest.
    if let Some(asked) = &created.asked {
        answer.insert("asked".to_owned(), json!(asked));
    }
    answer.insert("gated".to_owned(), json!(gated));
    Ok(Answer::created(Value::Object(answer)))
}

/// What Node handed `createTask`, as the ledger reads a task from JSON: who
/// gives it; what it is for (the window that did `after`, the chief itself, or
/// a pool, a tier and a purpose, each as it was asked, and only if it was);
/// and its brief and its order. A follow-up of a task that is no whole number
/// is given as task 0, which is refused in its place.
fn given(caller: &Caller, body: &Map<String, Value>, after: Option<f64>) -> Value {
    let handle = &caller.participant.handle;
    let flag = |name: &str| body.get(name) == Some(&Value::Bool(true));
    let mut task = Map::new();
    task.insert("from".to_owned(), json!(handle));
    match after {
        Some(number) => {
            task.insert("after".to_owned(), json!(whole(number).unwrap_or(0)));
        }
        None if flag("self") => {
            task.insert("to".to_owned(), json!(handle));
        }
        None => {
            let pool = if flag("design") {
                "designer"
            } else if flag("advice") {
                "advisor"
            } else if flag("review") {
                "reviewer"
            } else {
                "worker"
            };
            task.insert("pool".to_owned(), json!(pool));
            for name in ["tier", "purpose"] {
                if let Some(asked) = body.get(name) {
                    task.insert(name.to_owned(), asked.clone());
                }
            }
        }
    }
    for name in ["body", "needs", "before"] {
        if let Some(asked) = body.get(name) {
            task.insert(name.to_owned(), asked.clone());
        }
    }
    Value::Object(task)
}

#[cfg(test)]
mod tests;
