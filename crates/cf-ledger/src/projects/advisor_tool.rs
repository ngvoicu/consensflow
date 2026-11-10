//! Whether the human has the project's members run with Claude Code's own advisor tool, in
//! Project settings ("Advisor tool"). Off is what every project has until the human ticks it:
//! the project's Claude Code members are launched with the tool switched off
//! (`CLAUDE_CODE_DISABLE_ADVISOR_TOOL`, in the launch's own settings file); on is the owner's
//! own setting, as every member had before. It is kept on the project's row
//! (`project.advisor_tool`, migration 0035) and logged when it changes
//! (`project.advisor_tool`, with what it was and what it became, each a flag). Only the page
//! writes it: no route of the agents' API reaches it, so no `cf` command changes it.
//!
//! It is the human's word and no more: the ledger knows nothing of any harness. The engine
//! gives each member's launch the switch at launch and on resume, and a chief's launch never.

use cf_proto::ledger::ProjectView;
use rusqlite::params;
use serde_json::{json, Value};

use super::known_project;
use crate::model::LedgerError;
use crate::store::Store;

/// `value` read as the advisor tool on (true) or off (false): a flag, as the page sends it.
/// (A request to open a project that says nothing of it is off, which is what every project
/// has until the human ticks it.)
pub fn parse_advisor_tool(value: Option<&Value>) -> Result<bool, LedgerError> {
    value.and_then(Value::as_bool).ok_or_else(|| {
        LedgerError::refused(
            "invalid-advisor-tool",
            "the advisor tool is on (true) or off (false)",
        )
    })
}

/// Switches the project's advisor tool, and logs it where it changed.
pub(crate) fn set_advisor_tool(
    store: &mut Store,
    id: i64,
    on: bool,
) -> Result<ProjectView, LedgerError> {
    store.write(|store| {
        let from = store.project_row(id)?.advisor_tool == 1;
        if from != on {
            let at = store.at();
            store.db.execute(
                "UPDATE project SET advisor_tool = ?, updated_at = ? WHERE id = ?",
                params![i64::from(on), at, id],
            )?;
            store.log(
                id,
                "project.advisor_tool",
                json!({ "from": from, "to": on }),
            )?;
        }
        known_project(store, id)
    })
}
