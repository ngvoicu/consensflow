//! What the human chose about the roster, kept in the file beside their own
//! agents (`preferencesOf`, `RELAYED` and `hides`, `src/roster.js`).

use cf_proto::agents::{AgentView, Preferences};
use serde_json::Value;

use super::document::Document;

/// The choices the file records. One it does not record is off, and so is
/// one that is anything but `true`.
pub(crate) fn preferences_of(document: &Document) -> Preferences {
    let own_harness_only = document
        .fields()
        .get("preferences")
        .and_then(|preferences| preferences.get("ownHarnessOnly"))
        == Some(&Value::Bool(true));
    Preferences { own_harness_only }
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
