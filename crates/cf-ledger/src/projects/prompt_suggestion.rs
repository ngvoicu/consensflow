//! Whether the human leaves Claude Code's prompt suggestion on in the project's windows, in
//! Project settings ("Prompt suggestion"). Off is what every project has until the human ticks
//! it, and what every project that existed when it was added was given: the project's Claude Code
//! windows, the chief's and every member's, are launched with the suggestion switched off
//! (`CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION` as `false`, in the launch's own settings file); on is
//! the owner's own setting and Claude Code's own default, about which ConsensFlow writes nothing.
//! It is kept on the project's row (`project.prompt_suggestion`, migration 0036) and logged when
//! it changes (`project.prompt_suggestion`, with what it was and what it became, each a flag).
//! Only the page writes it: no route of the agents' API reaches it, so no `cf` command changes it.
//!
//! It is the human's word and no more: the ledger knows nothing of any harness. The engine gives
//! each window's launch the switch at launch and on resume, the chief's as a member's.

use cf_proto::ledger::ProjectView;
use rusqlite::params;
use serde_json::{json, Value};

use super::known_project;
use crate::model::LedgerError;
use crate::store::Store;

/// `value` read as the prompt suggestion on (true) or off (false): a flag, as the page sends it.
/// (A request to open a project that says nothing of it is off, which is what every project has
/// until the human ticks it.)
pub fn parse_prompt_suggestion(value: Option<&Value>) -> Result<bool, LedgerError> {
    value.and_then(Value::as_bool).ok_or_else(|| {
        LedgerError::refused(
            "invalid-prompt-suggestion",
            "the prompt suggestion is on (true) or off (false)",
        )
    })
}

/// Switches the project's prompt suggestion, and logs it where it changed.
pub(crate) fn set_prompt_suggestion(
    store: &mut Store,
    id: i64,
    on: bool,
) -> Result<ProjectView, LedgerError> {
    store.write(|store| {
        let from = store.project_row(id)?.prompt_suggestion == 1;
        if from != on {
            let at = store.at();
            store.db.execute(
                "UPDATE project SET prompt_suggestion = ?, updated_at = ? WHERE id = ?",
                params![i64::from(on), at, id],
            )?;
            store.log(
                id,
                "project.prompt_suggestion",
                json!({ "from": from, "to": on }),
            )?;
        }
        known_project(store, id)
    })
}
