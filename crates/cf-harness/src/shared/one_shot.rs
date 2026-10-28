//! A harness asked something over its own standard streams, with no window and no model
//! turn: one bounded conversation of a line out and the line that answers it, then the
//! program ended. Each harness's model list is read this way (CHF-03): Claude Code's SDK
//! `initialize`, Codex's app-server `model/list`, Pi's rpc `get_available_models`.
//!
//! An answer is the first line, read as JSON, that the asker's test takes for it: the
//! notifications and the answers to other requests that come between are passed over.

use serde_json::Value;

use crate::seams::processes::{Child, Processes, Program, Streams};
use crate::seams::{arm, Time};
use crate::shared::child::stop;

/// How long a conversation has to answer: each harness answers its list in a few seconds
/// (Codex with no network in 3.5 s, research-02 §8), and past this it is given up on.
pub(crate) const BOUND_MS: u64 = 30_000;

/// The longest line an answer may be: Pi lists 469 models in about 300 KB on one line.
const LINE_LIMIT: usize = 16 << 20;

/// What `talk` makes of the program `program` starts, which `what` names in a failure
/// (`codex app-server`). Fails with the reason where it does not start, ends, or is still
/// talking at `bound_ms`. The program is ended whichever way.
pub(crate) async fn conversation<T>(
    processes: &dyn Processes,
    time: &dyn Time,
    program: Program,
    what: &str,
    bound_ms: u64,
    talk: impl AsyncFnOnce(&dyn Child) -> Result<T, String>,
) -> Result<T, String> {
    let child = processes.spawn(program, Streams::Lines)?;
    let armed = arm(time, bound_ms);
    let answered = armed.bound(talk(&*child)).await;
    // Ended whichever way: one that will not end is the process's to be given up on.
    let _ = stop(time, &*child).await;
    answered.unwrap_or_else(|| {
        Err(format!(
            "{what} did not answer within {} s",
            bound_ms / 1000
        ))
    })
}

/// Writes `line`, then reads until the line `answers` takes: that line, as JSON. Fails
/// where the program ends first, `what` naming it.
pub(crate) async fn ask(
    child: &dyn Child,
    what: &str,
    line: &Value,
    answers: impl Fn(&Value) -> bool,
) -> Result<Value, String> {
    child.write_line(&line.to_string()).await?;
    loop {
        let line = child
            .read_line(LINE_LIMIT)
            .await?
            .ok_or_else(|| format!("{what} ended before it answered"))?;
        let Ok(answer) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if answers(&answer) {
            return Ok(answer);
        }
    }
}

#[cfg(test)]
mod tests;
