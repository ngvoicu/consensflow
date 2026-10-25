//! What the human chose about the roster, kept in the file beside their own
//! agents, under `preferences`; and the one writer of what the human chose,
//! which also writes what they say of specs for every project (`app_specs`): a
//! write of either keeps the other as the file has it, and keeps the keys of
//! `preferences` a later build wrote and this one does not know.

use std::path::Path;

use cf_base::json::as_parsed_fields;
use cf_base::refusal::Refusal;
use cf_proto::agents::{AgentView, Preferences};
use cf_proto::specs::AppSpecs;
use serde_json::{Map, Value};

use super::app_specs::{app_specs_of, keep_defaults, EARLIER};
use super::document::{load_document, Document};
use super::save::save_document;

/// The choices the file records. One it does not record is off, and so is
/// one that is anything but `true`.
pub(crate) fn preferences_of(document: &Document) -> Preferences {
    Preferences {
        own_harness_only: kept(document, "ownHarnessOnly") == Some(&Value::Bool(true)),
    }
}

/// What the file's `preferences` keep under `key`, as written.
pub(super) fn kept<'a>(document: &'a Document, key: &str) -> Option<&'a Value> {
    document
        .get("preferences")
        .and_then(|preferences| preferences.get(key))
}

/// What the human may choose about the roster: each by its name in the file.
const PREFERENCE_KEYS: [&str; 1] = ["ownHarnessOnly"];

/// `setPreferences`: the choices `patch` names set, each refused in the
/// order JavaScript enumerates the request's keys, then the whole set kept
/// in the file: a write even when nothing changes, and a file made when
/// there is none. The app's specs are kept as they were: they are no choice
/// about the roster, and a patch that names them is refused as one that names
/// any other.
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
    let app = app_specs_of(&document);
    keep(path, &mut document, next, &app)?;
    Ok(next)
}

/// Writes what the human chose into the file at `path`, whose `document` was
/// read for the write. `preferences` holds the choice about the roster, then
/// whether specs are on for every project (only when they are), as Node and
/// 3.0.4 wrote it, so a file that never knew of more is written as it always
/// was; then every key a later build kept there, as it was. The app's folder
/// and variant go under a key of their own (`app_specs`), and where an earlier
/// build of this one kept them in `preferences` they are moved there.
pub(super) fn keep(
    path: &Path,
    document: &mut Document,
    preferences: Preferences,
    app: &AppSpecs,
) -> Result<(), Refusal> {
    let mut kept = Map::from_iter([(
        "ownHarnessOnly".to_owned(),
        Value::Bool(preferences.own_harness_only),
    )]);
    if app.on {
        kept.insert("specs".to_owned(), Value::Bool(true));
    }
    if let Some(Value::Object(before)) = document.get("preferences") {
        let later = before.iter().filter(|(key, _)| {
            !matches!(key.as_str(), "ownHarnessOnly" | "specs") && !EARLIER.contains(&key.as_str())
        });
        kept.extend(later.map(|(key, value)| (key.clone(), value.clone())));
    }
    document.set("preferences", Value::Object(kept));
    keep_defaults(document, app);
    save_document(path, document)
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

/// The harnesses that reach other makers' models: Devin's Claude and GPT
/// agents too (the owner, 2026-10-25).
const RELAYED: [&str; 3] = ["pi", "opencode", "devin"];

/// Whether the list the human sees leaves `view` out: Claude and OpenAI
/// models reached through Pi, OpenCode or Devin are hidden when the human
/// keeps them to their own harnesses. A member already on one still runs.
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
