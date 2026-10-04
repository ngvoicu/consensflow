//! A project's staff: members who join and leave, the sessions their tasks
//! run in, and until when they are out of quota (`src/ledger/staff.js`).
//! What the projects need of it is here: a participant's joining.

use cf_proto::ledger::ParticipantView;
use rusqlite::{params, OptionalExtension};
use serde_json::json;

use crate::model::LedgerError;
use crate::store::Store;
use crate::views::participant_view;

/// A participant joining a project: its handle, its role or its roles (the
/// first leading), the agent and harness it runs on, and its tier.
pub(crate) struct NewParticipant<'a> {
    pub(crate) handle: &'a str,
    pub(crate) role: Option<&'a str>,
    pub(crate) roles: &'a [&'static str],
    pub(crate) agent: Option<&'a str>,
    pub(crate) harness: Option<&'a str>,
    pub(crate) designer: bool,
    pub(crate) tier: Option<&'a str>,
}

impl<'a> NewParticipant<'a> {
    /// The human, or the chief: a participant whose handle is its role.
    pub(crate) fn coordinator(
        role: &'a str,
        agent: Option<&'a str>,
        harness: Option<&'a str>,
    ) -> Self {
        Self {
            handle: role,
            role: Some(role),
            roles: &[],
            agent,
            harness,
            designer: false,
            tier: None,
        }
    }
}

/// A participant joins under a handle nobody in the project has; a member's joining is logged.
pub(crate) fn add_participant(
    store: &mut Store,
    project_id: i64,
    joining: NewParticipant<'_>,
) -> Result<ParticipantView, LedgerError> {
    let role = joining
        .role
        .or_else(|| joining.roles.first().copied())
        .unwrap_or_default();
    store.project_row(project_id)?;
    let taken = store
        .db
        .query_row(
            "SELECT 1 FROM participant WHERE project_id = ? AND handle = ?",
            params![project_id, joining.handle],
            |_| Ok(()),
        )
        .optional()?;
    if taken.is_some() {
        return Err(LedgerError::refused_with(
            "member-exists",
            format!("{} is already in project {project_id}", joining.handle),
            409,
        ));
    }
    let at = store.at();
    store.db.execute(
        "INSERT INTO participant (project_id, handle, role, roles, agent, harness, designer, tier, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            project_id,
            joining.handle,
            role,
            json!(joining.roles).to_string(),
            joining.agent,
            joining.harness,
            i64::from(joining.designer),
            joining.tier,
            at,
        ],
    )?;
    let id = store.db.last_insert_rowid();
    if role != "human" && role != "chief" {
        store.log(
            project_id,
            "member.added",
            json!({ "handle": joining.handle, "role": role, "roles": joining.roles, "harness": joining.harness }),
        )?;
    }
    participant_view(&store.participant_row(id)?)
}
