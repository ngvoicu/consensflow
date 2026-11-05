//! What a Codex thread ran on last (CHF-05): the newest of its rollout's `turn_context`
//! (`payload.model`, `payload.effort`, written as each turn begins) and its
//! `thread_settings_applied` events (`thread_settings.model`, `.reasoning_effort`, written
//! when a switch is applied, before any turn of it). Codex's window keeps both on a
//! flagless resume (T-445), but its headless resume runs the account's default (T-727), so
//! a reopen passes both.

use std::ops::ControlFlow;
use std::path::Path;

use cf_base::env::Env;
use serde_json::Value;

use super::paths;
use crate::contract::Used;
use crate::shared::tail::{lines_back, TAIL_BOUND};

/// What thread `session` ran on last.
pub(crate) fn last_used(session: &str, env: &Env) -> Option<Used> {
    let file = paths::transcript(session, env).ok()??;
    used_in(&file, TAIL_BOUND).ok()?
}

/// What the rollout `file` says, within its last `bound` bytes.
pub(super) fn used_in(file: &Path, bound: u64) -> std::io::Result<Option<Used>> {
    let mut found = None;
    lines_back(file, bound, |line| {
        if !line.contains("\"turn_context\"") && !line.contains("\"thread_settings_applied\"") {
            return ControlFlow::Continue(());
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            return ControlFlow::Continue(());
        };
        found = settings_of(&entry);
        if found.is_some() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?;
    Ok(found)
}

/// The model and effort a turn's context or an applied switch names.
fn settings_of(entry: &Value) -> Option<Used> {
    let payload = &entry["payload"];
    let (model, effort) = if entry["type"] == "turn_context" {
        (&payload["model"], &payload["effort"])
    } else if entry["type"] == "event_msg" && payload["type"] == "thread_settings_applied" {
        let settings = &payload["thread_settings"];
        (&settings["model"], &settings["reasoning_effort"])
    } else {
        return None;
    };
    Some(Used {
        model: model.as_str().filter(|model| !model.is_empty())?.to_owned(),
        effort: effort.as_str().map(str::to_owned),
        thinking: None,
        by_harness: false,
    })
}

#[cfg(test)]
mod tests;
