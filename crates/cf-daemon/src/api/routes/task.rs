//! One task: `GET /api/tasks/<n>`, its `/transcript`, and the `POST`s that move
//! it (`done`, `accept`, `reopen`, `edit`, `cancel`, `pause`, `resume`, `finish`, `tell`). It
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
//! not given to a caller who may not move the task while it waits at the gate,
//! nor its first line, the task's title, which says it waits instead.
//! The chief, and whoever else gave the task, wrote the brief: they read it.
//! Nor is what waits for the Resume: while the project is closed, the thread
//! leaves out a message no window was handed yet, and the brief while it is one
//! but for those who wrote it ([`Waits`]). The transcript is read a page at a
//! time and searched as well (`pages`).

mod pages;

use cf_base::js;
use cf_ledger::{NewQuestion, TaskThread, TaskView};
use hyper::Method;
use serde_json::{json, Map, Value};

use super::{Answer, Caller, Context, Failure, Request, TaskAction};
use crate::api::views::{value, MessageSummary, TaskSummary};
use crate::api::waits::{Waits, WAITS};
use pages::Asked;

/// A task in one of these states has a window a `tell` reaches and stops.
const WINDOWED: [&str; 3] = ["working", "waiting", "paused"];

/// A task in this state has no window yet: a `tell` keeps its words for the one
/// coming up.
const QUEUED: &str = "queued";

/// A task in this state is on the board for a window nobody has yet: a `tell` adds
/// its words to the brief the window that takes it is given.
const OPEN: &str = "open";

/// What a task's title says in place of its brief's first line while the brief
/// waits at the human's gate, for a caller who did not write it.
pub const GATED: &str = "(this waits for the human to pass it on)";

/// The most items of a transcript one request may ask for: a page's.
#[allow(clippy::cast_precision_loss)]
const MOST_ITEMS: f64 = pages::PER_PAGE as f64;

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
        // unseen, and so does the brief while it is one of them, but for those
        // who wrote it. While the project is closed, so does a message no window
        // was handed yet, and the brief while it is one, but for those who wrote
        // it: `cf` would take an answer it printed for received, so one whose text
        // waits is not in the thread at all.
        let waits = Waits::now(context, caller)?;
        let held_back = brief_waits(&task, &waits);
        task.messages
            .retain(|message| message.state != "gated" && !waits.holds(message));
        if let Some(held) = held_back.filter(|_| !may_move(caller, &task.task.requester)) {
            task.task.body.clear();
            task.task.title = held.to_owned();
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
        _ if !may_move(caller, &task.task.requester) => Err(Failure::refuse(
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
        TaskAction::Finish => finish(context, caller, &task).await,
        _ => moved(context, caller, &task, action, words.unwrap_or_default()),
    }
}

/// Whether the task's brief still waits, and what its title says in place of the
/// brief's first line while it does: for the human to pass it on (a task message
/// for the window that has the task is held at the gate, [`GATED`]), or for the
/// Resume (one no window was handed yet, read while the project is closed,
/// [`Waits`] and [`WAITS`]), and none was received by the window. A window that
/// received the brief has it, and a later task message that waits (the words of a
/// resume) does not take it back. The brief is `TaskView.body`, and its first line
/// the title (Calliope's review of the merge, T-408: a window of a closed project
/// read a line of a brief it was never handed in the title): the human and the
/// board read them from the ledger, and so do the chief and whoever gave the task,
/// who wrote it; an agent who did not does not until it has passed.
pub(super) fn brief_waits(task: &TaskThread, waits: &Waits) -> Option<&'static str> {
    let window = task.task.assignee.as_deref()?;
    let mut held = None;
    for message in task
        .messages
        .iter()
        .filter(|message| message.kind == "task" && message.recipient == window)
    {
        match message.state.as_str() {
            "delivered" | "read" => return None,
            "gated" => held = Some(GATED),
            _ if waits.holds(message) => held = held.or(Some(WAITS)),
            _ => {}
        }
    }
    held
}

/// What the task's window did so far, from ConsensFlow's own copy, for whoever
/// gave the task: the last items, or a page of them, searched or not (`pages`).
fn transcript(
    context: &Context,
    caller: &Caller,
    task: &TaskThread,
    request: &Request,
) -> Result<Answer, Failure> {
    if !may_move(caller, &task.task.requester) {
        return Err(Failure::refuse(
            403,
            "not-a-coordinator",
            format!(
                "only the chief or @{} may read what T-{}'s window did",
                task.task.requester, task.task.number
            ),
        ));
    }
    let (project, number) = (caller.project.id, task.task.number);
    if let Some(asked) = Asked::of(request.param("page"), request.param("find")) {
        let whole = context.ledger.borrow().transcript(project, number, None)?;
        return Ok(Answer::ok(asked.answer(whole)?));
    }
    let last = items_of(request.param("last"));
    let transcript = context
        .ledger
        .borrow()
        .transcript(project, number, Some(last))?;
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
/// its assignee may finish it. The chief's own step is accepted with it, and
/// the answer names what was still on its way and is withdrawn (`withdrawn`:
/// its brief, and any answer or note carried on it), and its brief if it was
/// being typed in and may still land once (`landing`), each as a message.
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
    let mut answer = Map::new();
    answer.insert("task".to_owned(), value(&TaskSummary::from(&done.task))?);
    for (key, messages) in [("withdrawn", &done.withdrawn), ("landing", &done.landing)] {
        if !messages.is_empty() {
            let named: Result<Vec<_>, _> = messages
                .iter()
                .map(|message| value(&MessageSummary::from(message)))
                .collect();
            answer.insert(key.to_owned(), Value::Array(named?));
        }
    }
    Ok(Answer::ok(Value::Object(answer)))
}

/// `POST /api/tasks/<n>/tell`: stop the task and put this to its window. The
/// agent is interrupted as for any pause, reads the question once idle, and
/// its answer comes back as a message; the chief resumes the task with its
/// words. An urgent question pauses its task itself, in the same step. A task
/// still queued, its window not up yet, is not stopped: the words are kept for
/// the window, and go in after its brief (`Ledger::tell_queued`). A task still
/// open has no window to keep them for, nor a message to carry them: they are
/// added to its brief (`Ledger::tell_open`), and the answer holds the task
/// alone. Which of the three it is, is read again once the words are in: the
/// task may have gone on while they came (its brief arrived, its resume queued
/// it again, the daemon gave it a window). A task that went where no tell
/// reaches is told as it was read, and the ledger refuses it in its own words.
fn tell(
    context: &Context,
    caller: &Caller,
    task: &TaskThread,
    words: Option<&str>,
) -> Result<Answer, Failure> {
    let number = task.task.number;
    let fresh = context.ledger.borrow().task(caller.project.id, number)?;
    let telling = fresh
        .as_ref()
        .map(|thread| &thread.task)
        .filter(|now| tellable(now))
        .unwrap_or(&task.task);
    let state = telling.state.as_str();
    if state == OPEN {
        let told = context.ledger.borrow_mut().tell_open(
            caller.project.id,
            number,
            &caller.participant.handle,
            words.unwrap_or_default(),
        )?;
        let mut answer = Map::new();
        answer.insert("task".to_owned(), value(&TaskSummary::from(&told))?);
        return Ok(Answer::ok(Value::Object(answer)));
    }
    let window = match &telling.assignee {
        Some(assignee) if reaches(telling) => assignee,
        _ => {
            return Err(Failure::refuse(
                409,
                "no-window",
                format!("T-{number} has no window to tell: it is {state}"),
            ))
        }
    };
    let told = if state == QUEUED {
        // Whatever is no text is no words: the ledger refuses it as it refuses
        // a blank one.
        context.ledger.borrow_mut().tell_queued(
            caller.project.id,
            number,
            &caller.participant.handle,
            words.unwrap_or_default(),
        )?
    } else {
        context.ledger.borrow_mut().ask(
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
        )?
    };
    (context.kick)();
    // The task as it is now: paused, or still queued with the words kept.
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

/// `POST /api/tasks/<n>/finish`: what the worker of a working task said last is its result,
/// and what its turn left running is not ended (the owner's decision of 2026-10-18: the
/// chief can finish any working task). Only the engine reads the window, so it is asked,
/// and answers with the task done, or with why it is not one to finish (not working, no
/// member's open window works on it, no complete answer since its latest message). `jobs`
/// and `agents` count what the worker's harness still counted running.
async fn finish(context: &Context, caller: &Caller, task: &TaskThread) -> Result<Answer, Failure> {
    let asked = (context.finish)(
        caller.project.id,
        task.task.number,
        caller.participant.handle.clone(),
    )
    .await?;
    (context.kick)();
    Ok(Answer::ok(json!({
        "task": value(&TaskSummary::from(&asked.task))?,
        "jobs": asked.jobs,
        "agents": asked.agents,
    })))
}

/// Whether a tell reaches the task's window: one that has it, which it stops,
/// or one coming up, which it keeps the words for.
fn reaches(task: &TaskView) -> bool {
    task.assignee.is_some() && (WINDOWED.contains(&task.state.as_str()) || task.state == QUEUED)
}

/// Whether a tell reaches the task at all: its window, or, for a task still open, its
/// brief, which the words are added to.
fn tellable(task: &TaskView) -> bool {
    task.state == OPEN || reaches(task)
}

/// The moves of a task a coordinator makes with the ledger's own words: accept
/// its result, cancel it, pause it, resume it, give it back. A cancel says
/// which of the words the caller told the task's window went with it, the
/// window never having had them (`dropped`, when any did): the ledger tells
/// everyone else who told it some.
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
    let mut dropped = Vec::new();
    let moved = match action {
        TaskAction::Accept => ledger.accept_task(project, number, by)?,
        TaskAction::Cancel => {
            // Read in the same borrow as the cancel: nothing is pasted between.
            dropped = ledger
                .told_unreceived(project, number)?
                .into_iter()
                .filter(|told| told.sender.as_deref() == Some(by))
                .map(|told| told.id)
                .collect();
            ledger.cancel_task(project, number, by)?
        }
        TaskAction::Edit => ledger.edit_task(project, number, by, words)?,
        TaskAction::Pause => ledger.pause_task(project, number, Some(by), None)?,
        TaskAction::Resume => ledger.resume_task(project, number, Some(by), words)?.task,
        // `reopen`; and what Node's last branch took any other word for, which
        // only `transcript` is, and only when it is posted.
        _ => ledger.reopen_task(project, number, by, words)?.task,
    };
    drop(ledger);
    (context.kick)();
    let mut answer = Map::new();
    answer.insert("task".to_owned(), value(&TaskSummary::from(&moved))?);
    if !dropped.is_empty() {
        answer.insert("dropped".to_owned(), json!(dropped));
    }
    Ok(Answer::ok(Value::Object(answer)))
}

/// Whether the window may do what only the chief or the one who gave the task
/// may: read what it did, and move it.
pub(super) fn may_move(caller: &Caller, requester: &str) -> bool {
    caller.participant.role == "chief" || requester == caller.participant.handle
}

/// A task route's last word, as the path spells it.
fn word(action: TaskAction) -> &'static str {
    match action {
        TaskAction::Done => "done",
        TaskAction::Accept => "accept",
        TaskAction::Reopen => "reopen",
        TaskAction::Edit => "edit",
        TaskAction::Cancel => "cancel",
        TaskAction::Pause => "pause",
        TaskAction::Resume => "resume",
        TaskAction::Finish => "finish",
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
