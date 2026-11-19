//! The durable cursor of each unanswered question's typed reminders.

use rusqlite::{params, OptionalExtension};

use crate::messages::{has_standing_answer, note, NewNote};
use crate::model::LedgerError;
use crate::store::Store;
use crate::MessageView;

/// What the engine needs to time a question, without its text.
#[derive(Debug, Clone)]
pub struct QuestionWait {
    pub id: i64,
    pub task: Option<i64>,
    /// The task the question's sender actually holds, if that is its subject.
    pub held_task: Option<i64>,
    pub sender: String,
    pub recipient: String,
    pub recipient_id: i64,
    pub received_at: String,
    pub reminder: Option<MessageView>,
}

/// A member's question that reached the chief and still has no answer standing.
/// A gated question does not reach the chief until it is approved and received;
/// an answer at the gate stands, even though the member still awaits approval.
fn is_open() -> String {
    format!(
        "q.kind = 'question' AND q.urgent = 0
         AND ((q.state IN ('delivered', 'read') AND q.delivered_at IS NOT NULL)
           OR (q.state = 'delivering' AND EXISTS (SELECT 1 FROM question_reminder early
                   WHERE early.question_id=q.id AND early.first_read_at IS NOT NULL)))
         AND s.role NOT IN ('chief', 'human')
         AND s.left_at IS NULL AND r.role = 'chief' AND r.left_at IS NULL
         AND NOT {} AND (q.task_id IS NULL OR (t.id IS NOT NULL AND t.deleted_at IS NULL
             AND t.state NOT IN ('accepted', 'cancelled', 'failed')))",
        has_standing_answer("q")
    )
}

/// Open questions the chief has received, oldest first, and their last reminder.
pub(crate) fn waiting(store: &Store, project: i64) -> Result<Vec<QuestionWait>, LedgerError> {
    let rows: Vec<(QuestionWait, Option<i64>)> = store
        .db
        .prepare(&format!(
            "SELECT q.id, t.number, CASE WHEN t.assignee_id=q.sender_id
                    AND t.state IN ('working', 'waiting', 'paused') THEN t.number END,
                    s.handle, r.handle, q.recipient_id, coalesce(qr.first_read_at, q.delivered_at), qr.reminder_id
             FROM message q JOIN participant s ON s.id=q.sender_id
             JOIN participant r ON r.id=q.recipient_id LEFT JOIN task t ON t.id=q.task_id
             LEFT JOIN question_reminder qr ON qr.question_id=q.id
             WHERE q.project_id=? AND {} ORDER BY coalesce(qr.first_read_at, q.delivered_at), q.id",
            is_open()
        ))?
        .query_map([project], |row| {
            Ok((
                QuestionWait {
                    id: row.get(0)?,
                    task: row.get(1)?,
                    held_task: row.get(2)?,
                    sender: row.get(3)?,
                    recipient: row.get(4)?,
                    recipient_id: row.get(5)?,
                    received_at: row.get(6)?,
                    reminder: None,
                },
                row.get(7)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    rows.into_iter()
        .map(|(mut wait, prior)| {
            wait.reminder = prior.map(|id| store.message(id)).transpose()?.flatten();
            Ok(wait)
        })
        .collect()
}

/// A question printed whole to the chief while its paste is still in flight
/// has already reached the chief; later confirmation must not restart the clock.
pub(crate) fn read_early(store: &mut Store, question: i64) -> Result<(), LedgerError> {
    let at = store.at();
    store.db.execute(
        "INSERT INTO question_reminder(question_id, first_read_at) VALUES (?1, ?2)
         ON CONFLICT(question_id) DO UPDATE SET first_read_at=coalesce(first_read_at, excluded.first_read_at)",
        params![question, at],
    )?;
    Ok(())
}

/// The check and write are one transaction; a second look cannot stack a note.
pub(crate) fn remind(
    store: &mut Store,
    project: i64,
    id: i64,
    previous: Option<i64>,
    new_note: &NewNote,
) -> Result<Option<MessageView>, LedgerError> {
    store.write(|store| {
        let current: Option<(Option<i64>, Option<String>)> = store.db.query_row(
            &format!("SELECT qr.reminder_id, prior.state FROM message q
                JOIN participant s ON s.id=q.sender_id JOIN participant r ON r.id=q.recipient_id
                LEFT JOIN task t ON t.id=q.task_id LEFT JOIN question_reminder qr ON qr.question_id=q.id
                LEFT JOIN message prior ON prior.id=qr.reminder_id
                WHERE q.id=?1 AND q.project_id=?2 AND {}", is_open()),
            params![id, project], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let Some((last, state)) = current else { return Ok(None) };
        if last != previous || state.as_deref().is_some_and(|state| matches!(state, "queued" | "gated" | "delivering")) {
            return Ok(None);
        }
        let sent = note(store, project, new_note)?;
        store.db.execute(
            "INSERT INTO question_reminder(question_id, reminder_id, ended) VALUES (?1, ?2, 0)
             ON CONFLICT(question_id) DO UPDATE SET reminder_id=excluded.reminder_id, ended=0",
            params![id, sent.id],
        )?;
        Ok(Some(sent))
    })
}

/// End the reminder when the question is answered or withdrawn. A paste
/// already handed stays intact, but if returned it cannot be sent again.
pub(crate) fn end(store: &mut Store, question: i64) -> Result<(), LedgerError> {
    let reminder: Option<(i64, bool, i64)> = store.db.query_row(
        "SELECT m.id, m.handed_at IS NOT NULL, m.project_id FROM question_reminder qr
         JOIN message m ON m.id=qr.reminder_id WHERE qr.question_id=? AND m.state IN ('queued','delivering')",
        [question], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    ).optional()?;
    store.db.execute(
        "UPDATE question_reminder SET ended=1 WHERE question_id=?",
        [question],
    )?;
    if let Some((reminder, false, project)) = reminder {
        crate::messages::withdraw_each(
            store,
            project,
            &[reminder],
            "the question no longer waits",
        )?;
    }
    Ok(())
}

/// Finish closes the questions of the task's own window and their reminders.
pub(crate) fn end_for_task(
    store: &mut Store,
    task: i64,
    assignee: Option<i64>,
) -> Result<(), LedgerError> {
    let ids: Vec<i64> = store
        .db
        .prepare(
            "SELECT q.id FROM question_reminder qr JOIN message q ON q.id=qr.question_id
         WHERE q.task_id=? AND q.sender_id=? AND q.kind='question' AND q.urgent=0",
        )?
        .query_map(params![task, assignee], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for id in ids {
        end(store, id)?;
    }
    Ok(())
}

/// A queued or returned reminder is stale once its question no longer waits.
pub(crate) fn stale_reminder(store: &Store, reminder: i64) -> Result<bool, LedgerError> {
    Ok(store
        .db
        .prepare(&format!(
            "SELECT 1 FROM question_reminder qr JOIN message reminder ON reminder.id=qr.reminder_id
        WHERE qr.reminder_id=? AND reminder.handed_at IS NULL AND (qr.ended=1 OR NOT EXISTS (
            SELECT 1 FROM message q JOIN participant s ON s.id=q.sender_id
            JOIN participant r ON r.id=q.recipient_id LEFT JOIN task t ON t.id=q.task_id
            WHERE q.id=qr.question_id AND {}))",
            is_open()
        ))?
        .exists([reminder])?)
}
