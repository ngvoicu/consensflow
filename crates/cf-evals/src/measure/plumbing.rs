//! The board's plumbing, counted from the ledger whatever the chief decided:
//! briefs delivered to members, results delivered back to the chief, questions
//! members put to the chief and the answers delivered back, tasks accepted, and
//! the controls the chief used. Did it hold: every brief delivered, every result
//! back to the chief, every member's question answered and the answer delivered,
//! every task shown on the board.

use rusqlite::{Connection, Params};

use super::metrics::{Check, MemberQuestions, Plumbing};
use crate::Result;

/// Whether a question `q` is owed an answer by the end of the run: it has one that
/// stands, or its task is not over. A question on an accepted, cancelled or failed
/// task is not open, the rule the chief's handoff lists by (`chief_open_work`).
const OWED: &str = "(EXISTS (SELECT 1 FROM message a
                              WHERE a.reply_to = q.id AND a.kind = 'answer' AND a.state != 'cancelled')
                  OR NOT EXISTS (SELECT 1 FROM task t
                                 WHERE t.id = q.task_id AND t.state IN ('accepted', 'cancelled', 'failed')))";

/// Whether a tell `q` (the chief's urgent question to a task's window) is owed an
/// answer by the end of the run: it was answered, or its task was not later
/// cancelled or failed, whose window was stopped or gone. A task accepted after the
/// tell still owes it: its window went on, and the answer was the tell's point.
const TELL_OWED: &str = "(EXISTS (SELECT 1 FROM message a
                              WHERE a.reply_to = q.id AND a.kind = 'answer' AND a.state != 'failed')
                  OR NOT EXISTS (SELECT 1 FROM task t
                                 WHERE t.id = q.task_id AND t.state IN ('cancelled', 'failed')))";

/// How many rows `sql` counts (`COUNT(*)`), with its parameters.
fn count(db: &Connection, sql: &str, params: impl Params) -> Result<i64> {
    Ok(db.query_row(sql, params, |row| row.get(0))?)
}

/// The plumbing of the ledger: `chief` and `human` are the ids of its participants.
pub(super) fn plumbing(db: &Connection, chief: i64, human: i64) -> Result<Plumbing> {
    let sent = rusqlite::params![chief, human];
    Ok(Plumbing {
        // A brief withdrawn with its cancelled task was rightly never delivered.
        briefs: count(
            db,
            "SELECT COUNT(*) FROM message WHERE kind = 'task' AND state != 'cancelled'",
            [],
        )?,
        briefs_delivered: count(
            db,
            "SELECT COUNT(*) FROM message WHERE kind = 'task' AND state IN ('delivered', 'read')",
            [],
        )?,
        // A result withdrawn was rightly never delivered: the chief accepted or sent
        // back its task before it was given the result (the native daemon withdraws it
        // then), or the task was called off.
        results: count(
            db,
            "SELECT COUNT(*) FROM message WHERE kind = 'result' AND recipient_id = ?1 AND state != 'cancelled'",
            [chief],
        )?,
        results_delivered: count(
            db,
            "SELECT COUNT(*) FROM message WHERE kind = 'result' AND recipient_id = ?1 AND state IN ('delivered', 'read')",
            [chief],
        )?,
        // A question on a task that is over is not open (`chief_open_work`), so the chief
        // is not owed to have answered it, unless it was answered for good: an answer the
        // cancel of its task took back was rightly never delivered, as a result was not.
        member_questions: count(
            db,
            &format!(
                "SELECT COUNT(*) FROM message q
                 WHERE q.kind = 'question' AND q.recipient_id = ?1
                   AND q.sender_id IS NOT NULL AND q.sender_id != ?1 AND q.sender_id != ?2
                   AND {OWED}"
            ),
            sent,
        )?,
        member_questions_answered: count(
            db,
            "SELECT COUNT(*) FROM message q
             WHERE q.kind = 'question' AND q.recipient_id = ?1
               AND q.sender_id IS NOT NULL AND q.sender_id != ?1 AND q.sender_id != ?2
               AND EXISTS (SELECT 1 FROM message a
                           WHERE a.reply_to = q.id AND a.kind = 'answer' AND a.state != 'cancelled')",
            sent,
        )?,
        answers_delivered: count(
            db,
            "SELECT COUNT(*) FROM message a JOIN message q ON q.id = a.reply_to
             WHERE a.kind = 'answer' AND a.state IN ('delivered', 'read')
               AND q.kind = 'question' AND q.recipient_id = ?1
               AND q.sender_id IS NOT NULL AND q.sender_id != ?1 AND q.sender_id != ?2",
            sent,
        )?,
        accepted: count(db, "SELECT COUNT(*) FROM task WHERE state = 'accepted'", [])?,
        // A tell the chief's own cancel took back is not one owed an answer; an answer a
        // cancel took back was still given (both daemons withdraw what is queued for a
        // cancelled task alike: crates/cf-ledger/tests/cancelled.rs). Nor is a tell on a
        // task that was later cancelled or failed ([`TELL_OWED`]).
        tells: count(
            db,
            &format!(
                "SELECT COUNT(*) FROM message q
                 WHERE q.kind = 'question' AND q.urgent = 1 AND q.sender_id = ?1
                   AND q.state != 'cancelled' AND {TELL_OWED}"
            ),
            [chief],
        )?,
        tells_answered: count(
            db,
            "SELECT COUNT(*) FROM message q
             WHERE q.kind = 'question' AND q.urgent = 1 AND q.sender_id = ?1 AND q.state != 'cancelled'
               AND EXISTS (SELECT 1 FROM message a
                           WHERE a.reply_to = q.id AND a.kind = 'answer' AND a.state != 'failed')",
            [chief],
        )?,
        // The tasks the board places: every one the human has not deleted. A lane has the
        // task of its participant (or of its member, once the session ended); every other,
        // whatever its state (waiting for a member, paused or called off before any had
        // it, a removed member's), is among the open ones, on both daemons alike
        // (crates/cf-e2e/tests/rig/core_board.rs).
        placed: count(db, "SELECT COUNT(*) FROM task WHERE deleted_at IS NULL", [])?,
        pauses: count(
            db,
            "SELECT COUNT(*) FROM event
             WHERE kind = 'task.state' AND json_extract(data, '$.to') = 'paused'",
            [],
        )?,
        resumes: count(
            db,
            "SELECT COUNT(*) FROM event
             WHERE kind = 'task.state' AND json_extract(data, '$.from') = 'paused'
               AND json_extract(data, '$.to') IN ('queued', 'working')",
            [],
        )?,
        // A task given straight to a session's window: only `--after` does that.
        continuations: count(
            db,
            "SELECT COUNT(*) FROM event e JOIN participant p
               ON p.project_id = e.project_id AND p.handle = json_extract(e.data, '$.to')
             WHERE e.kind = 'task.created' AND p.member_id IS NOT NULL",
            [],
        )?,
    })
}

/// The questions members put to the chief, by the asker's role and harness: how
/// many it was owed an answer to ([`OWED`]), how many the chief answered, how
/// many answers reached the member's window, and the longest question in
/// characters (of all of them).
pub(super) fn member_questions_by(
    db: &Connection,
    chief: i64,
    human: i64,
) -> Result<Vec<MemberQuestions>> {
    let mut statement = db.prepare(
        &format!("SELECT p.role, p.harness, SUM({OWED}) AS asked,
           SUM(EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer' AND a.state != 'cancelled')) AS answered,
           SUM(EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer' AND a.state IN ('delivered', 'read'))) AS delivered,
           MAX(LENGTH(q.body)) AS longest
         FROM message q JOIN participant p ON p.id = q.sender_id
         WHERE q.kind = 'question' AND q.recipient_id = ?1 AND q.sender_id NOT IN (?1, ?2)
         GROUP BY p.role, p.harness ORDER BY p.role, p.harness"),
    )?;
    let asked = statement
        .query_map([chief, human], |row| {
            Ok(MemberQuestions {
                role: row.get(0)?,
                harness: row.get(1)?,
                asked: row.get(2)?,
                answered: row.get(3)?,
                delivered: row.get(4)?,
                longest: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(asked)
}

/// Did the board's plumbing hold, whatever the chief decided: every brief
/// delivered, every result back to the chief, every member's question answered
/// and the answer delivered, every task shown on the board (`board_tasks`: how
/// many the board listed when the run ended).
pub fn mechanics(plumbing: &Plumbing, board_tasks: i64) -> Vec<Check> {
    let held = |name: &str, got: i64, of: i64| Check {
        name: format!("{name} ({got}/{of})"),
        ok: got == of,
    };
    vec![
        held(
            "every task brief was delivered",
            plumbing.briefs_delivered,
            plumbing.briefs,
        ),
        held(
            "every result reached the chief",
            plumbing.results_delivered,
            plumbing.results,
        ),
        held(
            "every question a member asked the chief was answered",
            plumbing.member_questions_answered,
            plumbing.member_questions,
        ),
        held(
            "every answer reached the member",
            plumbing.answers_delivered,
            plumbing.member_questions_answered,
        ),
        held(
            "every tell the chief sent was answered",
            plumbing.tells_answered,
            plumbing.tells,
        ),
        held("the board showed every task", board_tasks, plumbing.placed),
    ]
}
