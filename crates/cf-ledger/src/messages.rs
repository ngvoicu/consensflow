//! Messages: notes, questions and their answers (`src/ledger/messages.js`),
//! delivered into a window one at a time and oldest first, except the
//! human's, which wait in the app until read (`delivery`); and the human's
//! gate, which holds one agent's word to another until the human passes it
//! on or declines it (`gate`).

mod delivery;
mod gate;

use cf_proto::ledger::{MessageView, Question};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use crate::model::{self, require_active, LedgerError, MAX_BODY};
use crate::questions::{render_choices, render_questions, require_choices, require_questions};
use crate::queue::{queue, send, withdraw, Queued, Sent};
use crate::store::Store;
use crate::tasks::pause_task;
use crate::views::{message_view, TaskRow, MESSAGE_SELECT};

pub(crate) use delivery::{
    begin_delivery, cancel_message, confirm_delivery, fail_delivery, in_flight, next_delivery,
    retry_delivery, with_work,
};
pub(crate) use gate::{approve_message, decline_message, mark_read};

/// A note: from a participant (ConsensFlow itself when none) to another,
/// about one of the project's tasks or none.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewNote {
    pub from: Option<String>,
    pub to: String,
    pub body: String,
    pub task: Option<i64>,
}

/// A question for a coordinator: who asks it and whom, about which task,
/// and what it asks, in words or as a harness's question tool asked it, with
/// options; an urgent one is the chief's `cf tell` to a task's window.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewQuestion {
    pub from: Option<String>,
    pub to: String,
    pub body: Option<String>,
    pub task: Option<i64>,
    pub questions: Option<Value>,
    pub urgent: bool,
}

pub(crate) fn note(
    store: &mut Store,
    project_id: i64,
    note: &NewNote,
) -> Result<MessageView, LedgerError> {
    send(
        store,
        project_id,
        &Sent {
            from: note.from.as_deref(),
            to: &note.to,
            body: &note.body,
            task: note.task,
            kind: "note",
            ..Sent::default()
        },
    )
}

/// A question for a coordinator; the asker's task waits for the answer. The
/// human is never asked on the board: the chief asks them in its own
/// terminal, where they work with it. With questions, the question carries
/// options as a harness's own question tool asked them, and its text is
/// rendered from them. An urgent question is the chief's `cf tell` to a
/// task's window: the task is paused for it, and the question says so.
pub(crate) fn ask(
    store: &mut Store,
    project_id: i64,
    question: &NewQuestion,
) -> Result<MessageView, LedgerError> {
    let Some(from) = question.from.as_deref() else {
        return Err(LedgerError::refused(
            "unknown-participant",
            "a question names who asks it",
        ));
    };
    let options = question
        .questions
        .as_ref()
        .map(require_questions)
        .transpose()?;
    store.write(|store| {
        if store.participant_by_handle(project_id, &question.to)?.role == "human" {
            return Err(LedgerError::refused_with(
                "ask-in-your-terminal",
                "the human is asked in the chief's terminal, not on the board",
                403,
            ));
        }
        // The pause first, as one step with the tell: what it withdraws from
        // the window is never the tell, and a tell refused below pauses nothing.
        if question.urgent {
            let Some(number) = question.task else {
                return Err(LedgerError::refused_with(
                    "unknown-task",
                    format!("no task T-undefined in project {project_id}"),
                    404,
                ));
            };
            if store.task_row(project_id, number)?.state != "paused" {
                pause_task(store, project_id, number, Some(from), None)?;
            }
        }
        let body = match &options {
            Some(options) => render_questions(options),
            None => question.body.clone().unwrap_or_default(),
        };
        let message = send(
            store,
            project_id,
            &Sent {
                from: Some(from),
                to: &question.to,
                body: &body,
                task: question.task,
                kind: "question",
                questions: options.as_deref(),
                urgent: question.urgent,
            },
        )?;
        if let Some(number) = question.task {
            let row = store.task_row(project_id, number)?;
            let asker = store.participant_by_handle(project_id, from)?;
            if row.assignee_id == Some(asker.id) && row.state == "working" {
                store.move_task(&row, "waiting", json!({}))?;
            }
        }
        Ok(message)
    })
}

/// The answer goes back to whoever asked; the one asked answers. A plain
/// question's answer is delivered into the asker's window. A question with
/// options is answered by choice (or by text, one line per question): that
/// answer is read at once and never delivered, because the harness door
/// that asked collects it and the tool call completes with it; the asker's
/// task resumes here. A question on a cancelled task takes no answer, from
/// either side: nobody waits for it. `from` is the answerer's participant
/// id: handles repeat across projects (every chief is `chief`), ids never do.
pub(crate) fn answer(
    store: &mut Store,
    question_id: i64,
    from: i64,
    body: Option<&Value>,
    choices: Option<&Value>,
) -> Result<MessageView, LedgerError> {
    store.write(|store| {
        let question = store
            .message(question_id)?
            .filter(|question| question.kind == "question")
            .ok_or_else(|| {
                LedgerError::refused_with(
                    "not-a-question",
                    format!("message {question_id} is not a question"),
                    409,
                )
            })?;
        let answerer = store.participant_row(from)?;
        let (asker_id, task_id): (Option<i64>, Option<i64>) = store.db.query_row(
            "SELECT sender_id, task_id FROM message WHERE id = ?",
            [question_id],
            |row| Ok((row.get("sender_id")?, row.get("task_id")?)),
        )?;
        // The one asked, or (a question with options) the asker itself: its
        // window may have answered first, and the board's copy takes that answer.
        let from_window = !question.questions.is_null() && Some(answerer.id) == asker_id;
        if answerer.id != question.recipient_id && !from_window {
            return Err(LedgerError::refused_with(
                "not-your-question",
                format!(
                    "the question was put to {}, not {}",
                    question.recipient, answerer.handle
                ),
                403,
            ));
        }
        if let Some(task) = message_task(store, question_id)? {
            if task.state == "cancelled" {
                return Err(LedgerError::refused_with(
                    "task-cancelled",
                    format!("T-{} is cancelled: nobody waits for this answer", task.number),
                    409,
                ));
            }
        }
        if answered(store, question_id)? {
            return Err(LedgerError::refused_with(
                "already-answered",
                format!("m-{question_id} has its answer"),
                409,
            ));
        }
        let asked: Option<Vec<Question>> = match &question.questions {
            Value::Null => None,
            questions => Some(serde_json::from_value(questions.clone())?),
        };
        let picks = asked
            .as_deref()
            .map(|asked| require_choices(asked, choices, body))
            .transpose()?;
        let text = match (&asked, &picks) {
            (Some(asked), Some(picks)) => render_choices(asked, picks),
            _ => body.and_then(Value::as_str).unwrap_or_default().to_string(),
        };
        model::require_text(&text, "body", MAX_BODY)?;
        require_active(&answerer)?;
        let asker = store.participant_of(asker_id)?;
        require_active(&asker)?;
        let id = queue(
            store,
            question.project_id,
            &Queued {
                to: asker.id,
                from: Some(answerer.id),
                kind: "answer",
                task_id,
                reply_to: Some(question_id),
                body: &text,
                collected: picks.is_some(),
                choices: picks.as_deref(),
                ..Queued::default()
            },
        )?;
        store.log(
            question.project_id,
            "message.sent",
            json!({ "message": id, "kind": "answer", "from": answerer.handle, "to": question.sender }),
        )?;
        // A question still gated is answered before the one asked saw it: it goes no further.
        if question.state == "gated" {
            withdraw(store, question_id, &format!("answered by @{}", answerer.handle))?;
        }
        let answer = known_message(store, id)?;
        if answer.state == "read" {
            resume(store, task_id)?;
        }
        Ok(answer)
    })
}

/// A choice answer is read by the door that asked, at once: the asker's task goes on.
fn resume(store: &mut Store, task_id: Option<i64>) -> Result<(), LedgerError> {
    let Some(task_id) = task_id else {
        return Ok(());
    };
    let task = store
        .db
        .query_row("SELECT * FROM task WHERE id = ?", [task_id], TaskRow::read)?;
    if task.state == "waiting" {
        store.move_task(&task, "working", json!({}))?;
    }
    Ok(())
}

/// Whether a question on the task still waits for its answer.
fn unanswered(store: &Store, task_id: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message q WHERE q.task_id = ? AND q.kind = 'question'
           AND NOT EXISTS (
             SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer'
               AND a.state NOT IN ('gated', 'cancelled')
           )
         LIMIT 1",
        )?
        .exists([task_id])?)
}

/// Whether a question has an answer, on its way or still gated; a declined one never counts.
fn answered(store: &Store, question_id: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message WHERE reply_to = ? AND kind = 'answer' AND state != 'cancelled'",
        )?
        .exists([question_id])?)
}

/// The answer to a question, or none while it waits: for the one asked, or
/// for the human's approval.
pub(crate) fn answer_to(
    store: &Store,
    question_id: i64,
) -> Result<Option<MessageView>, LedgerError> {
    Ok(store
        .db
        .query_row(
            &format!(
                "{MESSAGE_SELECT} WHERE m.reply_to = ? AND m.kind = 'answer'
         AND m.state NOT IN ('gated', 'cancelled')
       ORDER BY m.id LIMIT 1"
            ),
            [question_id],
            message_view,
        )
        .optional()?)
}

/// What is on its way to a participant (queued, or being delivered), oldest first.
pub(crate) fn pending(store: &Store, participant_id: i64) -> Result<Vec<MessageView>, LedgerError> {
    messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.recipient_id = ? AND m.state IN ('queued', 'delivering')
       ORDER BY m.id"
        ),
        params![participant_id],
    )
}

/// A participant's messages, newest first, at most `limit`, whole: what
/// reached it or is on its way, never what still waits for the human. `cf
/// inbox` lists them over the local API.
pub(crate) fn inbox(
    store: &Store,
    participant_id: i64,
    limit: i64,
) -> Result<Vec<MessageView>, LedgerError> {
    messages(
        store,
        &format!(
            "{MESSAGE_SELECT} WHERE m.recipient_id = ? AND m.state != 'gated' ORDER BY m.id DESC LIMIT ?"
        ),
        params![participant_id, limit],
    )
}

/// The messages a `MESSAGE_SELECT` finds.
fn messages(
    store: &Store,
    sql: &str,
    values: impl rusqlite::Params,
) -> Result<Vec<MessageView>, LedgerError> {
    Ok(store
        .db
        .prepare(sql)?
        .query_map(values, message_view)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// A message the operation has just found or made.
fn known_message(store: &Store, id: i64) -> Result<MessageView, LedgerError> {
    store.message(id)?.ok_or_else(|| {
        LedgerError::refused_with("unknown-message", format!("no message {id}"), 404)
    })
}

/// A message in `state`, or a refusal saying which state it is in.
fn require_message(store: &Store, id: i64, state: &str) -> Result<MessageView, LedgerError> {
    let message = known_message(store, id)?;
    if message.state != state {
        return Err(LedgerError::refused_with(
            "invalid-transition",
            format!("message {id} is {}, not {state}", message.state),
            409,
        ));
    }
    Ok(message)
}

/// The task a message is about, if any.
fn message_task(store: &Store, message_id: i64) -> Result<Option<TaskRow>, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT t.* FROM task t JOIN message m ON m.task_id = t.id WHERE m.id = ?",
            [message_id],
            TaskRow::read,
        )
        .optional()?)
}
