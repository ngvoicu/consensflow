//! The size the human chooses for every member window of a project to compact at,
//! whatever its harness, in Project settings ("Compaction"), from [`COMPACTION_MIN`] to
//! [`COMPACTION_MAX`] tokens. None is each harness's own point, which every project has
//! until the human chooses a size. It is kept on the project's row (`project.compaction`,
//! which migration 0033 renamed from 0026's `claude_compaction`: it was Claude's alone
//! until Phase 7 of smaller-contexts) and logged when it changes (`project.compaction`,
//! with what it was and what it became). Only the page writes it: no route of the agents'
//! API reaches it, so no `cf` command, the chief's or a member's, changes it.
//!
//! It is the size as the human chose it: the ledger does no sum with any harness's own
//! margin. The engine gives every member's launch the size at launch and on resume, and a
//! chief's launch never; each harness's adapter gives it its harness by that harness's own
//! rule, and a size above a model's own window leaves the harness's own point.

use cf_proto::ledger::ProjectView;
use rusqlite::params;
use serde_json::{json, Value};

use super::known_project;
use crate::model::LedgerError;
use crate::store::Store;

/// The smallest size: the least Claude Code takes for its compaction window.
pub const COMPACTION_MIN: u32 = 100_000;

/// The largest size: the most Claude Code takes for its compaction window.
pub const COMPACTION_MAX: u32 = 1_000_000;

fn refused() -> LedgerError {
    LedgerError::refused(
        "invalid-compaction",
        format!(
            "the compaction size is a whole number of tokens from {COMPACTION_MIN} to {COMPACTION_MAX}, or none for each harness's own point"
        ),
    )
}

/// `tokens` as a size in the range, or the refusal.
fn size_of(tokens: u32) -> Result<u32, LedgerError> {
    (COMPACTION_MIN..=COMPACTION_MAX)
        .contains(&tokens)
        .then_some(tokens)
        .ok_or_else(refused)
}

/// `value` read as the size the members compact at: a whole
/// number of tokens in the range, or none (a missing field or `null`) for
/// each harness's own point.
pub fn parse_compaction(value: Option<&Value>) -> Result<Option<u32>, LedgerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let tokens = number.as_u64().ok_or_else(refused)?;
            size_of(u32::try_from(tokens).map_err(|_| refused())?).map(Some)
        }
        Some(_) => Err(refused()),
    }
}

/// Chooses the size the project's members compact at, or takes the choice
/// away, and logs it where it changed. The size is checked before the project is
/// looked for.
pub(crate) fn set_compaction(
    store: &mut Store,
    id: i64,
    window: Option<u32>,
) -> Result<ProjectView, LedgerError> {
    let to = window.map(size_of).transpose()?;
    store.write(|store| {
        let from = store.project_row(id)?.compaction;
        if from != to {
            let at = store.at();
            store.db.execute(
                "UPDATE project SET compaction = ?, updated_at = ? WHERE id = ?",
                params![to, at, id],
            )?;
            store.log(id, "project.compaction", json!({ "from": from, "to": to }))?;
        }
        known_project(store, id)
    })
}

#[cfg(test)]
mod tests;
