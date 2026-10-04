//! How the ledger's rows read: the SELECTs that join a row to the handles it
//! refers to, and the views every operation hands back (`src/ledger/views.js`).

use cf_proto::ledger::ParticipantView;
use rusqlite::Row;

use crate::model::LedgerError;

/// A participant with its member's handle, when it is a member's session.
pub(crate) const PARTICIPANT_SELECT: &str = "
  SELECT p.*, m.handle AS member_handle
  FROM participant p
  LEFT JOIN participant m ON m.id = p.member_id";

/// A participant's row, as `PARTICIPANT_SELECT` reads it.
pub(crate) struct ParticipantRow {
    pub(crate) id: i64,
    pub(crate) project_id: i64,
    pub(crate) handle: String,
    pub(crate) role: String,
    pub(crate) roles: String,
    pub(crate) agent: Option<String>,
    pub(crate) harness: Option<String>,
    pub(crate) tier: Option<String>,
    pub(crate) member_id: Option<i64>,
    pub(crate) out_until: Option<String>,
    pub(crate) out_since: Option<String>,
    pub(crate) created_at: String,
    pub(crate) left_at: Option<String>,
    pub(crate) designer: i64,
    pub(crate) member_handle: Option<String>,
}

impl ParticipantRow {
    pub(crate) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            handle: row.get("handle")?,
            role: row.get("role")?,
            roles: row.get("roles")?,
            agent: row.get("agent")?,
            harness: row.get("harness")?,
            tier: row.get("tier")?,
            member_id: row.get("member_id")?,
            out_until: row.get("out_until")?,
            out_since: row.get("out_since")?,
            created_at: row.get("created_at")?,
            left_at: row.get("left_at")?,
            designer: row.get("designer")?,
            member_handle: row.get("member_handle")?,
        })
    }
}

pub(crate) fn participant_view(row: &ParticipantRow) -> Result<ParticipantView, LedgerError> {
    // A member handle that says something makes this a session: its own name
    // is what follows the member's handle and its hyphen.
    let session = row
        .member_handle
        .as_ref()
        .filter(|handle| !handle.is_empty())
        .map(|handle| {
            row.handle
                .chars()
                .skip(handle.chars().count() + 1)
                .collect()
        });
    Ok(ParticipantView {
        id: row.id,
        project_id: row.project_id,
        handle: row.handle.clone(),
        role: row.role.clone(),
        agent: row.agent.clone(),
        harness: row.harness.clone(),
        designer: row.designer == 1,
        created_at: row.created_at.clone(),
        left_at: row.left_at.clone(),
        tier: row.tier.clone(),
        roles: serde_json::from_str(&row.roles)?,
        out_until: row.out_until.clone(),
        out_since: row.out_since.clone(),
        member_id: row.member_id,
        member: row.member_handle.clone(),
        session,
    })
}
