//! Who may take a task: the active members of a pool and tier, with what
//! the daemon ranks them by, and the tier a task for a pool goes to.

use cf_proto::ledger::MemberView;
use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;

use crate::model::{sql_list, LedgerError, HELD_TASK_STATES, TIERS};
use crate::store::Store;

/// The durable reason for an out mark. Old timed marks can have no known kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HoldKind {
    Quota,
    Account,
    Busy,
    Moved,
}

impl HoldKind {
    pub(crate) fn read(value: Option<&str>) -> Option<Self> {
        match value {
            Some("quota") => Some(Self::Quota),
            Some("account") => Some(Self::Account),
            Some("busy") => Some(Self::Busy),
            Some("moved") => Some(Self::Moved),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Quota => "quota",
            Self::Account => "account",
            Self::Busy => "busy",
            Self::Moved => "moved",
        }
    }
}

/// A primary member's backup, with its own availability and work count.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupCandidate {
    #[serde(flatten)]
    pub member: MemberView,
    pub had_it: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vacation_since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out_kind: Option<HoldKind>,
}

/// A primary candidate, and its backup if it has one.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    #[serde(flatten)]
    pub member: MemberView,
    pub had_it: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vacation_since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out_kind: Option<HoldKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<BackupCandidate>,
}

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
         AND p.backup_of IS NULL
         AND EXISTS (SELECT 1 FROM json_each(p.roles) r WHERE r.value = ?)
       ORDER BY p.id",
        )?
        .exists(params![project_id, tier, tier, pool])?;
    Ok(found)
}

/// The handles of the active members of `pool` at `tier`, in join order.
pub(crate) fn holders_of_tier(
    store: &Store,
    project_id: i64,
    pool: &str,
    tier: &str,
) -> Result<Vec<String>, LedgerError> {
    Ok(members(store, project_id, Some(pool))?
        .into_iter()
        .filter(|member| member.tier.as_deref() == Some(tier))
        .map(|member| member.handle)
        .collect())
}

/// The active members an open task may go to, with what the daemon ranks them by.
pub(crate) fn candidates(
    store: &Store,
    project_id: i64,
    number: i64,
) -> Result<Vec<Candidate>, LedgerError> {
    let task = store.task_row(project_id, number)?;
    members(store, project_id, task.pool.as_deref())?
        .into_iter()
        .filter(|member| task.tier.is_none() || member.tier == task.tier)
        .map(|member| {
            let row = store.participant_row(member.id)?;
            let backup = backup_of(store, member.id, task.taken_from_id)?;
            Ok(Candidate {
                had_it: Some(member.id) == task.taken_from_id,
                vacation_since: row.vacation_since,
                out_kind: HoldKind::read(row.out_kind.as_deref()),
                member,
                backup,
            })
        })
        .collect()
}

/// Read the backup by its owner, irrespective of a stale role or tier on it.
fn backup_of(
    store: &Store,
    owner: i64,
    taken_from: Option<i64>,
) -> Result<Option<BackupCandidate>, LedgerError> {
    let held = sql_list(&HELD_TASK_STATES);
    let result = store.db.query_row(
        &format!(
            "SELECT p.*,
              (SELECT COUNT(*) FROM task t JOIN participant s ON s.id = t.assignee_id
                WHERE s.id = p.id OR s.member_id = p.id) AS taken,
              (SELECT COUNT(*) FROM participant s
                WHERE (s.id = p.id OR s.member_id = p.id) AND s.left_at IS NULL
                  AND EXISTS (SELECT 1 FROM task WHERE assignee_id = s.id AND state IN ({held}))) AS sessions
             FROM participant p WHERE p.backup_of = ? AND p.left_at IS NULL"
        ),
        [owner],
        |row| {
            Ok((read_member(row)?, row.get::<_, Option<String>>("vacation_since")?, row.get::<_, Option<String>>("out_kind")?))
        },
    ).optional()?;
    result
        .map(|((member, roles), vacation_since, out_kind)| {
            Ok(BackupCandidate {
                had_it: Some(member.id) == taken_from,
                member: MemberView {
                    roles: serde_json::from_str(&roles)?,
                    ..member
                },
                vacation_since,
                out_kind: HoldKind::read(out_kind.as_deref()),
            })
        })
        .transpose()
}

fn read_member(row: &Row<'_>) -> rusqlite::Result<(MemberView, String)> {
    Ok((
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
            needs_edit: row.get::<_, i64>("needs_edit")? == 1,
        },
        row.get("roles")?,
    ))
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
       WHERE p.project_id = ? AND p.left_at IS NULL AND p.member_id IS NULL AND p.backup_of IS NULL
         AND EXISTS (SELECT 1 FROM json_each(p.roles) r WHERE r.value = ?)
       ORDER BY p.id"
        ))?
        .query_map(params![project_id, role], read_member)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(member, roles)| {
            Ok(MemberView {
                roles: serde_json::from_str(&roles)?,
                ..member
            })
        })
        .collect()
}
