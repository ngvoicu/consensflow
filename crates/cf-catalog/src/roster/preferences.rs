//! What the human chose about the roster, kept in the file beside their own
//! agents (`preferencesOf`, `RELAYED` and `hides`, `src/roster.js`).

use std::path::Path;

use cf_base::json::as_parsed_fields;
use cf_base::refusal::Refusal;
use cf_proto::agents::{AgentView, Preferences};
use serde_json::{Map, Value};

use super::document::{load_document, Document};
use super::save::save_document;

/// The choices the file records. One it does not record is off, and so is
/// one that is anything but `true`.
pub(crate) fn preferences_of(document: &Document) -> Preferences {
    let own_harness_only = document
        .get("preferences")
        .and_then(|preferences| preferences.get("ownHarnessOnly"))
        == Some(&Value::Bool(true));
    Preferences { own_harness_only }
}

/// What the human may choose: each by its name in the file.
const PREFERENCE_KEYS: [&str; 1] = ["ownHarnessOnly"];

/// `setPreferences`: the choices `patch` names set, each refused in the
/// order JavaScript enumerates the request's keys, then the whole set kept
/// in the file: a write even when nothing changes, and a file made when
/// there is none. A key the file kept that this build does not know is
/// dropped from it.
pub(crate) fn set_preferences(path: &Path, patch: Option<&Value>) -> Result<Preferences, Refusal> {
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
        next.own_harness_only = on;
    }
    document.set(
        "preferences",
        Value::Object(Map::from_iter([(
            "ownHarnessOnly".to_owned(),
            Value::Bool(next.own_harness_only),
        )])),
    );
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

/// The harnesses that reach other makers' models.
const RELAYED: [&str; 2] = ["pi", "opencode"];

/// Whether the list the human sees leaves `view` out: Claude and OpenAI
/// models reached through Pi or OpenCode are hidden when the human keeps
/// them to their own harnesses. A member already on one still runs.
pub(crate) fn hides(preferences: Preferences, view: &AgentView) -> bool {
    preferences.own_harness_only
        && view
            .harness
            .as_deref()
            .is_some_and(|harness| RELAYED.contains(&harness))
        && ["claude-", "gpt-"]
            .iter()
            .any(|prefix| view.profile.model_key.starts_with(prefix))
}

#[cfg(test)]
mod tests;
