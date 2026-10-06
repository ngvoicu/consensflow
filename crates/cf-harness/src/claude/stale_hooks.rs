//! The hooks an older version left in Claude Code's own settings
//! (`staleClaudeHooks`, `src/host-payloads.js`): reported, never written.
//! Claude Code's settings are not ours to write, so a hook an older version
//! left there is named rather than removed behind the user's back.
//!
//! A file that is not there, cannot be read, or is no JSON says nothing. So
//! does one that is the JSON `null`, where Node's own `null.hooks` threw a
//! `TypeError` that `cf doctor` then printed as its failure: that is its
//! runtime's accident, and no sentence of ours.
//!
//! Kept from Node on purpose: JSON nested more than `cf_base::json::DEEPEST`
//! levels, or holding a number past a double's range, is no JSON here, where
//! Node read it; a settings file of that kind says nothing here.

use cf_base::env::Env;
use cf_base::json::from_slice_lossy;
use cf_base::{js, path};
use serde_json::Value;

/// The hooks of ours that Claude Code's settings still hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleHooks {
    /// The settings file looked at, whether or not it is there.
    pub path: String,
    /// The events that hold a hook that names ConsensFlow, in the order
    /// JavaScript enumerates them.
    pub events: Vec<String>,
}

/// The settings of the Claude Code `env` names, and the events in them that
/// still reference ConsensFlow.
pub fn stale_hooks(env: &Env) -> StaleHooks {
    // `CLAUDE_CONFIG_DIR ?? ~/.claude`: an empty one is a folder, the working
    // one, as `??` kept it.
    let folder = env.os("CLAUDE_CONFIG_DIR").map_or_else(
        || path::join(&[&home(env), ".claude"]),
        |folder| folder.to_string_lossy().into_owned(),
    );
    let path = path::join(&[&folder, "settings.json"]);
    let events = std::fs::read(&path)
        .ok()
        .and_then(|bytes| from_slice_lossy(&bytes).ok())
        .map(|settings| events_of(&settings))
        .unwrap_or_default();
    StaleHooks { path, events }
}

impl StaleHooks {
    /// The line `cf doctor` says of them, when there are any.
    pub fn report(&self) -> Option<String> {
        (!self.events.is_empty()).then(|| {
            format!(
                "{:<14}{} in {} still reference consensflow — no version answers them; remove those entries",
                "hooks:",
                self.events.join(", "),
                self.path
            )
        })
    }
}

/// The user's home as this reads it: `HOME`, else `USERPROFILE`, whichever is
/// set, empty or not, and nothing at all when neither is.
fn home(env: &Env) -> String {
    env.os("HOME")
        .or_else(|| env.os("USERPROFILE"))
        .map(|home| home.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `Object.entries(settings.hooks ?? {})` with a hook of ours in it: the
/// events whose entries are a list, and one of whose entries, written as
/// JSON, holds `consensflow` anywhere, its keys too. `hooks` that is a list
/// has the index of each item for its events.
fn events_of(settings: &Value) -> Vec<String> {
    let entries: Vec<(String, &Value)> = match settings.get("hooks") {
        Some(Value::Object(hooks)) => hooks
            .iter()
            .map(|(event, entries)| (event.clone(), entries))
            .collect(),
        Some(Value::Array(hooks)) => hooks
            .iter()
            .enumerate()
            .map(|(at, entries)| (at.to_string(), entries))
            .collect(),
        _ => Vec::new(),
    };
    entries
        .into_iter()
        .filter(|(_, entries)| {
            entries.as_array().is_some_and(|list| {
                list.iter()
                    .any(|entry| js::stringify(entry).contains("consensflow"))
            })
        })
        .map(|(event, _)| event)
        .collect()
}
