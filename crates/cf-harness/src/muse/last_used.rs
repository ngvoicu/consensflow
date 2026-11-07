//! What a Muse Code conversation ran on last (CHF-05): the model of the newest
//! `run.model.configured` of its session's log, which Muse writes for each turn it runs,
//! and whether Muse moved to it by itself. A move is a completed
//! `runtime.model_reconfigure.completed` to that model; it is Muse's own (MUS-24) unless the
//! human asked for it with a `/model` they typed (`command.invoked`, provisional: never seen
//! live) or Muse put a resumed session back on its launch's model (`previous.source` is
//! `replay`), and unless it moved to the model the launch asked for. The session index's
//! `model_id` keeps the first model of the session whatever came after (T-727): never read.
//! Muse's record keeps no effort for a turn, and a resume does not restore it (CHF-02).
//!
//! A human's `/model` in the window writes its reconfigure at once, and the next turn's
//! `run.model.configured` names the model: a reopen before that turn goes on with the one
//! before it, as Claude Code's does.

use std::ops::ControlFlow;
use std::path::Path;

use cf_base::env::Env;
use serde_json::Value;

use super::paths;
use super::record::{asks_for_a_model, field, model_id};
use crate::contract::Used;
use crate::shared::tail::{lines_back, TAIL_BOUND};

/// What conversation `session` ran on last; `asked` the model its last launch asked for.
pub(crate) fn last_used(session: &str, asked: Option<&str>, env: &Env) -> Option<Used> {
    let file = paths::transcript(session, env).ok()??;
    used_in(&file, asked, TAIL_BOUND).ok()?
}

/// What the log `file` says, within its last `bound` bytes.
pub(super) fn used_in(
    file: &Path,
    asked: Option<&str>,
    bound: u64,
) -> std::io::Result<Option<Used>> {
    let mut used: Option<Used> = None;
    // The reconfigure that moved to the model used, while the walk looks for who asked.
    let mut moved = false;
    lines_back(file, bound, |line| {
        if !line.contains("payload_type") {
            return ControlFlow::Continue(());
        }
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            return ControlFlow::Continue(());
        };
        let payload = record.get("payload");
        let kind = record.get("payload_type").and_then(Value::as_str);
        match (&mut used, kind) {
            (None, Some("run.model.configured")) => {
                let Some(model) = model_id(field(payload, "record")) else {
                    return ControlFlow::Continue(());
                };
                used = Some(Used {
                    model: model.to_owned(),
                    effort: None,
                    thinking: None,
                    by_harness: false,
                });
                // The launch's own model needs no word of how it came.
                if asked == Some(model) {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            }
            (Some(used), Some("runtime.model_reconfigure.completed")) if !moved => {
                let record = field(payload, "record");
                let completed =
                    field(record, "apply_outcome").and_then(Value::as_str) == Some("completed");
                if !completed || model_id(field(record, "effective")) != Some(used.model.as_str()) {
                    return ControlFlow::Continue(());
                }
                let replay = field(field(record, "previous"), "source").and_then(Value::as_str)
                    == Some("replay");
                if replay {
                    return ControlFlow::Break(());
                }
                // Muse's own, unless the human's `/model` came just before it.
                used.by_harness = true;
                moved = true;
                ControlFlow::Continue(())
            }
            (Some(used), Some("command.invoked")) if moved => {
                let command = field(field(payload, "record"), "command").and_then(Value::as_str);
                if asks_for_a_model(command) {
                    used.by_harness = false;
                }
                ControlFlow::Break(())
            }
            // An earlier turn, or an earlier move: nothing before it says how this one came.
            (Some(_), Some("run.model.configured" | "runtime.model_reconfigure.completed")) => {
                ControlFlow::Break(())
            }
            _ => ControlFlow::Continue(()),
        }
    })?;
    Ok(used)
}

#[cfg(test)]
mod tests;
