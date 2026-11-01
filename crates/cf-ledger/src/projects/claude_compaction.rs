//! The size the human chooses for a project's Claude member windows to compact
//! at, in Project settings: the window Claude Code is given by each member's
//! launch (its `autoCompactWindow`, which fires a little before the window fills),
//! from [`CLAUDE_COMPACTION_MIN`] to [`CLAUDE_COMPACTION_MAX`] tokens, the range
//! Claude takes. None is Claude's own point, which every project has until the
//! human chooses a size. It is kept on the project's row and logged when it
//! changes (`project.claude_compaction`, with what it was and what it became).
//! Only the page writes it: no route of the agents' API reaches it, so no `cf`
//! command, the chief's or a member's, changes it.
//!
//! It is the size as Claude is given it: the ledger does no sum with Claude's own
//! margin, which is Claude's to change. The engine gives a member's launch the size
//! at launch and on resume, and a chief's launch never; only Claude's adapter reads it.

use cf_proto::ledger::ProjectView;
use rusqlite::params;
use serde_json::{json, Value};

use super::known_project;
use crate::model::LedgerError;
use crate::store::Store;

/// The smallest window Claude Code takes for its compaction.
pub const CLAUDE_COMPACTION_MIN: u32 = 100_000;

/// The largest window Claude Code takes for its compaction.
pub const CLAUDE_COMPACTION_MAX: u32 = 1_000_000;

fn refused() -> LedgerError {
    LedgerError::refused(
        "invalid-claude-compaction",
        format!(
            "the compaction window for Claude members is a whole number of tokens from {CLAUDE_COMPACTION_MIN} to {CLAUDE_COMPACTION_MAX}, or none for Claude's own point"
        ),
    )
}

/// `tokens` as a size Claude takes, or the refusal.
fn size_of(tokens: u32) -> Result<u32, LedgerError> {
    (CLAUDE_COMPACTION_MIN..=CLAUDE_COMPACTION_MAX)
        .contains(&tokens)
        .then_some(tokens)
        .ok_or_else(refused)
}

/// `value` read as the size of the Claude members' compaction window: a whole
/// number of tokens in the range, or none (a missing field or `null`) for
/// Claude's own point.
pub fn parse_claude_compaction(value: Option<&Value>) -> Result<Option<u32>, LedgerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let tokens = number.as_u64().ok_or_else(refused)?;
            size_of(u32::try_from(tokens).map_err(|_| refused())?).map(Some)
        }
        Some(_) => Err(refused()),
    }
}

/// Chooses the project's Claude members' compaction window, or takes the choice
/// away, and logs it where it changed. The size is checked before the project is
/// looked for.
pub(crate) fn set_claude_compaction(
    store: &mut Store,
    id: i64,
    window: Option<u32>,
) -> Result<ProjectView, LedgerError> {
    let to = window.map(size_of).transpose()?;
    store.write(|store| {
        let from = store.project_row(id)?.claude_compaction;
        if from != to {
            let at = store.at();
            store.db.execute(
                "UPDATE project SET claude_compaction = ?, updated_at = ? WHERE id = ?",
                params![to, at, id],
            )?;
            store.log(
                id,
                "project.claude_compaction",
                json!({ "from": from, "to": to }),
            )?;
        }
        known_project(store, id)
    })
}

#[cfg(test)]
mod tests;
