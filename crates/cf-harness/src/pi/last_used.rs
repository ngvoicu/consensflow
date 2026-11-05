//! What a Pi session ran on last (CHF-05): its last assistant message, `provider` and
//! `model` (what `--model` takes as `provider/model`, kept apart: an OpenRouter model's id
//! holds a `/` of its own) and `thinkingLevel`. Not its `model_change` and
//! `thinking_level_change` entries: a launch's flags write neither, so after a
//! `--thinking low` launch a reading of them says the settings' level; and a native model
//! switch resets the thinking, which the next message on it says. A message that ended in
//! an error is skipped: after a launch on a gone model, Pi's own flagless resume fell back
//! to the settings' default, never to the last model that answered (T-727).

use std::ops::ControlFlow;
use std::path::Path;

use cf_base::env::Env;
use serde_json::Value;

use super::paths;
use crate::contract::Used;
use crate::shared::tail::{lines_back, TAIL_BOUND};

/// What session `session` ran on last.
pub(crate) fn last_used(session: &str, env: &Env) -> Option<Used> {
    let file = paths::transcript(session, env).ok()??;
    used_in(&file, TAIL_BOUND).ok()?
}

/// What the session `file` says, within its last `bound` bytes.
pub(super) fn used_in(file: &Path, bound: u64) -> std::io::Result<Option<Used>> {
    let mut found = None;
    lines_back(file, bound, |line| {
        if !line.contains("\"assistant\"") {
            return ControlFlow::Continue(());
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            return ControlFlow::Continue(());
        };
        found = answered_on(&entry);
        if found.is_some() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?;
    Ok(found)
}

/// The model and thinking an assistant message that did not fail ran on.
fn answered_on(entry: &Value) -> Option<Used> {
    let message = &entry["message"];
    if entry["type"] != "message"
        || message["role"] != "assistant"
        || message["stopReason"] == "error"
    {
        return None;
    }
    let provider = message["provider"]
        .as_str()
        .filter(|provider| !provider.is_empty())?;
    let model = message["model"]
        .as_str()
        .filter(|model| !model.is_empty())?;
    Some(Used {
        model: format!("{provider}/{model}"),
        effort: None,
        thinking: message["thinkingLevel"].as_str().map(str::to_owned),
        by_harness: false,
    })
}

#[cfg(test)]
mod tests;
