//! The brief actually handed to a window, and the questions of that holding.
//! These rows are written only as new handovers and questions occur; opening a
//! ledger never reconstructs them from old messages or the event history.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use serde_json::json;

use crate::model::LedgerError;
use crate::queue::{queue, Queued};
use crate::store::Store;

// A process lifetime is memory only: opening a ledger never rewrites rows.
// A new dispatcher gets a distinct suffix, including in restart fixtures.
fn process_run() -> &'static str {
    static RUN: OnceLock<String> = OnceLock::new();
    RUN.get_or_init(|| {
        format!(
            "{}-{}-{:016x}-{:016x}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |v| v.as_nanos()),
            RandomState::new().build_hasher().finish(),
            RandomState::new().build_hasher().finish()
        )
    })
}

pub(crate) fn initial_run() -> String {
    format!("{}-0", process_run())
}
pub(crate) fn new_run() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("{}-{}", process_run(), NEXT.fetch_add(1, Ordering::Relaxed))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowHolding {
    pub task_id: i64,
    pub task_number: i64,
    pub first_brief: i64,
    pub last_brief: i64,
    pub active: bool,
    /// Its physical window ended, but the same task may resume on its own conversation.
    pub resumable: bool,
}

pub(crate) fn of(store: &Store, window: i64) -> Result<Option<WindowHolding>, LedgerError> {
    let row: Option<(i64, i64, i64, i64, i64, String, bool)> = store
        .db
        .query_row(
            "SELECT h.task_id, t.number, h.first_brief_id, h.last_brief_id, h.closed, h.run_id,
                EXISTS(SELECT 1 FROM message b JOIN participant p ON p.id = h.participant_id
                  WHERE b.id = h.last_brief_id AND b.kind = 'task' AND b.task_id = t.id
                    AND b.project_id = t.project_id AND p.project_id = t.project_id
                    AND b.recipient_id = h.participant_id)
                AND EXISTS(SELECT 1 FROM message b WHERE b.id = h.first_brief_id
                    AND b.kind = 'task' AND b.task_id = t.id AND b.project_id = t.project_id
                    AND b.recipient_id = h.participant_id)
         FROM question_holding h JOIN task t ON t.id = h.task_id WHERE h.participant_id = ?",
            [window],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional()?;
    let Some((task_id, task_number, first_brief, last_brief, closed, run, valid)) = row else {
        return Ok(None);
    };
    if !valid {
        return Err(LedgerError::refused_with(
            "question-conflict",
            "this window's brief and task records disagree, so the question cannot be placed",
            409,
        ));
    }
    Ok(Some(WindowHolding {
        task_id,
        task_number,
        first_brief,
        last_brief,
        active: closed == 0 && run == store.question_run,
        resumable: closed == 2,
    }))
}

/// The one check for an asking window and an answer to it. `first_brief` is
/// the generation of a task holding; taskless questions belong to the window.
pub(crate) fn matches(
    store: &Store,
    window: i64,
    task: Option<i64>,
    first_brief: Option<i64>,
) -> Result<bool, LedgerError> {
    let live = store
        .db
        .prepare("SELECT 1 FROM participant WHERE id = ? AND left_at IS NULL")?
        .exists([window])?;
    if !live {
        return Ok(false);
    }
    let Some(task) = task else {
        return Ok(first_brief.is_none());
    };
    let Some(first_brief) = first_brief else {
        return Ok(false);
    };
    let holding = of(store, window)?;
    if !holding.is_some_and(|h| h.active && h.task_id == task && h.first_brief == first_brief) {
        return Ok(false);
    }
    Ok(store
        .db
        .prepare(
            "SELECT 1 FROM task WHERE id = ?1 AND assignee_id = ?2 AND deleted_at IS NULL
           AND state NOT IN ('accepted','cancelled','failed')",
        )?
        .exists(params![task, window])?)
}

/// A successful handover, or proof of delivery, is the point a brief becomes
/// the one in the window. Same-task handovers continue the same holding.
pub(crate) fn hand_brief(store: &mut Store, message: i64) -> Result<(), LedgerError> {
    let brief: Option<(i64, i64)> = store
        .db
        .query_row(
            "SELECT m.recipient_id, m.task_id FROM message m
         JOIN task t ON t.id = m.task_id AND t.project_id = m.project_id
         JOIN participant p ON p.id = m.recipient_id AND p.project_id = m.project_id
         WHERE m.id = ? AND m.kind = 'task' AND t.assignee_id = m.recipient_id
           AND p.role NOT IN ('chief', 'human') AND p.left_at IS NULL AND t.deleted_at IS NULL
           AND t.state NOT IN ('accepted', 'cancelled', 'failed')",
            [message],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((window, task)) = brief else {
        return Ok(());
    };
    if let Some(previous) = of(store, window)? {
        if previous.active && previous.task_id != task {
            let number: i64 =
                store
                    .db
                    .query_row("SELECT number FROM task WHERE id = ?", [task], |r| r.get(0))?;
            let place = format!(
                "{} This window was given T-{number}.",
                task_place(store, Some(previous.task_id))?
            );
            close(store, window, Some(previous.task_id), Some(&place))?;
        }
    }
    // Save the prior holding before admitting a new brief. A refusal can put
    // this exact state back without reading or reconstructing event history.
    store.db.execute(
        "INSERT INTO question_handover (brief_id, participant_id, run_id, previous_task_id,
             previous_first_brief_id, previous_last_brief_id, previous_closed, previous_run_id)
         SELECT ?1, ?2, ?3, h.task_id, h.first_brief_id, h.last_brief_id, h.closed, h.run_id
         FROM (SELECT 1) seed LEFT JOIN question_holding h ON h.participant_id = ?2 WHERE 1
         ON CONFLICT(brief_id) DO UPDATE SET participant_id=excluded.participant_id, run_id=excluded.run_id,
           previous_task_id=excluded.previous_task_id, previous_first_brief_id=excluded.previous_first_brief_id,
           previous_last_brief_id=excluded.previous_last_brief_id, previous_closed=excluded.previous_closed,
           previous_run_id=excluded.previous_run_id WHERE question_handover.run_id != excluded.run_id",
        params![message, window, store.question_run],
    )?;
    store.db.execute(
        "INSERT INTO question_holding (participant_id, task_id, first_brief_id, last_brief_id, closed, run_id)
         VALUES (?1, ?2, ?3, ?3, 0, ?4)
         ON CONFLICT(participant_id) DO UPDATE SET
           first_brief_id = CASE WHEN question_holding.closed = 0 AND question_holding.task_id = excluded.task_id AND question_holding.run_id = excluded.run_id
                                 THEN question_holding.first_brief_id ELSE excluded.first_brief_id END,
           task_id = excluded.task_id, last_brief_id = excluded.last_brief_id, closed = 0, run_id = excluded.run_id",
        params![window, task, message, store.question_run],
    )?;
    Ok(())
}

/// A confirmed brief needs no rollback checkpoint.
pub(crate) fn confirm_brief(store: &Store, brief: i64) -> Result<(), LedgerError> {
    store
        .db
        .execute("DELETE FROM question_handover WHERE brief_id = ?", [brief])?;
    Ok(())
}

/// A handover that proved refused was not a delivered brief. Restore the
/// exact earlier holding; questions of the rejected new holding close first.
pub(crate) fn retract_brief(store: &mut Store, brief: i64) -> Result<(), LedgerError> {
    type Previous = (
        i64,
        String,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<String>,
    );
    let previous: Option<Previous> = store.db.query_row(
        "SELECT participant_id, run_id, previous_task_id, previous_first_brief_id,
                previous_last_brief_id, previous_closed, previous_run_id FROM question_handover WHERE brief_id = ?",
        [brief], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
    ).optional()?;
    let Some((window, run, task, first, last, closed, previous_run)) = previous else {
        return Ok(());
    };
    let current = of(store, window)?;
    if let Some(current) =
        current.filter(|h| run == store.question_run && h.last_brief == brief && h.active)
    {
        let same = task == Some(current.task_id)
            && first == Some(current.first_brief)
            && closed == Some(0)
            && previous_run.as_deref() == Some(run.as_str());
        if !same {
            close(store, window, Some(current.task_id), None)?;
        }
        if let (Some(task), Some(first), Some(last), Some(closed), Some(previous_run)) =
            (task, first, last, closed, previous_run)
        {
            store.db.execute("UPDATE question_holding SET task_id = ?1, first_brief_id = ?2, last_brief_id = ?3, closed = ?4, run_id = ?5 WHERE participant_id = ?6",
                params![task, first, last, closed, previous_run, window])?;
        } else {
            store.db.execute(
                "DELETE FROM question_holding WHERE participant_id = ?",
                [window],
            )?;
        }
    }
    confirm_brief(store, brief)
}

/// Bind every new ordinary member question to its window's actual holding.
pub(crate) fn bind(store: &Store, question: i64, window: i64) -> Result<(), LedgerError> {
    // A question may discuss a different task. Its delivery still belongs to
    // the task actually handed to the asking window, not the topic it names.
    let holding = of(store, window)?;
    let (task, first) = match holding {
        Some(h) if h.active && matches(store, window, Some(h.task_id), Some(h.first_brief))? => {
            (Some(h.task_id), Some(h.first_brief))
        }
        _ => (None, None),
    };
    store.db.execute("INSERT INTO question_binding (question_id, participant_id, task_id, first_brief_id, run_id) VALUES (?1, ?2, ?3, ?4, ?5)", params![question, window, task, first, store.question_run])?;
    Ok(())
}

/// None means a pre-tracking question; Some(None) is a taskless holding.
pub(crate) fn binding_task(
    store: &Store,
    question: i64,
) -> Result<Option<Option<i64>>, LedgerError> {
    Ok(store
        .db
        .query_row(
            "SELECT task_id FROM question_binding WHERE question_id = ?",
            [question],
            |r| r.get(0),
        )
        .optional()?)
}

/// The held task of a tracked question, or the topic for an untracked one.
/// A tracked idle question has no held task even if it mentions a topic.
pub(crate) fn task_for_question_sql(question: &str) -> String {
    format!(
        "CASE WHEN EXISTS (SELECT 1 FROM question_binding b WHERE b.question_id = {question}.id) \
         THEN (SELECT b.task_id FROM question_binding b WHERE b.question_id = {question}.id) \
         ELSE {question}.task_id END"
    )
}

pub(crate) fn task_of_question(
    store: &Store,
    question: i64,
) -> Result<Option<crate::views::TaskRow>, LedgerError> {
    let found: Option<(i64, i64)> = store
        .db
        .query_row(
            "SELECT t.project_id, t.number FROM question_binding b
         JOIN message q ON q.id = b.question_id AND q.sender_id = b.participant_id
         JOIN participant p ON p.id = b.participant_id AND p.project_id = q.project_id
         JOIN task t ON t.id = b.task_id AND t.project_id = q.project_id
         WHERE b.question_id = ?",
            [question],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    found
        .map(|(project, number)| store.task_row(project, number))
        .transpose()
}

/// None is a question from before tracking began; no inferred repair is made.
pub(crate) fn question_is_live(store: &Store, question: i64) -> Result<Option<bool>, LedgerError> {
    let binding: Option<(i64, Option<i64>, Option<i64>, i64, String, bool)> = store.db.query_row(
        "SELECT b.participant_id, b.task_id, b.first_brief_id, b.closed, b.run_id,
          EXISTS(SELECT 1 FROM message q
            JOIN participant p ON p.id = b.participant_id AND p.project_id = q.project_id
            JOIN participant recipient ON recipient.id = q.recipient_id AND recipient.project_id = q.project_id
            WHERE q.id = b.question_id AND q.kind = 'question' AND q.sender_id = b.participant_id
              AND (q.task_id IS NULL OR EXISTS (SELECT 1 FROM task t WHERE t.id = q.task_id AND t.project_id = q.project_id))
              AND (b.task_id IS NULL OR EXISTS (SELECT 1 FROM task t WHERE t.id = b.task_id AND t.project_id = q.project_id))
              AND ((b.task_id IS NULL AND b.first_brief_id IS NULL) OR (b.task_id IS NOT NULL AND b.first_brief_id IS NOT NULL)))
         FROM question_binding b WHERE b.question_id = ?",
        [question], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    ).optional()?;
    binding
        .map(|(window, task, first, closed, run, valid)| {
            if !valid {
                return Err(LedgerError::refused_with(
                    "question-conflict",
                    "the question's project, task or window records disagree; no answer was moved",
                    409,
                ));
            }
            Ok(closed == 0 && run == store.question_run && matches(store, window, task, first)?)
        })
        .transpose()
}

/// Return one undelivered, approved answer. State and notice are one write, so
/// retries and later closes cannot report the same answer twice.
pub(crate) fn return_answer(
    store: &mut Store,
    answer: i64,
    place: &str,
) -> Result<bool, LedgerError> {
    let row: Option<(i64, i64, i64, String, Option<i64>)> = store
        .db
        .query_row(
            "SELECT a.project_id, a.sender_id, a.reply_to, a.body,
                (SELECT number FROM task WHERE id = a.task_id AND project_id = a.project_id) FROM message a
         JOIN message q ON q.id = a.reply_to AND q.project_id = a.project_id
         JOIN question_binding b ON b.question_id = q.id
         JOIN participant receiver ON receiver.id = b.participant_id AND receiver.project_id = q.project_id
         JOIN participant author ON author.id = a.sender_id AND author.project_id = q.project_id
         WHERE a.id = ? AND a.kind = 'answer' AND a.state IN ('queued', 'delivering')
           AND q.sender_id = b.participant_id AND a.task_id IS b.task_id
           AND (q.task_id IS NULL OR EXISTS (SELECT 1 FROM task t WHERE t.id = q.task_id AND t.project_id = q.project_id))
           AND (b.task_id IS NULL OR EXISTS (SELECT 1 FROM task t WHERE t.id = b.task_id AND t.project_id = q.project_id))
           AND a.recipient_id = b.participant_id AND a.sender_id = q.recipient_id
           AND COALESCE(json_extract(a.receipt, '$.readEarly'), '') = ''",
            [answer],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((project, author, question, words, task)) = row else {
        let pending = store.db.prepare(
            "SELECT 1 FROM message WHERE id = ? AND kind = 'answer' AND state IN ('queued', 'delivering')
             AND COALESCE(json_extract(receipt, '$.readEarly'), '') = ''"
        )?.exists([answer])?;
        if pending {
            return Err(LedgerError::refused_with(
                "question-conflict",
                "the answer's project, author or window records disagree; its words were not moved",
                409,
            ));
        }
        return Ok(false);
    };
    let mut recipient = store.participant_row(author)?;
    if recipient.role == "chief" && recipient.left_at.is_some() {
        let current: Option<i64> = store.db.query_row(
            "SELECT id FROM participant WHERE project_id = ? AND role = 'chief' AND left_at IS NULL ORDER BY id DESC LIMIT 1",
            [project], |row| row.get(0),
        ).optional()?;
        let Some(current) = current else {
            return Err(LedgerError::refused_with(
                "chief-gone",
                "the answer cannot be returned until this project has a chief",
                409,
            ));
        };
        recipient = store.participant_row(current)?;
    }
    let body = format!("Your answer m-{answer} to m-{question} did not reach its window. {place}\n\nYour words:\n{words}");
    let notice = queue(
        store,
        project,
        &Queued {
            to: recipient.id,
            kind: "note",
            body: &body,
            urgent: recipient.role != "chief",
            ..Queued::default()
        },
    )?;
    store.db.execute(
        "UPDATE message SET state = 'cancelled', reason = ?1 WHERE id = ?2",
        params![format!("returned in m-{notice}"), answer],
    )?;
    let mut event = json!({"message":answer,"notice":notice});
    if let Some(task) = task {
        event["task"] = json!(task);
    }
    store.log(project, "message.returned", event)?;
    Ok(true)
}

pub(crate) fn task_place(store: &Store, task: Option<i64>) -> Result<String, LedgerError> {
    let Some(task) = task else {
        return Ok("Its window has ended.".into());
    };
    let (project, number): (i64, i64) = store.db.query_row(
        "SELECT project_id, number FROM task WHERE id = ?",
        [task],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let task = store.task_row(project, number)?;
    Ok(format!(
        "T-{number} {}.",
        crate::messages::moved_task_location(store, &task)?
    ))
}

/// Close exactly one session's current holding, and its taskless questions
/// when the window itself ends. Gated answers stay gated until approval.
pub(crate) fn close(
    store: &mut Store,
    window: i64,
    task: Option<i64>,
    where_now: Option<&str>,
) -> Result<(), LedgerError> {
    close_deferring(store, window, task, where_now, None).map(|_| ())
}

/// Close the door now, but allow the engine to settle the one paste that was
/// in flight as a window exited before it decides whether to return its words.
pub(crate) fn close_deferring(
    store: &mut Store,
    window: i64,
    task: Option<i64>,
    where_now: Option<&str>,
    in_flight: Option<i64>,
) -> Result<Vec<i64>, LedgerError> {
    let holding = of(store, window)?;
    let first = match (task, holding) {
        (Some(task), Some(h)) if h.task_id == task && h.active => Some(h.first_brief),
        (Some(_), _) => return Ok(Vec::new()),
        (None, _) => None,
    };
    let questions: Vec<i64> = store
        .db
        .prepare(
            "SELECT question_id FROM question_binding WHERE participant_id = ?1 AND task_id IS ?2
           AND first_brief_id IS ?3 AND closed = 0 ORDER BY question_id",
        )?
        .query_map(params![window, task, first], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let location = match where_now {
        Some(text) => text.to_owned(),
        None => task_place(store, task)?,
    };
    let mut deferred = Vec::new();
    for question in questions {
        deferred.extend(close_question_deferring(
            store, question, &location, in_flight,
        )?);
    }
    if task.is_some() {
        store.db.execute(
            "UPDATE question_holding SET closed = 1 WHERE participant_id = ?",
            [window],
        )?;
    }
    Ok(deferred)
}

/// Close a bound question and return each approved answer that never arrived.
/// This also settles a question whose old engine died before it could close it.
pub(crate) fn close_question(
    store: &mut Store,
    question: i64,
    location: &str,
) -> Result<(), LedgerError> {
    close_question_deferring(store, question, location, None).map(|_| ())
}

fn close_question_deferring(
    store: &mut Store,
    question: i64,
    location: &str,
    in_flight: Option<i64>,
) -> Result<Vec<i64>, LedgerError> {
    let exists = store
        .db
        .prepare(
            "SELECT 1 FROM question_binding b JOIN message q ON q.id = b.question_id
         JOIN participant p ON p.id = b.participant_id AND p.project_id = q.project_id
         WHERE b.question_id = ?1 AND q.kind = 'question' AND q.sender_id = b.participant_id
           AND (q.task_id IS NULL OR EXISTS (SELECT 1 FROM task t WHERE t.id = q.task_id AND t.project_id = q.project_id))
           AND (b.task_id IS NULL OR EXISTS (SELECT 1 FROM task t WHERE t.id = b.task_id AND t.project_id = q.project_id))",
        )?
        .exists([question])?;
    if !exists {
        return Err(LedgerError::refused_with(
            "question-conflict",
            "the question's project or window records disagree; its words were not moved",
            409,
        ));
    }
    store.db.execute(
        "UPDATE question_binding SET closed = 1 WHERE question_id = ?",
        [question],
    )?;
    let at = store.at();
    store.db.execute(
        "UPDATE message SET
           state = CASE WHEN EXISTS (SELECT 1 FROM message a WHERE a.reply_to = ?3 AND a.kind = 'answer' AND a.state IN ('delivered', 'read')) THEN state ELSE 'cancelled' END,
           reason = CASE WHEN EXISTS (SELECT 1 FROM message a WHERE a.reply_to = ?3 AND a.kind = 'answer' AND a.state IN ('delivered', 'read')) THEN reason ELSE ?1 END,
           door_closed_at = COALESCE(door_closed_at, ?2) WHERE id = ?3",
        params![crate::messages::MOVED_QUESTION, at, question],
    )?;
    let answers: Vec<(i64, Option<i64>)> = store.db.prepare("SELECT id, carried_by FROM message WHERE reply_to = ? AND kind = 'answer' AND state IN ('queued','delivering') ORDER BY id")?.query_map([question], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    let mut deferred = Vec::new();
    for (answer, carrier) in answers {
        if in_flight.is_some_and(|id| id == answer || Some(id) == carrier) {
            deferred.push(answer);
        } else {
            return_answer(store, answer, location)?;
        }
    }
    Ok(deferred)
}

pub(crate) fn answer_place(store: &Store, answer: i64) -> Result<String, LedgerError> {
    let task: Option<i64> =
        store
            .db
            .query_row("SELECT task_id FROM message WHERE id = ?", [answer], |r| {
                r.get(0)
            })?;
    task_place(store, task)
}

/// Close the current holding of a task, if the task has a window.
pub(crate) fn close_task(
    store: &mut Store,
    task: &crate::views::TaskRow,
    where_now: Option<&str>,
) -> Result<(), LedgerError> {
    if let Some(window) = task.assignee_id {
        close(store, window, Some(task.id), where_now)?;
        // A move after a window ended also ends the right to resume that
        // window's former holding, even if this session is later used again.
        store.db.execute(
            "UPDATE question_holding SET closed = 1 WHERE participant_id = ?1 AND task_id = ?2 AND closed = 2",
            params![window, task.id],
        )?;
    }
    Ok(())
}
