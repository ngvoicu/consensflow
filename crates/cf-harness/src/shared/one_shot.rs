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

/// How long a program that ended before it answered is given to finish saying why: its error
/// stream is read apart, and closes a moment after the program has gone.
const SAYING_WHY_MS: u64 = 2_000;

/// How much of what a program said is quoted in a failure: its last lines that are no frame of
/// a stack, within a few hundred characters.
const QUOTE_LINES: usize = 4;
const QUOTE_CHARS: usize = 600;

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

/// `failure`, and the code `child` exited with and the last it said, as far as it has.
fn with_its_words(failure: String, child: &dyn Child) -> String {
    let code = child
        .code()
        .map_or_else(String::new, |code| format!(" (exit {code})"));
    match quoted(&child.said()) {
        Some(said) => format!("{failure}{code}; it said: {said}"),
        None => format!("{failure}{code}"),
    }
}

/// The last lines of `said` that are no frame of a stack (`at …`), on one line, within
/// [`QUOTE_CHARS`] characters: what a program says of why it ended is at the end, and what it
/// printed of the code that failed is not the reason.
fn quoted(said: &str) -> Option<String> {
    let lines: Vec<&str> = said
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("at "))
        .collect();
    let kept = &lines[lines.len().saturating_sub(QUOTE_LINES)..];
    if kept.is_empty() {
        return None;
    }
    let joined = kept.join(" ");
    Some(match joined.char_indices().nth(QUOTE_CHARS) {
        Some((cut, _)) => format!("{}…", &joined[..cut]),
        None => joined,
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
