//! What a Devin session ran on last (CHF-05): its row's `model` in the store's `sessions`,
//! which Devin rewrites as the session's model moves (a `--model` resume rewrote it; a
//! `/model` switch too, T-445), the effort inside the id (`swe-2-high`). Never a message
//! node's: the store keeps every revision of the conversation as a node, and 221 nodes
//! held 69 distinct messages after four starts and resumes and one switch (T-445), so the
//! first that names a model is no word on the last.

use std::path::Path;

use cf_base::env::Env;

use super::paths;
use crate::contract::Used;
use crate::shared::record::sqlite::{self, Cell};

/// What session `session` ran on last.
pub(crate) fn last_used(session: &str, env: &Env) -> Option<Used> {
    let file = paths::store(env).ok()?;
    used_in(Path::new(&file), session).ok()?
}

/// What the store in `file` says of `session`: none where it holds no such session, or
/// names no model for it.
pub(super) fn used_in(file: &Path, session: &str) -> Result<Option<Used>, String> {
    let store = sqlite::Store::open(file)?;
    store.read(|reads| {
        let row = reads.get("select model from sessions where id = ?", [session])?;
        Ok(match row.as_ref().and_then(|row| row.get("model")) {
            Some(Cell::Text(model)) if !model.is_empty() => Some(Used {
                model: model.clone(),
                effort: None,
                thinking: None,
                by_harness: false,
            }),
            _ => None,
        })
    })
}

#[cfg(test)]
mod tests;
