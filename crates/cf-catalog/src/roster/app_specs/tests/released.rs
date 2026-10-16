//! The writer of the human's choices as the released 3.0.4 has it: the
//! functions of `crates/cf-catalog/src/roster/preferences.rs` at `28ca102e`
//! (the tag `v3.0.4`), word for word but for the `Preferences` they return,
//! which had `specs` then and is `Choices` here. A fixture for what becomes of a
//! file this build wrote once 3.0.4, which shares it, has saved it. Its
//! document and save are this build's, which are as they were at that tag.

use std::path::Path;

use cf_base::json::as_parsed_fields;
use cf_base::refusal::Refusal;
use serde_json::{Map, Value};

use crate::roster::document::{load_document, Document};
use crate::roster::save::save_document;

/// 3.0.4's `Preferences`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Choices {
    pub(super) own_harness_only: bool,
    pub(super) specs: bool,
}

/// The choices the file records. One it does not record is off, and so is
/// one that is anything but `true`.
pub(super) fn preferences_of(document: &Document) -> Choices {
    let chosen = |key: &str| {
        document
            .get("preferences")
            .and_then(|preferences| preferences.get(key))
            == Some(&Value::Bool(true))
    };
    Choices {
        own_harness_only: chosen("ownHarnessOnly"),
        specs: chosen("specs"),
    }
}

/// What the human may choose: each by its name in the file.
const PREFERENCE_KEYS: [&str; 2] = ["ownHarnessOnly", "specs"];

/// `setPreferences`: the choices `patch` names set, each refused in the
/// order JavaScript enumerates the request's keys, then the whole set kept
/// in the file: a write even when nothing changes, and a file made when
/// there is none. A key the file kept that this build does not know is
/// dropped from it. Specs are kept only when on: a file that never knew of
/// them reads, and is written, as it always was.
pub(super) fn set_preferences(path: &Path, patch: Option<&Value>) -> Result<Choices, Refusal> {
    let mut document = load_document(path)?;
    let mut next = preferences_of(&document);
    for (key, value) in entries(patch) {
        if !PREFERENCE_KEYS.contains(&key.as_str()) {
            return Err(Refusal::new(
                "preference-unknown",
                format!("no preference named {key}"),
            ));
        }
        let Value::Bool(on) = value else {
            return Err(Refusal::new(
                "preference-switch",
                format!("{key} is on or off"),
            ));
        };
        match key.as_str() {
            "specs" => next.specs = on,
            _ => next.own_harness_only = on,
        }
    }
    let mut kept = Map::from_iter([(
        "ownHarnessOnly".to_owned(),
        Value::Bool(next.own_harness_only),
    )]);
    if next.specs {
        kept.insert("specs".to_owned(), Value::Bool(true));
    }
    document.set("preferences", Value::Object(kept));
    save_document(path, &mut document)?;
    Ok(next)
}

/// `Object.entries(patch ?? {})`: an object's fields in the order JavaScript
/// enumerates them, a list's items under their indices, a text's characters
/// under theirs, and nothing of a number or a flag. Every index is a key no
/// preference has, so of a list or a text only the first entry is ever read.
fn entries(patch: Option<&Value>) -> Vec<(String, Value)> {
    match patch {
        Some(Value::Object(fields)) => {
            let mut fields = fields.clone();
            as_parsed_fields(&mut fields);
            fields.into_iter().collect()
        }
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(index, item)| (index.to_string(), item.clone()))
            .collect(),
        Some(Value::String(text)) => text
            .chars()
            .enumerate()
            .map(|(index, character)| (index.to_string(), Value::from(character.to_string())))
            .collect(),
        _ => Vec::new(),
    }
}
