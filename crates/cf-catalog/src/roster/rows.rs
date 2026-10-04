//! The roster as the app sees it, in the file's shape (`CATALOG_BY_PRESET`,
//! `catalogRow`, `entryOf` and `rows`, `src/roster.js`): every catalog agent
//! as the catalog has it, then the agents the human defined, marked
//! `custom`. A stored copy of a catalog entry is ignored; a custom row with
//! a catalog agent's name hides that entry.

use std::collections::HashSet;

use serde_json::{Map, Value};

use super::agent_row::{effort_key, AgentRow};
use super::document::Document;
use crate::presets::Preset;
use crate::Catalog;

impl Catalog {
    /// The preset a row's `preset` names (`CATALOG_BY_PRESET`). Of two
    /// presets with one name the later is found, as a JavaScript `Map` keeps
    /// the last value of a key.
    fn preset_named(&self, name: &str) -> Option<&Preset> {
        self.presets().iter().rfind(|preset| preset.preset == name)
    }

    /// The catalog entry a stored row is a copy of, if it is one: by the
    /// provenance it carries, as every copy has since 2026-08-21, and only
    /// when the row is under the entry's own name. A row without it is the
    /// human's own agent, even one whose name a later catalog took, on any
    /// harness: it hides that entry, and is never folded away as a copy.
    pub(crate) fn entry_of(&self, row: &AgentRow) -> Option<&Preset> {
        let entry = self.preset_named(row.preset()?)?;
        (row.id() == Some(entry.id.as_str())).then_some(entry)
    }

    /// The roster as the app sees it: every catalog agent the human's own
    /// names do not hide, in the presets' order, then the human's own
    /// agents in the file's order, each marked `custom`: in place when the
    /// row has the key, last when it has not.
    pub(crate) fn rows(&self, document: &Document) -> Vec<AgentRow> {
        let custom: Vec<AgentRow> = document
            .agents()
            .into_iter()
            .filter(|row| self.entry_of(row).is_none())
            .collect();
        let hidden: HashSet<&str> = custom.iter().filter_map(AgentRow::id).collect();
        let mut rows: Vec<AgentRow> = self
            .presets()
            .iter()
            .filter(|preset| !hidden.contains(preset.id.as_str()))
            .map(catalog_row)
            .collect();
        rows.extend(custom.into_iter().map(|mut row| {
            row.set("custom", Value::Bool(true));
            row
        }));
        rows
    }
}

/// A catalog entry as the row the launcher runs when nothing on it is
/// overridden. The one-line label is what a roster row calls itself; the
/// preset's own description is the catalog card's paragraph.
fn catalog_row(preset: &Preset) -> AgentRow {
    let mut fields = Map::new();
    fields.insert("id".to_owned(), Value::from(preset.id.as_str()));
    fields.insert("name".to_owned(), Value::from(preset.name.as_str()));
    fields.insert("kind".to_owned(), Value::from(preset.kind.as_str()));
    if preset.designer {
        fields.insert("designer".to_owned(), Value::Bool(true));
    }
    fields.insert("model".to_owned(), Value::from(preset.model.as_str()));
    // `preset.effort ?? preset.thinking`, said when it is not empty, under
    // the key the kind's runner reads.
    let effort = preset.effort.as_deref().or(preset.thinking.as_deref());
    if let Some(effort) = effort.filter(|effort| !effort.is_empty()) {
        fields.insert(
            effort_key(Some(&preset.kind)).to_owned(),
            Value::from(effort),
        );
    }
    fields.insert("description".to_owned(), Value::from(preset.label.as_str()));
    fields.insert("preset".to_owned(), Value::from(preset.preset.as_str()));
    AgentRow::new(fields)
}

#[cfg(test)]
mod tests;
