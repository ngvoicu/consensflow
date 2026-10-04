//! The document `agents.json` holds (`storedDocument` and `loadDocument`,
//! `src/roster.js`): an ordered JSON object with typed readings over it,
//! never a struct, since what the file carries beyond the agents is not
//! this build's to drop.

use std::path::Path;

use cf_base::refusal::Refusal;
use serde_json::{Map, Value};

use super::agent_row::AgentRow;
use super::file::{read_roster, unreadable};

/// The file as it is stored, with the version and the list an older one left
/// out. Everything else the file carried is kept, each key in its place: the
/// two that were left out come last, the version first. No file is the file
/// with nothing in it.
pub(crate) fn stored_document(path: &Path) -> Result<Map<String, Value>, Refusal> {
    let mut fields = read_roster(path)?.unwrap_or_default();
    // `schemaVersion ?? 1`: a version that is `null` is no version.
    if matches!(fields.get("schemaVersion"), None | Some(Value::Null)) {
        fields.insert("schemaVersion".to_owned(), Value::from(1));
    }
    if !matches!(fields.get("agents"), Some(Value::Array(_))) {
        fields.insert("agents".to_owned(), Value::Array(Vec::new()));
    }
    Ok(fields)
}

/// The file in the shape this build reads: stored, every row of a shape it
/// can read, and an image agent an older build saved on a harness of its own
/// (`image`, until 2026-10-03) the Codex agent with the designer flag it is
/// now.
#[derive(Debug)]
pub(crate) struct Document {
    /// Every key of the file in JavaScript's order; `agents` holds the rows
    /// [`load_document`] checked.
    fields: Map<String, Value>,
}

impl Document {
    /// Every key of the file with what it holds.
    pub(crate) fn fields(&self) -> &Map<String, Value> {
        &self.fields
    }

    /// The rows, in the file's order. Each was checked when the document was
    /// loaded, so none is left out here.
    pub(crate) fn agents(&self) -> Vec<AgentRow> {
        match self.fields.get("agents") {
            Some(Value::Array(rows)) => rows
                .iter()
                .filter_map(|row| AgentRow::from_value(row.clone()))
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// `loadDocument`: the stored document, its rows checked. A row of the wrong
/// shape refuses the file, as one that is no agents file: stricter than Node,
/// which failed with a TypeError or passed the value through, and kept on
/// purpose. The work tier a row names is not checked here: only a view of the
/// row, with its profile, refuses a tier no longer known.
pub(crate) fn load_document(path: &Path) -> Result<Document, Refusal> {
    let mut fields = stored_document(path)?;
    if let Some(Value::Array(rows)) = fields.get_mut("agents") {
        for row in rows {
            let checked = AgentRow::from_value(row.take())
                .ok_or_else(|| unreadable(path, "is not an agents file"))?;
            *row = designing_codex(checked).into_value();
        }
    }
    Ok(Document { fields })
}

/// An image agent as it is now: `{...row, kind: 'codex', designer: true}`.
/// `kind` keeps its place, and so does `designer` when the row had it.
fn designing_codex(mut row: AgentRow) -> AgentRow {
    if row.kind() == Some("image") {
        row.set("kind", Value::from("codex"));
        row.set("designer", Value::Bool(true));
    }
    row
}

#[cfg(test)]
mod tests;
