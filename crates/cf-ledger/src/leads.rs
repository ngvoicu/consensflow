//! A lead: a second coordinator of the chief's staff (design `leads`, contracts section 1).
//!
//! This is the ledger's side only. A lead's participant row has `role = 'lead'`, no roles, no
//! tier, no member and no backup; `lead` holds what it leads and whether it works with its own
//! hands. [`add_lead`] is reached by no route, command or page operation: the tests of the
//! steps that follow make a lead through it, and the step that gives a person the page's
//! operation is the one that calls it from outside this crate.

use cf_proto::ledger::ParticipantView;
use rusqlite::params;
use serde_json::json;

use crate::model::{self, LedgerError};
use crate::staff::{add_participant, NewParticipant};
use crate::store::Store;
use crate::views::participant_view;

/// How many leads a project has at once.
pub const MOST_LEADS: i64 = 3;

/// A lead to add: the harness, model and effort it runs on, as a chief's are, what it leads and
/// whether it works with its own hands.
#[derive(Debug, Clone, PartialEq)]
pub struct NewLead {
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// What its harness's list calls the model; held with the model only.
    pub model_name: Option<String>,
    pub scope: String,
    pub own_hands: bool,
}

/// The lowest `lead-N` (N from 1) that was never any participant's handle in the project,
/// present or departed, and is none of the names a participant went by before its own.
fn next_handle(store: &Store, project: i64) -> Result<String, LedgerError> {
    for n in 1.. {
        let handle = format!("lead-{n}");
        let used: bool = store.db.query_row(
            "SELECT EXISTS (
               SELECT 1 FROM participant p
               WHERE p.project_id = ?1
                 AND (p.handle = ?2
                      OR EXISTS (SELECT 1 FROM json_each(p.former_handles) f WHERE f.value = ?2)))",
            params![project, handle],
            |row| row.get(0),
        )?;
        if !used {
            return Ok(handle);
        }
    }
    unreachable!("a project has fewer handles than numbers")
}

/// Adds a lead to a project: its handle is chosen in the write that checks the limit.
pub(crate) fn add_lead(
    store: &mut Store,
    project: i64,
    lead: &NewLead,
) -> Result<ParticipantView, LedgerError> {
    let harness = model::require_harness(&lead.harness)?;
    if lead.scope.trim().is_empty() {
        return Err(LedgerError::refused(
            "blank-scope",
            "a lead needs its scope: what it leads",
        ));
    }
    store.write(|store| {
        store.project_row(project)?;
        let present: i64 = store.db.query_row(
            "SELECT COUNT(*) FROM participant WHERE project_id = ? AND role = 'lead' AND left_at IS NULL",
            [project],
            |row| row.get(0),
        )?;
        if present >= MOST_LEADS {
            return Err(LedgerError::refused_with(
                "too-many-leads",
                format!("a project has at most {MOST_LEADS} leads: remove one first"),
                409,
            ));
        }
        let handle = next_handle(store, project)?;
        let added = add_participant(
            store,
            project,
            NewParticipant {
                handle: &handle,
                role: Some("lead"),
                roles: &[],
                harness: Some(harness),
                model: lead.model.as_deref(),
                effort: lead.effort.as_deref(),
                // A name goes with a model: none for the harness's own default.
                model_name: lead.model.as_ref().and(lead.model_name.as_deref()),
                designer: false,
                tier: None,
                backup_of: None,
            },
        )?;
        store.db.execute(
            "INSERT INTO lead (participant_id, scope, own_hands) VALUES (?, ?, ?)",
            params![added.id, lead.scope, i64::from(lead.own_hands)],
        )?;
        store.log(
            project,
            "lead.added",
            json!({
                "handle": handle,
                "harness": harness,
                "scope": lead.scope,
                "ownHands": lead.own_hands,
            }),
        )?;
        participant_view(&store.participant_row(added.id)?)
    })
}
