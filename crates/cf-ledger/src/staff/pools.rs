//! Who may take a task: the active members of a pool and tier, with what
//! the daemon ranks them by. The tier a task goes to comes with the tasks.

use cf_proto::ledger::{Candidate, MemberView};
use rusqlite::params;

use crate::model::{sql_list, LedgerError, HELD_TASK_STATES};
use crate::store::Store;

/// The active members an open task may go to, with what the daemon ranks them by.
pub(crate) fn candidates(
    store: &Store,
    project_id: i64,
    number: i64,
) -> Result<Vec<Candidate>, LedgerError> {
    let task = store.task_row(project_id, number)?;
    Ok(members(store, project_id, task.pool.as_deref())?
        .into_iter()
        .filter(|member| task.tier.is_none() || member.tier == task.tier)
        .map(|member| Candidate {
            had_it: Some(member.id) == task.taken_from_id,
            member,
        })
        .collect())
}

/// The active members of one role, in join order, with what the daemon
/// ranks them by: their tier, how many tasks they have taken, whether one
/// is on their hands now, and until when they are out of quota. A task with
/// no pool asks for none.
pub(crate) fn members(
    store: &Store,
    project_id: i64,
    role: Option<&str>,
) -> Result<Vec<MemberView>, LedgerError> {
    let held = sql_list(&HELD_TASK_STATES);
    let rows = store
        .db
        .prepare(&format!(
            "SELECT p.*,
              (SELECT COUNT(*) FROM task t JOIN participant s ON s.id = t.assignee_id
                WHERE s.id = p.id OR s.member_id = p.id) AS taken,
              (SELECT COUNT(*) FROM participant s
                WHERE (s.id = p.id OR s.member_id = p.id) AND s.left_at IS NULL
                  AND EXISTS (SELECT 1 FROM task WHERE assignee_id = s.id AND state IN ({held}))
              ) AS sessions
       FROM participant p
       WHERE p.project_id = ? AND p.left_at IS NULL AND p.member_id IS NULL
         AND EXISTS (SELECT 1 FROM json_each(p.roles) r WHERE r.value = ?)
       ORDER BY p.id"
        ))?
        .query_map(params![project_id, role], |row| {
            Ok((
                row.get::<_, String>("roles")?,
                MemberView {
                    id: row.get("id")?,
                    handle: row.get("handle")?,
                    agent: row.get("agent")?,
                    harness: row.get("harness")?,
                    tier: row.get("tier")?,
                    roles: Vec::new(),
                    taken: row.get("taken")?,
                    sessions: row.get("sessions")?,
                    out_until: row.get("out_until")?,
                },
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(roles, member)| {
            Ok(MemberView {
                roles: serde_json::from_str(&roles)?,
                ..member
            })
        })
        .collect()
}
