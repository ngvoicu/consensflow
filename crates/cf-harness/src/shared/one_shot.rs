//! A harness asked something over its own standard streams, with no window and no model
//! turn: one bounded conversation of a line out and the line that answers it, then the
//! program ended. Each harness's model list is read this way (CHF-03): Claude Code's SDK
//! `initialize`, Codex's app-server `model/list`, Pi's rpc `get_available_models`.
//!
//! An answer is the first line, read as JSON, that the asker's test takes for it: the
//! notifications and the answers to other requests that come between are passed over.

use cf_proto::models::{IT_SAID, WORDS_WITHHELD};
use serde_json::Value;

use crate::seams::processes::{Child, Processes, Program, Streams};
use crate::seams::{arm, Time};
use crate::shared::child::stop;

/// How long a conversation has to answer: each harness answers its list in a few seconds
/// (Codex with no network in 3.5 s, research-02 §8), and past this it is given up on.
pub(crate) const BOUND_MS: u64 = 30_000;

/// The longest line an answer may be: Pi lists 469 models in about 300 KB on one line.
const LINE_LIMIT: usize = 16 << 20;

/// How long a program that ended before it answered is given to finish saying why: its error
/// stream is read apart, and closes a moment after the program has gone.
const SAYING_WHY_MS: u64 = 2_000;

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
    conversation_as(
        processes,
        time,
        program,
        Streams::Lines,
        what,
        bound_ms,
        talk,
    )
    .await
}

/// [`conversation`] with the program started as `streams` says. Started as
/// [`Streams::Conversation`], it is a read of a program that loads code of the human's, and a
/// failure says what the program said on its error stream and the code it exited with
/// ([`with_its_words`]): the one start of it is the only evidence of why it failed, and it is
/// never started again to find out. What it started ends with it.
pub(crate) async fn conversation_as<T>(
    processes: &dyn Processes,
    time: &dyn Time,
    program: Program,
    streams: Streams,
    what: &str,
    bound_ms: u64,
    talk: impl AsyncFnOnce(&dyn Child) -> Result<T, String>,
) -> Result<T, String> {
    let child = processes.spawn(program, streams)?;
    let armed = arm(time, bound_ms);
    let answered = armed.bound(talk(&*child)).await;
    let keeps_words = streams == Streams::Conversation;
    let outcome = match answered {
        Some(Ok(answer)) => Ok(answer),
        Some(Err(failure)) => {
            // It ended or its pipe broke: its error stream closes a moment after it.
            if keeps_words {
                let _ = arm(time, SAYING_WHY_MS).bound(child.closed()).await;
            }
            Err(failure)
        }
        None => Err(format!(
            "{what} did not answer within {} s",
            bound_ms / 1000
        )),
    };
    let outcome = if keeps_words {
        outcome.map_err(|failure| with_its_words(failure, &*child))
    } else {
        outcome
    };
    // Ended whichever way: one that will not end is the process's to be given up on.
    let _ = stop(time, &*child).await;
    outcome
}

/// `failure`, and the code `child` exited with, and after [`IT_SAID`] **all** it has said that
/// is kept (the last `SAID_KEPT` bytes of its error stream, by cf-process), as it said it: not
/// cut to its last lines here. The words of a program can carry a credential, and a cut made
/// before they are looked at takes the line that names it and leaves the lines of its value
/// (the third reading of T-1270, F2); whoever keeps or tells the failure looks at the whole
/// block first and cuts it after (`cf_engine::quotable`), and what the program wrote never
/// stands before the mark, where ConsensFlow's own words are.
///
/// Where the front of what it said was let go (cf-process keeps the last bytes only), the
/// beginning of a value can be in what is kept and its label in what is not, and no look at what
/// is kept can tell (T-1276, `label-before-4KiB`): none of it is handed on, and
/// [`WORDS_WITHHELD`] stands after the mark in its place.
fn with_its_words(failure: String, child: &dyn Child) -> String {
    let code = child
        .code()
        .map_or_else(String::new, |code| format!(" (exit {code})"));
    let said = child.said();
    let said = said.trim();
    if said.is_empty() {
        format!("{failure}{code}")
    } else if child.said_cut() {
        format!("{failure}{code}{IT_SAID}{WORDS_WITHHELD}")
    } else {
        format!("{failure}{code}{IT_SAID}{said}")
    }
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
