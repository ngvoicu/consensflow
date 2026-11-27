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
    /// The task named as the subject, if any.
    pub task: Option<i64>,
    /// The finishable task the question's sender actually holds, if any.
    pub held_task: Option<i64>,
    pub sender: String,
    pub recipient: String,
    pub recipient_id: i64,
    pub received_at: String,
    pub reminder: Option<MessageView>,
    /// For a question a restart or a project Close closed: how it ended
    /// (`ConsensFlow restarted`, `the project closed`). None for an open one.
    pub closed: Option<String>,
}

/// For a tracked question its window's holding is distinct from its topic.
/// A pre-tracking question continues to use its topic as it did before.
fn holding_join() -> String {
    format!(
        "LEFT JOIN task t ON t.id={}",
        crate::holding::task_for_question_sql("q")
    )
}

/// A tracked question, or its reminder, whose topic is another task. A taskless
/// or pre-tracking question keeps the older topic-based handling.
pub(crate) fn other_holding_sql(message: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM question_binding b
         WHERE b.closed=0 AND b.task_id IS NOT NULL AND b.task_id != {message}.task_id
           AND (b.question_id={message}.id OR b.question_id=(SELECT qr.question_id
                FROM question_reminder qr WHERE qr.reminder_id={message}.id)))"
    )
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
         AND NOT {} AND ({} IS NULL OR (t.id IS NOT NULL AND t.deleted_at IS NULL
             AND t.state NOT IN ('accepted', 'cancelled', 'failed')))",
        has_standing_answer("q"),
        crate::holding::task_for_question_sql("q")
    )
}

/// Open questions the chief has received, oldest first, and their last reminder.
pub(crate) fn waiting(store: &Store, project: i64) -> Result<Vec<QuestionWait>, LedgerError> {
    let rows: Vec<(QuestionWait, Option<i64>)> = store
        .db
        .prepare(&format!(
            "SELECT q.id, topic.number, CASE WHEN t.assignee_id=q.sender_id
                    AND t.state IN ('working', 'waiting', 'paused') THEN t.number END,
                    s.handle, r.handle, q.recipient_id, coalesce(qr.first_read_at, q.delivered_at), qr.reminder_id
             FROM message q JOIN participant s ON s.id=q.sender_id
             JOIN participant r ON r.id=q.recipient_id LEFT JOIN task topic ON topic.id=q.task_id
             {}
             LEFT JOIN question_reminder qr ON qr.question_id=q.id
             WHERE q.project_id=? AND {} ORDER BY coalesce(qr.first_read_at, q.delivered_at), q.id",
            holding_join(), is_open()
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
                    closed: None,
                },
                row.get(7)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut waits = rows
        .into_iter()
        .map(|(mut wait, prior)| {
            wait.reminder = prior.map(|id| store.message(id)).transpose()?.flatten();
            Ok(wait)
        })
        .collect::<Result<Vec<_>, LedgerError>>()?;
    waits.extend(closed_waiting(store, project)?);
    Ok(waits)
}

/// The closed waits (Q2) that are eligible, from `question_wait w`: the question's
/// binding still holds T-1869's wait (`closed = 2`), its task is still the asking
/// member's, waiting or working, not deleted, in an open project; the member is on
/// the staff and the chief is the question's chief; and the chief has not acted
/// since the close: no message of the project's chief to that member on that task
/// holds a higher id than the close's place (a queued or gated one counts, one
/// that failed or was taken back does not). Read, never kept, so each end of the
/// wait ends the reminders with no step of its own. The clock is not part of it.
const CLOSED_FROM: &str = "FROM question_wait w
    JOIN question_binding b ON b.question_id=w.question_id AND b.closed=2
    JOIN message q ON q.id=w.question_id AND q.kind='question'
    JOIN task t ON t.id=b.task_id AND t.project_id=q.project_id AND t.assignee_id=b.participant_id
    JOIN project pr ON pr.id=q.project_id AND pr.state='open'
    JOIN participant s ON s.id=q.sender_id AND s.project_id=q.project_id
    JOIN participant r ON r.id=q.recipient_id AND r.project_id=q.project_id
    LEFT JOIN message n ON n.id=w.note_id
    WHERE t.deleted_at IS NULL AND t.state IN ('waiting','working')
      AND s.left_at IS NULL AND s.role NOT IN ('chief','human')
      AND r.role='chief' AND r.left_at IS NULL
      AND NOT EXISTS (SELECT 1 FROM message a JOIN participant c ON c.id=a.sender_id
            WHERE c.role='chief' AND a.recipient_id=b.participant_id AND a.task_id=t.id
              AND a.kind IN ('task','note') AND a.id > w.after_id
              AND a.state IN ('queued','gated','delivering','delivered','read'))";

/// When a closed question's reminders are counted from: the update, for a wait
/// an earlier build left with no note the ledger names; else when the chief first
/// had the note, its earliest inbox read while the paste was on its way, else its
/// delivery. None until it did.
const CLOSED_CLOCK: &str = "coalesce(w.from_update_at, w.first_read_at, n.delivered_at)";

/// The closed questions' waits whose clock has started, oldest first.
fn closed_waiting(store: &Store, project: i64) -> Result<Vec<QuestionWait>, LedgerError> {
    let rows: Vec<(QuestionWait, Option<i64>)> = store
        .db
        .prepare(&format!(
            "SELECT q.id, t.number, s.handle, r.handle, q.recipient_id, {CLOSED_CLOCK}, w.reminder_id, q.reason
             {CLOSED_FROM} AND q.project_id=? AND {CLOSED_CLOCK} IS NOT NULL
             ORDER BY {CLOSED_CLOCK}, q.id"
        ))?
        .query_map([project], |row| {
            let reason: Option<String> = row.get(7)?;
            let number: i64 = row.get(1)?;
            Ok((
                QuestionWait {
                    id: row.get(0)?,
                    task: Some(number),
                    held_task: Some(number),
                    sender: row.get(2)?,
                    recipient: row.get(3)?,
                    recipient_id: row.get(4)?,
                    received_at: row.get(5)?,
                    reminder: None,
                    closed: Some(
                        if reason.as_deref() == Some(crate::messages::CLOSED_PROJECT_QUESTION) {
                            crate::messages::CLOSED_PROJECT_QUESTION
                        } else {
                            crate::messages::RESTARTED_QUESTION
                        }
                        .to_owned(),
                    ),
                },
                row.get(6)?,
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

/// A close keeps its wait: the close's place is the last message written so far.
/// The note is linked when it is written ([`link_note`]).
pub(crate) fn begin_wait(store: &Store, question: i64) -> Result<(), LedgerError> {
    store.db.execute(
        "INSERT OR IGNORE INTO question_wait(question_id, after_id)
         VALUES (?1, (SELECT COALESCE(MAX(id), 0) FROM message))",
        [question],
    )?;
    Ok(())
}

/// The one note the chief was given for the closed question: the first written is
/// kept. A question with no wait keeps nothing.
pub(crate) fn link_note(store: &Store, question: i64, note: i64) -> Result<(), LedgerError> {
    store.db.execute(
        "UPDATE question_wait SET note_id = coalesce(note_id, ?1) WHERE question_id = ?2",
        params![note, question],
    )?;
    Ok(())
}

/// The note was read in the inbox while its paste was on its way: that is the
/// chief's earliest receipt, whenever the paste is confirmed.
pub(crate) fn note_read_early(store: &mut Store, note: i64) -> Result<(), LedgerError> {
    let at = store.at();
    store.db.execute(
        "UPDATE question_wait SET first_read_at = coalesce(first_read_at, ?1) WHERE note_id = ?2",
        params![at, note],
    )?;
    Ok(())
}

/// The reminder's note, known as a reminder from now on whatever the cursors name.
fn reminder_note(
    store: &mut Store,
    project: i64,
    new_note: &NewNote,
) -> Result<MessageView, LedgerError> {
    let sent = note(store, project, new_note)?;
    store.db.execute(
        "INSERT INTO question_reminder_sent(message_id) VALUES (?1)",
        [sent.id],
    )?;
    Ok(sent)
}

/// A reminder of a closed question, in one write with its eligibility, its own
/// cursor and the queued reminder. None where the wait is not eligible, the
/// cursor is not `previous`, or a reminder is still on its way.
fn remind_closed(
    store: &mut Store,
    project: i64,
    id: i64,
    previous: Option<i64>,
    new_note: &NewNote,
) -> Result<Option<MessageView>, LedgerError> {
    store.write(|store| {
        let current: Option<(Option<i64>, Option<String>)> = store
            .db
            .query_row(
                &format!(
                    "SELECT w.reminder_id, (SELECT state FROM message WHERE id=w.reminder_id)
                     {CLOSED_FROM} AND q.id=?1 AND q.project_id=?2 AND {CLOSED_CLOCK} IS NOT NULL"
                ),
                params![id, project],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((last, state)) = current else {
            return Ok(None);
        };
        if last != previous
            || state
                .as_deref()
                .is_some_and(|state| matches!(state, "queued" | "gated" | "delivering"))
        {
            return Ok(None);
        }
        let sent = reminder_note(store, project, new_note)?;
        store.db.execute(
            "UPDATE question_wait SET reminder_id = ?1 WHERE question_id = ?2",
            params![sent.id, id],
        )?;
        Ok(Some(sent))
    })
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
        let current: Option<(Option<i64>, Option<String>)> = store
            .db
            .query_row(
                &format!(
                    "SELECT qr.reminder_id, prior.state FROM message q
                JOIN participant s ON s.id=q.sender_id JOIN participant r ON r.id=q.recipient_id
                {} LEFT JOIN question_reminder qr ON qr.question_id=q.id
                LEFT JOIN message prior ON prior.id=qr.reminder_id
                WHERE q.id=?1 AND q.project_id=?2 AND {}",
                    holding_join(),
                    is_open()
                ),
                params![id, project],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((last, state)) = current else {
            return remind_closed(store, project, id, previous, new_note);
        };
        if last != previous
            || state
                .as_deref()
                .is_some_and(|state| matches!(state, "queued" | "gated" | "delivering"))
        {
            return Ok(None);
        }
        let sent = reminder_note(store, project, new_note)?;
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
        .prepare(&format!(
            "SELECT q.id FROM question_reminder qr JOIN message q ON q.id=qr.question_id
         WHERE {}=? AND q.sender_id=? AND q.kind='question' AND q.urgent=0",
            crate::holding::task_for_question_sql("q")
        ))?
        .query_map(params![task, assignee], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for id in ids {
        end(store, id)?;
    }
    Ok(())
}

/// A queued or returned reminder is stale once its question no longer waits.
pub(crate) fn stale_reminder(store: &Store, reminder: i64) -> Result<bool, LedgerError> {
    let open = store
        .db
        .prepare(&format!(
            "SELECT 1 FROM question_reminder qr JOIN message reminder ON reminder.id=qr.reminder_id
        WHERE qr.reminder_id=? AND reminder.handed_at IS NULL AND (qr.ended=1 OR NOT EXISTS (
            SELECT 1 FROM message q JOIN participant s ON s.id=q.sender_id
            JOIN participant r ON r.id=q.recipient_id {}
            WHERE q.id=qr.question_id AND {}))",
            holding_join(),
            is_open()
        ))?
        .exists([reminder])?;
    if open {
        return Ok(true);
    }
    // A project's Close holds a reminder, as transport does: it is judged at the
    // Resume, when the tasks it paused are at work again.
    Ok(store
        .db
        .prepare(&format!(
            "SELECT 1 FROM question_wait own JOIN message reminder ON reminder.id=own.reminder_id
             JOIN project held ON held.id=reminder.project_id AND held.state='open'
             WHERE own.reminder_id=? AND reminder.handed_at IS NULL AND NOT EXISTS (
               SELECT 1 {CLOSED_FROM} AND w.question_id=own.question_id)"
        ))?
        .exists([reminder])?)
}

/// Why a reminder is not shown to its recipient now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withheld {
    /// A chief's meeting holds it: it is deliverable once the meeting ends, and a
    /// reader is told it waits.
    Meeting,
    /// It no longer applies or is held otherwise (a Close, its wait ended, it was
    /// withdrawn or failed): it is nobody's to read.
    Gone,
}

/// Whether `message` is a reminder its recipient is not to be shown now: listed,
/// served or received, it is one transport would hand over this moment, or one the
/// recipient has had. A reminder the recipient has had stays its own whatever
/// happens to its wait: delivered or read, handed to its window, or read whole
/// while its paste was on its way. One it has not had is shown only while the
/// project is open, its wait stands and no meeting holds it; one cancelled or
/// failed before the recipient had it is nobody's. Any other message is shown.
/// Nothing is changed by asking.
pub(crate) fn withheld(store: &Store, message: i64) -> Result<Option<Withheld>, LedgerError> {
    let found: Option<(String, bool, bool, bool)> = store
        .db
        .query_row(
            "SELECT m.state,
                    m.handed_at IS NOT NULL OR m.delivered_at IS NOT NULL
                      OR COALESCE(json_extract(m.receipt, '$.readEarly'), '') != '',
                    p.state='open',
                    EXISTS (SELECT 1 FROM message held WHERE held.id = m.id AND {held})
             FROM question_reminder_sent sent JOIN message m ON m.id=sent.message_id
             JOIN project p ON p.id=m.project_id WHERE sent.message_id=?"
                .replace("{held}", &crate::meeting::held_by_meeting("held"))
                .as_str(),
            [message],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((state, had, open, in_meeting)) = found else {
        return Ok(None);
    };
    if had || matches!(state.as_str(), "delivered" | "read") {
        return Ok(None);
    }
    Ok(match state.as_str() {
        "queued" | "delivering" => {
            if !open || stale_reminder(store, message)? {
                Some(Withheld::Gone)
            } else if in_meeting {
                Some(Withheld::Meeting)
            } else {
                None
            }
        }
        _ => Some(Withheld::Gone),
    })
}

#[cfg(test)]
mod tests {
    use super::{holding_join, is_open};
    use rusqlite::Connection;

    #[test]
    fn checking_unanswered_questions_uses_an_index_to_find_each_answer() {
        let db = Connection::open_in_memory().unwrap();
        crate::migrate(&db, &crate::MIGRATIONS).unwrap();
        let sql = format!(
            "EXPLAIN QUERY PLAN SELECT q.id FROM message q
             JOIN participant s ON s.id=q.sender_id
             JOIN participant r ON r.id=q.recipient_id
             {}
             WHERE q.project_id=1 AND {}",
            holding_join(),
            is_open()
        );
        let plan: Vec<String> = db
            .prepare(&sql)
            .unwrap()
            .query_map([], |row| row.get(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            plan.iter().any(|step| step.starts_with("SEARCH a USING")),
            "each question must find answers by index, not scan all messages: {plan:?}"
        );
    }
}
