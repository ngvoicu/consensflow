//! What a Claude Code conversation ran on last (CHF-05): its last assistant entry of the
//! main conversation, `message.model` (the id an alias resolved to: `--model sonnet` is
//! recorded as `claude-sonnet-5-5`) and the entry's own `effort`. A resume with no flag
//! keeps the model but not the effort (`low` came back `medium`), so a reopen passes both.
//! Skipped: a sidechain's entries (its sub-agents'), and what Claude Code wrote itself, a
//! `<synthetic>` model or an API error (a launch on a gone model writes one). A switch with
//! `/model` shows nowhere until the next assistant entry: the three entries the command
//! echoes name no model, so a reopen before that entry goes on with the model before it.
//! A move Claude Code made by itself is its `fallback` block, the one block of an assistant
//! entry, `{type, from: {model}, to: {model}}` (2.1.205, T-816): the model it moved to is
//! said to be the harness's, never the human's, until a model the human chose follows it.

use std::ops::ControlFlow;
use std::path::Path;

use cf_base::env::Env;
use serde_json::Value;

use super::paths;
use crate::contract::Used;
use crate::shared::tail::{lines_back, TAIL_BOUND};

/// What conversation `session` ran on last; `asked` the model its last launch asked for.
pub(crate) fn last_used(session: &str, asked: Option<&str>, env: &Env) -> Option<Used> {
    let file = paths::transcript(session, env).ok()??;
    used_in(&file, asked, TAIL_BOUND).ok()?
}

/// What the transcript `file` says, within its last `bound` bytes.
pub(super) fn used_in(
    file: &Path,
    asked: Option<&str>,
    bound: u64,
) -> std::io::Result<Option<Used>> {
    let mut newest: Option<Used> = None;
    lines_back(file, bound, |line| {
        if !line.contains("\"assistant\"") {
            return ControlFlow::Continue(());
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            return ControlFlow::Continue(());
        };
        let Some(model) = model_of(&entry) else {
            return ControlFlow::Continue(());
        };
        let fell_back = fallback_to(&entry) == Some(model);
        match &mut newest {
            None => {
                let done = fell_back || asked == Some(model);
                newest = Some(Used {
                    model: model.to_owned(),
                    effort: entry["effort"].as_str().map(str::to_owned),
                    thinking: None,
                    by_harness: fell_back,
                });
                // What the launch asked for needs no word of how it came.
                if done {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            }
            // Back through the entries on the same model, for the move that began them.
            Some(used) if used.model == model => {
                if fell_back {
                    used.by_harness = true;
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            }
            // An entry on another model: the newest model came with no word of Claude Code's.
            Some(_) => ControlFlow::Break(()),
        }
    })?;
    Ok(newest)
}

/// The model an assistant entry of the main conversation ran on, where Claude Code ran it
/// on one: none for a sidechain's, a `<synthetic>` one, or an API error's.
fn model_of(entry: &Value) -> Option<&str> {
    if entry["type"] != "assistant"
        || entry["isSidechain"] == true
        || entry["isApiErrorMessage"] == true
    {
        return None;
    }
    entry["message"]["model"]
        .as_str()
        .filter(|model| !model.is_empty() && *model != "<synthetic>")
}

/// The model an entry's `fallback` block says Claude Code moved to.
fn fallback_to(entry: &Value) -> Option<&str> {
    entry["message"]["content"]
        .as_array()?
        .iter()
        .find(|block| block["type"] == "fallback")?["to"]["model"]
        .as_str()
}

#[cfg(test)]
mod tests;
