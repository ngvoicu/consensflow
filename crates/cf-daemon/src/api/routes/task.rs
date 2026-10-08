//! One task: `GET /api/tasks/<n>`, its `/transcript`, and the `POST`s that move
//! it (`done`, `accept`, `reopen`, `cancel`, `pause`, `resume`, `tell`). It
//! reads the task first, so an unknown one is 404 before anything else is
//! asked; and it takes the task as it was then, awaiting the body after: who
//! may do what is decided on the task as it was read, and the ledger decides on
//! it as it is. The number is the digits as the path had them, read as
//! `Number(...)` reads them.
//!
//! A read changes nothing: an answer in the thread that `cf` printed is
//! received when `cf` says it wrote it whole (`POST /api/answers/read`), not
//! when the thread was served. What the human has not passed on is not for an
//! agent to read: the thread leaves out a gated message, and the brief is
//! not given while it waits at the gate.

use cf_base::js;
use cf_ledger::{NewQuestion, TaskThread};
use hyper::Method;
use serde_json::{json, Map, Value};

use super::{Answer, Caller, Context, Failure, Request, TaskAction};
use crate::api::views::{value, MessageSummary, TaskSummary};

/// A task in one of these states has a window a `tell` reaches (a queued one
/// has none yet).
const WINDOWED: [&str; 3] = ["working", "waiting", "paused"];

/// The most items of a transcript one request may ask for.
const MOST_ITEMS: f64 = 50.0;

/// How many it gives when it is asked for none that is a number.
const DEFAULT_ITEMS: f64 = 10.0;

/// The largest whole number a double holds exactly (`Number.MAX_SAFE_INTEGER`):
/// no task has a number past it, and past it digits are not what JavaScript
/// writes for the number.
const MOST_EXACT: f64 = 9_007_199_254_740_991.0;

pub(super) async fn handle(
    context: &Context,
    caller: &Caller,
    mut request: Request,
    digits: &str,
    action: Option<TaskAction>,
) -> Result<Answer, Failure> {
    let asked = js::number(digits);
    let found = match whole(asked) {
        Some(number) => context.ledger.borrow().task(caller.project.id, number)?,
        None => None,
    };
    let Some(mut task) = found else {
        return Err(Failure::refuse(
            404,
            "unknown-task",
            format!("no task T-{} in this project", js::number_text(asked)),
        ));
    };
    let get = request.method == Method::GET;
    if action.is_none() && get {
        // The thread as far as the human has let it go: a gated message waits
        // unseen, and so does the brief while it is one of them.
        let held_back = brief_waits_at_the_gate(&task);
        task.messages.retain(|message| message.state != "gated");
        if held_back {
            task.task.body.clear();
        }
        return Ok(Answer::ok(json!({ "task": value(&task)? })));
    }
    if action == Some(TaskAction::Transcript) && get {
        return transcript(context, caller, &task, &request);
    }
    let Some(action) = action.filter(|_| request.method == Method::POST) else {
        return Err(Failure::refuse(
            404,
            "unknown-route",
            "no such task command",
        ));
    };
    let body = request.json().await?;
    let words = body.get("body").and_then(Value::as_str);
    match action {
        TaskAction::Done => done(context, caller, &task, words.unwrap_or_default()),
        _ if !may_move(caller, &task) => Err(Failure::refuse(
            403,
            "not-a-coordinator",
            format!(
                "only the chief or @{} may {} T-{}",
                task.task.requester,
                word(action),
                task.task.number
            ),
        )),
        TaskAction::Tell => tell(context, caller, &task, words),
        _ => moved(context, caller, &task, action, words.unwrap_or_default()),
    }
}

/// Whether the task's brief still waits for the human to pass it on: a task
/// message for the window that has the task is held at the gate, and none was
/// received by it. A window that received the brief has it, and a later task
/// message held at the gate (the words of a resume) does not take it back.
/// The brief is `TaskView.body`: the human and the board read it from the
/// ledger, and an agent does not until it has passed.
fn brief_waits_at_the_gate(task: &TaskThread) -> bool {
    let Some(window) = task.task.assignee.as_deref() else {
        return false;
    };
    let mut held = false;
    for message in task
        .messages
        .iter()
        .filter(|message| message.kind == "task" && message.recipient == window)
    {
        match message.state.as_str() {
            "delivered" | "read" => return false,
            "gated" => held = true,
            _ => {}
        }
    }
    held
}

/// What the task's window did so far, from ConsensFlow's own copy: the last
/// items, for whoever gave the task.
fn transcript(
    context: &Context,
    caller: &Caller,
    task: &TaskThread,
    request: &Request,
) -> Result<Answer, Failure> {
    if !may_move(caller, task) {
        return Err(Failure::refuse(
            403,
            "not-a-coordinator",
            format!(
                "only the chief or @{} may read what T-{}'s window did",
                task.task.requester, task.task.number
            ),
        ));
    }
    let last = items_of(request.param("last"));
    let transcript =
        context
            .ledger
            .borrow()
            .transcript(caller.project.id, task.task.number, Some(last))?;
    Ok(Answer::ok(value(&transcript)?))
}

/// `Math.min(50, Math.max(1, Number(last) || 10))`: how many items were asked
/// for. None, or what is no number or is 0, is ten. A fraction is read as
/// `rows.slice(rows.length - last)` read it: the ledger counts from the
/// whole item before it, so it gives the next whole number of items.
fn items_of(asked: Option<&str>) -> usize {
    let number = asked.map_or(0.0, js::number);
    let number = if number.is_nan() || number == 0.0 {
        DEFAULT_ITEMS
    } else {
        number
    };
    // Between 1 and 50, and whole: the cast loses nothing.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    return number.clamp(1.0, MOST_ITEMS).ceil() as usize;
}

/// `POST /api/tasks/<n>/done`: the assignee's answer finishes the task. Only
/// its assignee may finish it.
fn done(
    context: &Context,
    caller: &Caller,
    task: &TaskThread,
    words: &str,
) -> Result<Answer, Failure> {
    if task.task.assignee.as_deref() != Some(caller.participant.handle.as_str()) {
        return Err(Failure::refuse(
            403,
            "not-yours",
            format!(
                "T-{} is assigned to @{}",
                task.task.number,
                task.task.assignee.as_deref().unwrap_or("null")
            ),
        ));
    }
    let done =
        context
            .ledger
            .borrow_mut()
            .record_result(caller.project.id, task.task.number, words)?;
    (context.kick)();
    Ok(Answer::ok(
        json!({ "task": value(&TaskSummary::from(&done.task))? }),
    ))
}

/// `POST /api/tasks/<n>/tell`: stop the task and put this to its window. The
/// agent is interrupted as for any pause, reads the question once idle, and
/// its answer comes back as a message; the chief resumes the task with its
/// words. An urgent question pauses its task itself, in the same step.
fn tell(
    context: &Context,
    caller: &Caller,
    task: &TaskThread,
    words: Option<&str>,
) -> Result<Answer, Failure> {
    let number = task.task.number;
    let window = match &task.task.assignee {
        Some(assignee) if WINDOWED.contains(&task.task.state.as_str()) => assignee,
        _ => {
            return Err(Failure::refuse(
                409,
                "no-window",
                format!(
                    "T-{number} has no window to tell: it is {}",
                    task.task.state
                ),
            ))
        }
    };
    let told = context.ledger.borrow_mut().ask(
        caller.project.id,
        &NewQuestion {
            from: Some(caller.participant.handle.clone()),
            to: window.clone(),
            // Whatever is no text is no words: the ledger refuses it as it
            // refuses a blank one, once the task is paused for it.
            body: words.map(str::to_owned),
            task: Some(number),
            questions: None,
            urgent: true,
        },
    )?;
    (context.kick)();
    // The task as it is now, paused.
    let now = context
        .ledger
        .borrow()
        .task(caller.project.id, number)?
        .ok_or_else(|| {
            Failure::Internal("Cannot read properties of null (reading 'number')".to_owned())
        })?;
    let mut answer = Map::new();
    answer.insert("message".to_owned(), value(&MessageSummary::from(&told))?);
    answer.insert("task".to_owned(), value(&TaskSummary::from(&now.task))?);
    Ok(Answer::ok(Value::Object(answer)))
}

/// The moves of a task a coordinator makes with the ledger's own words: accept
/// its result, cancel it, pause it, resume it, give it back.
fn moved(
    context: &Context,
    caller: &Caller,
    task: &TaskThread,
    action: TaskAction,
    words: &str,
) -> Result<Answer, Failure> {
    let (project, number) = (caller.project.id, task.task.number);
    let by = caller.participant.handle.as_str();
    let mut ledger = context.ledger.borrow_mut();
    let moved = match action {
        TaskAction::Accept => ledger.accept_task(project, number, by)?,
        TaskAction::Cancel => ledger.cancel_task(project, number, by)?,
        TaskAction::Pause => ledger.pause_task(project, number, Some(by), None)?,
        TaskAction::Resume => ledger.resume_task(project, number, Some(by), words)?.task,
        // `reopen`; and what Node's last branch took any other word for, which
        // only `transcript` is, and only when it is posted.
        _ => ledger.reopen_task(project, number, by, words)?.task,
    };
    drop(ledger);
    (context.kick)();
    Ok(Answer::ok(
        json!({ "task": value(&TaskSummary::from(&moved))? }),
    ))
}

/// Whether the window may do what only the chief or the one who gave the task
/// may: read what it did, and move it.
fn may_move(caller: &Caller, task: &TaskThread) -> bool {
    caller.participant.role == "chief" || task.task.requester == caller.participant.handle
}

/// A task route's last word, as the path spells it.
fn word(action: TaskAction) -> &'static str {
    match action {
        TaskAction::Done => "done",
        TaskAction::Accept => "accept",
        TaskAction::Reopen => "reopen",
        TaskAction::Cancel => "cancel",
        TaskAction::Pause => "pause",
        TaskAction::Resume => "resume",
        TaskAction::Tell => "tell",
        TaskAction::Transcript => "transcript",
    }
}

/// `number` as a task number the ledger can look for: a whole number a double
/// holds exactly. No task has any other number: a fraction, a number too big,
/// NaN and infinity name none.
pub(super) fn whole(number: f64) -> Option<i64> {
    if number.is_finite() && number.fract() == 0.0 && number.abs() <= MOST_EXACT {
        // A whole number the double holds exactly: the cast loses nothing.
        #[allow(clippy::cast_possible_truncation)]
        return Some(number as i64);
    }
    None
}

#[cfg(test)]
mod tests;
