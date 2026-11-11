//! The rounds of the chief's tasks, for a scenario that holds the chief to what it did
//! with a result: when a task was asked for and with what brief, when each result came
//! in, each time the chief sent it back and with what words, and when it was accepted.
//! Every number is SQL over the tables as they are, like the rest of the measure.
//!
//! A send-back is the move of a finished task (`done`, or `failed`) back to `queued`:
//! the ledger logs it as a `task.state` event whose `message` is the task message that
//! carries the chief's words. A move from `paused` is a resume, and no send-back.

use rusqlite::Connection;

use super::board::ROLE_OF_TASK;
use super::metrics::{Reopen, Rounds};
use crate::Result;

/// The rounds of every task the participant `chief` of `project` asked for, by number.
pub(super) fn rounds(db: &Connection, project: i64, chief: &str) -> Result<Vec<Rounds>> {
    let mut tasks = db.prepare(&format!(
        "SELECT t.id, t.number, t.pool, t.state, t.created_at, t.body, {ROLE_OF_TASK} AS role
         FROM task t JOIN participant q ON q.id = t.requester_id
         LEFT JOIN participant a ON a.id = t.assignee_id
         WHERE t.project_id = ?1 AND q.handle = ?2 ORDER BY t.number"
    ))?;
    let mut results = db.prepare(
        "SELECT created_at FROM message
         WHERE task_id = ?1 AND kind = 'result' AND state != 'cancelled' ORDER BY id",
    )?;
    let mut reopens = db.prepare(
        "SELECT e.at, m.body
         FROM event e JOIN message m ON m.id = json_extract(e.data, '$.message')
         WHERE e.project_id = ?1 AND e.kind = 'task.state'
           AND json_extract(e.data, '$.task') = ?2
           AND json_extract(e.data, '$.from') IN ('done', 'failed')
           AND json_extract(e.data, '$.to') = 'queued'
         ORDER BY e.id",
    )?;
    let mut accepted = db.prepare(
        "SELECT at FROM event
         WHERE project_id = ?1 AND kind = 'task.state'
           AND json_extract(data, '$.task') = ?2 AND json_extract(data, '$.to') = 'accepted'
         ORDER BY id DESC LIMIT 1",
    )?;
    let asked = tasks
        .query_map(rusqlite::params![project, chief], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                Rounds {
                    number: row.get(1)?,
                    pool: row.get(2)?,
                    role: row.get(6)?,
                    state: row.get(3)?,
                    created_at: row.get(4)?,
                    brief: row.get(5)?,
                    ..Rounds::default()
                },
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    asked
        .into_iter()
        .map(|(id, mut task)| {
            task.results = results
                .query_map([id], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            task.reopens = reopens
                .query_map(rusqlite::params![project, task.number], |row| {
                    Ok(Reopen {
                        at: row.get(0)?,
                        body: row.get(1)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            task.accepted_at = accepted
                .query_map(rusqlite::params![project, task.number], |row| {
                    row.get::<_, String>(0)
                })?
                .next()
                .transpose()?;
            Ok(task)
        })
        .collect()
}
