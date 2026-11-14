//! `GET /api/tasks`, the open tasks and each lane's, and `POST /api/tasks`, a
//! task given: the chief's alone, refused before its body is read. One task is
//! [`super::task`]: a card says of a brief that waits, at the human's gate or for
//! the Resume, what the task's own view says, its title in a caller's list as in
//! its view ([`brief_waits`]).

use cf_base::js;
use cf_ledger::{NewTask, PlanTask};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use cf_ledger::TaskCard;

use super::task::{brief_waits, may_move, whole};
use super::{Answer, Caller, Context, Failure, Request};
use crate::api::views::{value, TaskSummary};
use crate::api::waits::Waits;

/// The states of a task whose brief may still wait for its window: given to it
/// and not over. One open has no window; one over had its brief, or took it with it.
const GIVEN: [&str; 4] = ["queued", "working", "waiting", "paused"];

/// `GET /api/tasks`: the tasks no lane has (`open`: those waiting for a
/// member, those paused, called off or failed before any member had them, and
/// those of a member who left the staff), and each participant's lane with the
/// tasks on it, and, for one that runs on a harness, its model and effort as the page
/// shows them (`runsOn`: `Opus · high`, spec harness-models).
pub(super) async fn list(
    context: &Context,
    caller: &Caller,
    _request: Request,
) -> Result<Answer, Failure> {
    let board = context.ledger.borrow().board(caller.project.id)?;
    let waits = Waits::now(context, caller)?;
    // Nothing waits where nothing is held at the gate and the project is open.
    let held = !board.gated.is_empty() || waits.closed();
    let summary = |card: &TaskCard| -> Result<Value, Failure> {
        let mut summary = TaskSummary::from(card);
        if held && GIVEN.contains(&card.state.as_str()) && !may_move(caller, &card.requester) {
            let task = context
                .ledger
                .borrow()
                .task(caller.project.id, card.number)?;
            if let Some(waiting) = task.as_ref().and_then(|task| brief_waits(task, &waits)) {
                waiting.clone_into(&mut summary.title);
            }
        }
        value(&summary)
    };
    let open = board
        .open
        .iter()
        .map(summary)
        .collect::<Result<Vec<_>, _>>()?;
    let mut lanes = Vec::new();
    for lane in &board.lanes {
        let tasks = lane
            .tasks
            .iter()
            .map(summary)
            .collect::<Result<Vec<_>, _>>()?;
        let mut entry = json!({
            "handle": lane.participant.handle,
            "role": lane.participant.role,
            "tasks": tasks,
        });
        let participant = &lane.participant;
        // A name that holds the model and effort says them already (HMD-12); the chief's
        // name is its role, and never does.
        if participant.harness.is_some() && (participant.role == "chief" || participant.name_apart)
        {
            entry["runsOn"] = json!(cf_ledger::handles::runs_on(participant));
        }
        lanes.push(entry);
    }
    Ok(Answer::ok(json!({ "open": open, "lanes": lanes })))
}

/// `POST /api/tasks`. The board is the only channel between agents: no task
/// is given by name. A follow-up that needs the context of the window that did
/// T-n goes back to that window (`after`); the chief's own later step is its
/// own (`self`); everything else is fresh work for a tier of worker, advice
/// from a tier of advisor, a review from a tier of reviewer, a test from a tier
/// of tester, or an image from the designer.
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
    if body.contains_key("plan") {
        return create_plan(context, caller, &body);
    }
    if body.contains_key("action") {
        return decide(context, caller, &body);
    }
    // `Number(body.after)`, read before anything is asked of the ledger.
    let after = match body.get("after") {
        Some(after) => Some(js::to_number(Some(after)).map_err(|_| {
            Failure::Internal("Cannot convert object to primitive value".to_owned())
        })?),
        None => None,
    };
    let given = NewTask::from_json(&given(caller, &body, after)?)?;
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
        // And whose tier it went to, so the chief is told which member holds it.
        let (pool, tier) = (created.task.pool.as_deref(), created.task.tier.as_deref());
        let held_by: Vec<String> = match pool {
            Some(pool) => context
                .ledger
                .borrow()
                .members(caller.project.id, pool)?
                .into_iter()
                .filter(|member| member.tier.as_deref() == tier)
                .map(|member| member.handle)
                .collect(),
            None => Vec::new(),
        };
        answer.insert("heldBy".to_owned(), json!(held_by));
    }
    answer.insert("gated".to_owned(), json!(gated));
    Ok(Answer::created(Value::Object(answer)))
}

/// Put a section's name in every refusal without changing its code or status.
fn in_section(name: &str, error: Failure) -> Failure {
    match error {
        Failure::Refused(mut refusal) => {
            refusal.message = format!("section {name}: {}", refusal.message);
            Failure::Refused(refusal)
        }
        other => other,
    }
}

/// Parse and validate each section as one ordinary task. Named references are
/// held for the ledger, which resolves them inside the transaction.
fn create_plan(
    context: &Context,
    caller: &Caller,
    body: &Map<String, Value>,
) -> Result<Answer, Failure> {
    let sections = body
        .get("plan")
        .and_then(Value::as_array)
        .ok_or_else(|| Failure::refuse(400, "invalid-plan", "plan must be a list of sections"))?;
    let key = match body.get("key") {
        None => None,
        Some(Value::String(key)) => Some(key.as_str()),
        Some(_) => return Err(Failure::refuse(400, "invalid-key", "key must be one word")),
    };
    let mut plan = Vec::new();
    for section in sections {
        let part = section.as_object().ok_or_else(|| {
            Failure::refuse(400, "invalid-plan", "each section must be an object")
        })?;
        let name = part.get("name").and_then(Value::as_str).unwrap_or("");
        let parse = || -> Result<PlanTask, Failure> {
            let mut part = part.clone();
            part.remove("name");
            let mut named = [Vec::new(), Vec::new()];
            for (index, field) in ["needs", "before"].into_iter().enumerate() {
                if let Some(value) = part.get_mut(field) {
                    let array = value.as_array_mut().ok_or_else(|| {
                        Failure::refuse(400, "invalid-needs", format!("{field} takes a list"))
                    })?;
                    let mut numbers = Vec::new();
                    for item in array.iter() {
                        match item {
                            Value::String(name) if !name.is_empty() => {
                                named[index].push(name.clone())
                            }
                            Value::Number(_) => numbers.push(item.clone()),
                            _ => {
                                return Err(Failure::refuse(
                                    400,
                                    "invalid-needs",
                                    format!("{field} takes task numbers or section names"),
                                ))
                            }
                        }
                    }
                    *array = numbers;
                }
            }
            let after = match part.get("after") {
                Some(value) => Some(js::to_number(Some(value)).map_err(|_| {
                    Failure::refuse(400, "invalid-after", "after takes a task number")
                })?),
                None => None,
            };
            if let Some(number) = after.filter(|number| whole(*number).is_none()) {
                return Err(Failure::refuse(
                    404,
                    "unknown-task",
                    format!("no task T-{} in this project", js::number_text(number)),
                ));
            }
            let task = NewTask::from_json(&given(caller, &part, after)?)?;
            if part.get("self") == Some(&Value::Bool(true))
                && task.needs.is_empty()
                && task.before.is_empty()
                && named.iter().all(Vec::is_empty)
            {
                return Err(Failure::refuse(
                    400,
                    "own-work",
                    "you are already at it: do it now, or give it --needs",
                ));
            }
            Ok(PlanTask {
                name: name.to_owned(),
                task,
                needs: std::mem::take(&mut named[0]),
                before: std::mem::take(&mut named[1]),
            })
        };
        plan.push(parse().map_err(|error| in_section(name, error))?);
    }
    // Hash both what the server will create and the optional fingerprint of the
    // source file, so a key cannot silently substitute different task fields.
    let encoded = serde_json::to_vec(&json!({ "plan": sections, "source": body.get("source") }))
        .map_err(|error| Failure::Internal(error.to_string()))?;
    let fingerprint = format!("{:x}", Sha256::digest(encoded));
    let created =
        context
            .ledger
            .borrow_mut()
            .create_plan(caller.project.id, &plan, key, &fingerprint)?;
    if !created.repeated {
        (context.kick)();
    }
    let gated = context
        .ledger
        .borrow()
        .project(caller.project.id)?
        .is_some_and(|project| project.gate);
    let tasks: Vec<Value> = created
        .tasks
        .iter()
        .map(|(name, number)| json!({ "name": name, "task": { "number": number } }))
        .collect();
    let answer = json!({ "tasks": tasks, "gated": gated });
    Ok(if created.repeated {
        Answer::ok(answer)
    } else {
        Answer::created(answer)
    })
}

/// Several decisions are one ledger write. Read each task and its warnings in
/// the same synchronous borrow, then wake the dispatcher only after commit.
fn decide(
    context: &Context,
    caller: &Caller,
    body: &Map<String, Value>,
) -> Result<Answer, Failure> {
    let action = body
        .get("action")
        .and_then(Value::as_str)
        .filter(|action| ["accept", "cancel"].contains(action))
        .ok_or_else(|| Failure::refuse(400, "invalid-action", "action must be accept or cancel"))?;
    let values = body
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| Failure::refuse(400, "no-tasks", "tasks must be a list of task numbers"))?;
    let numbers: Vec<i64> = values
        .iter()
        .map(|number| {
            number.as_i64().filter(|number| *number > 0).ok_or_else(|| {
                let named = number
                    .as_i64()
                    .map_or_else(|| number.to_string(), |n| format!("T-{n}"));
                Failure::refuse(
                    400,
                    "invalid-task",
                    format!("{named} is not a positive task number"),
                )
            })
        })
        .collect::<Result<_, _>>()?;
    let by = caller.participant.handle.as_str();
    let words = body.get("body").and_then(Value::as_str).unwrap_or_default();
    let mut ledger = context.ledger.borrow_mut();
    let mut warnings = Vec::new();
    for &number in &numbers {
        let thread = ledger.task(caller.project.id, number)?.ok_or_else(|| {
            Failure::refuse(
                404,
                "unknown-task",
                format!("no task T-{number} in this project"),
            )
        })?;
        if !may_move(caller, &thread.task.requester) {
            return Err(Failure::refuse(
                403,
                "not-a-coordinator",
                format!(
                    "only the chief or @{} may {action} T-{number}",
                    thread.task.requester
                ),
            ));
        }
        let running = if action == "accept" {
            Some(ledger.left_running(caller.project.id, number)?)
        } else {
            None
        };
        let dropped = if action == "cancel" {
            ledger
                .told_unreceived(caller.project.id, number)?
                .into_iter()
                .filter(|message| message.sender.as_deref() == Some(by))
                .map(|message| message.id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        warnings.push((running, dropped));
    }
    let moved = ledger.decide_tasks(caller.project.id, &numbers, by, action, words)?;
    drop(ledger);
    (context.kick)();
    let mut tasks = Vec::new();
    for (task, (running, dropped)) in moved.iter().zip(warnings) {
        let mut result = Map::new();
        result.insert("task".into(), value(&TaskSummary::from(task))?);
        if let Some((jobs, agents)) = running.filter(|count| *count != (0, 0)) {
            result.insert("running".into(), json!({"jobs":jobs,"agents":agents}));
        }
        if !dropped.is_empty() {
            result.insert("dropped".into(), json!(dropped));
        }
        tasks.push(Value::Object(result));
    }
    Ok(Answer::ok(json!({ "tasks": tasks })))
}

/// What Node handed `createTask`, as the ledger reads a task from JSON: who
/// gives it; what it is for (the window that did `after`, the chief itself, or
/// a pool, a tier and a purpose, each as it was asked, and only if it was);
/// and its brief and its order. A follow-up of a task that is no whole number
/// is given as task 0, which is refused in its place. What the request asks of the
/// staff is the ledger's to check first (`cf_ledger::asked_role`): one role at most,
/// and nothing beside it that the task it is would drop; a role asked for a follow-up
/// goes with it, for the ledger to hold it to the window's.
fn given(caller: &Caller, body: &Map<String, Value>, after: Option<f64>) -> Result<Value, Failure> {
    let handle = &caller.participant.handle;
    let flag = |name: &str| body.get(name) == Some(&Value::Bool(true));
    let asked = cf_ledger::asked_role(body)?;
    let mut task = Map::new();
    task.insert("from".to_owned(), json!(handle));
    match after {
        Some(number) => {
            task.insert("after".to_owned(), json!(whole(number).unwrap_or(0)));
            if let Some(role) = asked {
                task.insert("pool".to_owned(), json!(role.id));
            }
        }
        None if flag("self") => {
            task.insert("to".to_owned(), json!(handle));
        }
        None => {
            // The role whose field is true (`frontend`, `advice`, `design`, …); a worker where
            // none is.
            let pool = asked.unwrap_or_else(cf_ledger::roles::unasked).id;
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
    Ok(Value::Object(task))
}

#[cfg(test)]
mod tests;
