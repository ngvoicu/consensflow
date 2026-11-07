//! Migration 0031's import of the agents of the human's own (T-1202): the ones they defined
//! and the ones Add member made for a model (T-1143's `<harness>-<model>-<effort>`), which
//! the agents file beside the ledger keeps (`agents.json`, as 3.0.6 wrote it) and no ledger
//! row does. Each participant that names one (a member, its windows, a chief from before
//! CHF-04) takes the model and the effort it saved, and loses the mark of a participant
//! that needs an edit, which 0031's SQL put on every agent it did not know. An agent the
//! file does not have, or a file that is not there or cannot be read, leaves the mark: the
//! participant is kept as it was, never turned into its harness's default.
//!
//! The file is the one the ledger's own folder holds: the daemon keeps both in
//! ConsensFlow's folder. A ledger in memory has none.

use std::path::Path;

use rusqlite::{params, Connection};
use serde_json::Value;

use crate::model::LedgerError;

/// The name of the agents file, beside the ledger.
const AGENTS_FILE: &str = "agents.json";

/// Each saved agent of the agents file beside `db`'s file run on by the participants that
/// name it, on its harness: the model it saved and the effort it saved (Pi's under
/// `thinking`, the key Pi's runner read). Devin's agent named the model and its effort
/// apart, and its launch joined them: the participant holds the joined id, as a Devin
/// member does, with its effort beside. A model the agent's harness no longer lists is
/// kept, and the staff dialog marks it gone.
pub(crate) fn import_saved_agents(db: &Connection) -> Result<(), LedgerError> {
    for agent in saved_agents(db) {
        let Some((id, kind)) = agent
            .get("id")
            .and_then(Value::as_str)
            .zip(agent.get("kind").and_then(Value::as_str))
        else {
            continue;
        };
        let text = |key: &str| {
            agent
                .get(key)
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
        };
        let effort = text(if kind == "pi" { "thinking" } else { "effort" });
        let model = text("model").map(|model| match effort {
            Some(effort) if kind == "devin" && !model.ends_with(&format!("-{effort}")) => {
                format!("{model}-{effort}")
            }
            _ => model.to_owned(),
        });
        // SQLite reads every value of a SET at the row as it was: the name the harness's list
        // gave the model stays only where the model does.
        db.execute(
            "UPDATE participant
                SET model_name = CASE WHEN model IS ?1 THEN model_name ELSE NULL END,
                    model = ?1, effort = ?2, needs_edit = 0
              WHERE agent = ?3 AND harness = ?4 AND role <> 'human'",
            params![model, effort, id, kind],
        )?;
    }
    Ok(())
}

/// The agents the file beside `db`'s file saved: none where `db` has no file, or the file
/// is not there, or is no agents file.
fn saved_agents(db: &Connection) -> Vec<Value> {
    let Some(file) = db
        .path()
        .filter(|path| !path.is_empty())
        .map(|path| Path::new(path).with_file_name(AGENTS_FILE))
    else {
        return Vec::new();
    };
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|file| file.get("agents").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}
