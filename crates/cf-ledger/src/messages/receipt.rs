//! Receipt: what makes a message received, and what that does to its task.
//! A message is received when its window's record shows its marker, when
//! the paste that carried it was confirmed, when a door handed an answer to
//! its harness and said so, or when `cf` served an answer's whole body to
//! the one it was for. Withdrawing a message never resolves what it obliged:
//! only receipt does. A task moves between `queued`, `working` and `waiting`
//! in one place, [`reconcile`], after each receipt.
//!
//! A door is the hook or broker that asked a question for a harness's
//! question tool and waits for its answer. It claims an answer, hands it to
//! its harness, and then says so; until it has, the answer is still the
//! ledger's to deliver as text, and a claim that no one acknowledged is
//! voided at the first sign that its door is gone.

use cf_proto::ledger::{Claim, MessageView};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use super::{known_message, message_task};
use crate::model::LedgerError;
use crate::store::Store;
use crate::views::{message_view, TaskRow, MESSAGE_SELECT};

/// How `cf` served an answer whole: to its recipient's `cf inbox read`, or in
/// the thread `cf task get` printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    Inbox,
    Task,
}

impl Read {
    fn word(self) -> &'static str {
        match self {
            Read::Inbox => "inbox",
            Read::Task => "task",
        }
    }
}

/// Whether a task message for the window is still on its way: queued, held
/// for the human, or being pasted.
fn barrier(store: &Store, task: i64, window: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message WHERE task_id = ? AND recipient_id = ? AND kind = 'task'
               AND state IN ('queued', 'gated', 'delivering')",
        )?
        .exists(params![task, window])?)
}

/// Whether the window received a task message about the task: with none
/// still on its way ([`barrier`]), the words it was given are what it has.
fn arrived(store: &Store, task: i64, window: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message WHERE task_id = ? AND recipient_id = ? AND kind = 'task'
               AND state IN ('delivered', 'read')",
        )?
        .exists(params![task, window])?)
}

/// Whether the window asked a question on the task that no received answer
/// resolves. A question the ledger withdrew still obliges once it has an
/// answer, whatever became of that answer: only its transport was taken back,
/// and an answer that failed, was cancelled or declined was not received. One
/// withdrawn with no answer at all (its asker's window ended first, its task
/// was failed) obliges no more: nobody has anything to answer.
fn outstanding(store: &Store, task: i64, window: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM message q
             WHERE q.task_id = ?1 AND q.kind = 'question' AND q.sender_id = ?2 AND q.urgent = 0
               AND NOT EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id
                               AND a.kind = 'answer' AND a.state IN ('delivered', 'read'))
               AND (q.state != 'cancelled'
                    OR EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id
                               AND a.kind = 'answer'))
             LIMIT 1",
        )?
        .exists(params![task, window])?)
}

/// The one place a task moves between `queued`, `working` and `waiting`:
/// - a `queued` task goes on once no task message for its window is still on
///   its way and one was received: to `waiting` if the window has a question
///   nobody received an answer to, else to `working`;
/// - a `working` task waits for such a question;
/// - a `waiting` task works again once none is left;
/// - any other state stays as it is.
pub(crate) fn reconcile(store: &mut Store, task_id: i64) -> Result<(), LedgerError> {
    let task = store
        .db
        .query_row("SELECT * FROM task WHERE id = ?", [task_id], TaskRow::read)?;
    let Some(window) = task.assignee_id else {
        return Ok(());
    };
    let to = match task.state.as_str() {
        "queued" => {
            if barrier(store, task.id, window)? || !arrived(store, task.id, window)? {
                return Ok(());
            }
            if outstanding(store, task.id, window)? {
                "waiting"
            } else {
                "working"
            }
        }
        "working" if outstanding(store, task.id, window)? => "waiting",
        "waiting" if !outstanding(store, task.id, window)? => "working",
        _ => return Ok(()),
    };
    store.move_task(&task, to, json!({}))
}

/// Whether a question its window asks about `task` comes from a turn that
/// is an old one already, and is born with its door shut: the task is
/// paused, or queued with the words that send it on still waiting for their
/// paste, so the window is still on the turn that came before them. Its
/// answer comes as a message, with those words, when the task goes on.
pub(crate) fn door_born_closed(store: &Store, task: &TaskRow) -> Result<bool, LedgerError> {
    let Some(window) = task.assignee_id else {
        return Ok(false);
    };
    Ok(match task.state.as_str() {
        "paused" => true,
        "queued" => store
            .db
            .prepare(
                "SELECT 1 FROM message WHERE task_id = ? AND recipient_id = ? AND kind = 'task'
                   AND state IN ('queued', 'gated')",
            )?
            .exists(params![task.id, window])?,
        _ => false,
    })
}

/// A door asks for the answer to its question, as the one who asked it. In
/// one step:
/// - a shut door, or a task paused, is closed;
/// - no answer yet that was not given up on, or one still held for the
///   human, waits;
/// - an answer still `queued` and in no paste is claimed for the door: the
///   paste skips it from now, until the door says it handed it over, or the
///   claim is voided;
/// - an answer the door took already, and its reply was lost, is answered
///   again, writing nothing;
/// - an answer that rides in a paste, is being pasted or was pasted is
///   closed: it comes as text.
pub(crate) fn claim_answer(
    store: &mut Store,
    question_id: i64,
    asker: i64,
) -> Result<Claim, LedgerError> {
    store.write(|store| {
        let (sender, task, shut): (Option<i64>, Option<i64>, Option<String>) = store
            .db
            .query_row(
                "SELECT sender_id, task_id, door_closed_at FROM message
                 WHERE id = ? AND kind = 'question'",
                [question_id],
                |row| Ok((row.get("sender_id")?, row.get("task_id")?, row.get("door_closed_at")?)),
            )
            .optional()?
            .ok_or_else(|| {
                LedgerError::refused_with(
                    "not-a-question",
                    format!("message {question_id} is not a question"),
                    409,
                )
            })?;
        if sender != Some(asker) {
            return Err(LedgerError::refused_with(
                "not-your-question",
                format!("m-{question_id} was asked by someone else"),
                403,
            ));
        }
        let paused = match task {
            Some(task) => {
                let state: String = store.db.query_row(
                    "SELECT state FROM task WHERE id = ?",
                    [task],
                    |row| row.get(0),
                )?;
                state == "paused"
            }
            None => false,
        };
        if shut.is_some() || paused {
            return Ok(Claim::Closed);
        }
        let found: Option<(i64, String, Option<i64>, Option<String>)> = store
            .db
            .query_row(
                "SELECT id, state, carried_by, claimed_at FROM message
                 WHERE reply_to = ? AND kind = 'answer' AND state NOT IN ('gated', 'cancelled', 'failed')
                 ORDER BY id LIMIT 1",
                [question_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((answer, state, carrier, claimed)) = found else {
            return Ok(Claim::Waiting);
        };
        match (state.as_str(), carrier) {
            ("read", _) => Ok(Claim::Answered(Box::new(known_message(store, answer)?))),
            ("queued", None) => {
                if claimed.is_none() {
                    let at = store.at();
                    store.db.execute(
                        "UPDATE message SET claimed_at = ? WHERE id = ?",
                        params![at, answer],
                    )?;
                    let project = known_message(store, answer)?.project_id;
                    store.log(
                        project,
                        "delivery.claimed",
                        json!({ "message": answer, "question": question_id }),
                    )?;
                }
                Ok(Claim::Answered(Box::new(known_message(store, answer)?)))
            }
            _ => Ok(Claim::Closed),
        }
    })
}

/// A door says whether it handed the answer it claimed to its harness, as
/// the one the answer was for:
/// - handed over, the answer still `queued`, in no paste, its door open: it is
///   `read`, with the door's receipt, and its task is reconciled;
/// - handed over, and read already: nothing more;
/// - handed over, and anything else (the door was shut meanwhile, the answer
///   was pasted as text): refused `door-closed`, and the answer comes as text;
/// - not handed over: the claim is given back, and the answer is the ledger's.
pub(crate) fn settle_claim(
    store: &mut Store,
    answer_id: i64,
    recipient: i64,
    received: bool,
) -> Result<(), LedgerError> {
    store.write(|store| {
        let answer = store
            .message(answer_id)?
            .filter(|answer| answer.kind == "answer" && answer.recipient_id == recipient)
            .ok_or_else(|| {
                LedgerError::refused_with(
                    "unknown-message",
                    format!("no answer m-{answer_id} for you"),
                    404,
                )
            })?;
        if !received {
            store.db.execute(
                "UPDATE message SET claimed_at = NULL WHERE id = ? AND state = 'queued'",
                [answer_id],
            )?;
            return Ok(());
        }
        if answer.state == "read" {
            return Ok(());
        }
        let handed = answer.state == "queued"
            && store
                .db
                .prepare(
                    "SELECT 1 FROM message a JOIN message q ON q.id = a.reply_to
                     WHERE a.id = ? AND a.carried_by IS NULL AND q.door_closed_at IS NULL",
                )?
                .exists([answer_id])?;
        if !handed {
            return Err(LedgerError::refused_with(
                "door-closed",
                format!("m-{answer_id} is not answered here: it comes to you as a message"),
                409,
            ));
        }
        received_as(store, &answer, &json!({ "door": true }))?;
        reconcile_of(store, &answer)
    })
}

/// An answer served whole to the one it was for by `cf`, whichever way: the
/// answers among `ids` that are still `queued` for `recipient`, a carried or
/// claimed one included (its claim is given up), are `read`, each with its
/// receipt, and their tasks reconciled, once each. What is not an answer for
/// them, or has been received, is left as it is.
pub(crate) fn receive_read(
    store: &mut Store,
    recipient: i64,
    ids: &[i64],
    via: Read,
) -> Result<(), LedgerError> {
    store.write(|store| {
        let mut tasks = Vec::new();
        for id in ids {
            let Some(answer) = store.message(*id)?.filter(|answer| {
                answer.kind == "answer"
                    && answer.recipient_id == recipient
                    && answer.state == "queued"
            }) else {
                continue;
            };
            received_as(store, &answer, &json!({ "read": via.word() }))?;
            if let Some(task) = message_task(store, answer.id)? {
                if !tasks.contains(&task.id) {
                    tasks.push(task.id);
                }
            }
        }
        for task in tasks {
            reconcile(store, task)?;
        }
        Ok(())
    })
}

/// The answer is `read`: when, and the proof, its claim given up.
fn received_as(
    store: &mut Store,
    answer: &MessageView,
    receipt: &Value,
) -> Result<(), LedgerError> {
    let at = store.at();
    store.db.execute(
        "UPDATE message SET state = 'read', delivered_at = ?, receipt = ?, claimed_at = NULL
         WHERE id = ?",
        params![at, receipt.to_string(), answer.id],
    )?;
    store.log(
        answer.project_id,
        "message.read",
        json!({ "message": answer.id, "receipt": receipt }),
    )
}

/// The task an answer is about is reconciled.
fn reconcile_of(store: &mut Store, answer: &MessageView) -> Result<(), LedgerError> {
    match message_task(store, answer.id)? {
        Some(task) => reconcile(store, task.id),
        None => Ok(()),
    }
}

/// Every claim on an answer for `participant` that no door has acknowledged
/// is voided, and `because` is said in the log: the answer is the ledger's
/// again, to deliver as text. The first look that finds the window at rest,
/// and its exit, do this: the door that claimed it is as good as gone.
pub(crate) fn release_claims(
    store: &mut Store,
    participant: i64,
    because: &str,
) -> Result<(), LedgerError> {
    release(store, Some(participant), because)
}

/// Every claim of every window is voided, as a daemon that starts again does:
/// no door survives it.
pub(crate) fn release_all_claims(store: &mut Store) -> Result<(), LedgerError> {
    release(store, None, "the daemon started again")
}

fn release(store: &mut Store, participant: Option<i64>, because: &str) -> Result<(), LedgerError> {
    store.write(|store| {
        let claimed: Vec<MessageView> = store
            .db
            .prepare(&format!(
                "{MESSAGE_SELECT} WHERE m.claimed_at IS NOT NULL AND m.state = 'queued'
                   AND (?1 IS NULL OR m.recipient_id = ?1) ORDER BY m.id"
            ))?
            .query_map([participant], message_view)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut tasks = Vec::new();
        for answer in &claimed {
            store.db.execute(
                "UPDATE message SET claimed_at = NULL WHERE id = ?",
                [answer.id],
            )?;
            store.log(
                answer.project_id,
                "delivery.unclaimed",
                json!({ "message": answer.id, "because": because }),
            )?;
            if let Some(task) = message_task(store, answer.id)? {
                if !tasks.contains(&task.id) {
                    tasks.push(task.id);
                }
            }
        }
        for task in tasks {
            reconcile(store, task)?;
        }
        Ok(())
    })
}
