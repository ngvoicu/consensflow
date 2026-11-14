//! The person's per-project choice of how the chief works.

use cf_proto::ledger::ProjectView;
use rusqlite::params;
use serde_json::{json, Value};

use super::known_project;
use crate::{model::LedgerError, store::Store};

pub fn parse_own_hands(value: Option<&Value>) -> Result<bool, LedgerError> {
    value.and_then(Value::as_bool).ok_or_else(|| {
        LedgerError::refused(
            "invalid-own-hands",
            "Do with your own hands is on (true) or off (false)",
        )
    })
}

pub(crate) fn set_own_hands(
    store: &mut Store,
    id: i64,
    on: bool,
) -> Result<ProjectView, LedgerError> {
    store.write(|store| {
        let from = store.project_row(id)?.own_hands == 1;
        if from != on {
            let at = store.at();
            store.db.execute(
                "UPDATE project SET own_hands = ?, updated_at = ? WHERE id = ?",
                params![i64::from(on), at, id],
            )?;
            store.log(id, "project.own_hands", json!({ "from": from, "to": on }))?;
        }
        known_project(store, id)
    })
}
