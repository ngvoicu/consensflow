//! Who may take a task: the active members of a pool and tier, with what
//! the daemon ranks them by, and the tier a task for a pool goes to.

use cf_proto::ledger::{Candidate, MemberView};
use rusqlite::params;

use crate::model::{sql_list, LedgerError, HELD_TASK_STATES, TIERS};
use crate::store::Store;

/// The tier a task for `pool` goes to: the one asked, when somebody holds
/// it; else the nearest one somebody does, the next one up before the next
/// one down. A pool with no tier (the designer) keeps none.
pub(crate) fn nearest_tier(
    store: &Store,
    project_id: i64,
    pool: &str,
    tier: Option<&str>,
) -> Result<Option<String>, LedgerError> {
    let Some(asked) = tier else { return Ok(None) };
    if has_members_of_tier(store, project_id, pool, Some(asked))? {
        return Ok(Some(asked.to_string()));
    }
    let at = TIERS
        .iter()
        .position(|tier| *tier == asked)
        .unwrap_or_default();
    let mut near: Vec<(usize, &str)> = TIERS
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, other)| *other != asked)
        .collect();
    near.sort_by_key(|(index, _)| (index.abs_diff(at), *index));
    for (_, other) in near {
        if has_members_of_tier(store, project_id, pool, Some(other))? {
            return Ok(Some(other.to_string()));
        }
    }
    Ok(Some(asked.to_string()))
}

/// Whether the staff has members of one role and tier (any tier when none),
/// whatever role they were saved with first.
pub(crate) fn has_members_of_tier(
    store: &Store,
    project_id: i64,
    pool: &str,
    tier: Option<&str>,
) -> Result<bool, LedgerError> {
    let found = store
        .db
        .prepare(
            "SELECT * FROM participant p
       WHERE p.project_id = ? AND (? IS NULL OR p.tier = ?) AND p.left_at IS NULL AND p.member_id IS NULL
         AND EXISTS (SELECT 1 FROM json_each(p.roles) r WHERE r.value = ?)
       ORDER BY p.id",
        )?
        .exists(params![project_id, tier, tier, pool])?;
    Ok(found)
}

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
