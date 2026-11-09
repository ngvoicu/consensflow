//! A harness asked something over its own standard streams, with no window and no model
//! turn: one bounded conversation of a line out and the line that answers it, then the
//! program ended. Each harness's model list is read this way (CHF-03): Claude Code's SDK
//! `initialize`, Codex's app-server `model/list`, Pi's rpc `get_available_models`.
//!
//! An answer is the first line, read as JSON, that the asker's test takes for it: the
//! notifications and the answers to other requests that come between are passed over.

use cf_base::startup::{StartFailure, StartKind, StartRefusal};
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

/// Why a conversation with a program failed: the words, for the page and the person's own screen
/// (they may carry what the program said), and, where the failure has a kind, what a note says of
/// it and nothing more (`cf_base::startup`, T-1328).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Said {
    pub(crate) words: String,
    pub(crate) failure: Option<StartFailure>,
}

impl From<String> for Said {
    fn from(words: String) -> Self {
        Self {
            words,
            failure: None,
        }
    }
}

impl From<StartRefusal> for Said {
    fn from(refusal: StartRefusal) -> Self {
        Self {
            failure: Some(refusal.failure()),
            words: refusal.into(),
        }
    }
}

impl std::fmt::Display for Said {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.words)
    }
}

/// A conversation's failure reads as its words, where the words are what is looked at.
impl std::ops::Deref for Said {
    type Target = str;

    fn deref(&self) -> &str {
        &self.words
    }
}

impl From<Said> for String {
    fn from(said: Said) -> Self {
        said.words
    }
}

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
) -> Result<T, Said> {
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
) -> Result<T, Said> {
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
            Err(Said {
                words: failure,
                failure: Some(StartFailure::of(StartKind::Failed)),
            })
        }
        None => Err(Said {
            words: format!("{what} did not answer within {} s", bound_ms / 1000),
            failure: Some(StartFailure::of(StartKind::NoAnswer)),
        }),
    };
    let outcome = if keeps_words {
        outcome.map_err(|said| Said {
            words: with_its_words(said.words, &*child),
            failure: with_its_status(said.failure, &*child),
        })
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

/// What a note says of a program that ended with an error: the status it exited with, where it
/// exited with one. Nothing it wrote is in it.
fn with_its_status(kind: Option<StartFailure>, child: &dyn Child) -> Option<StartFailure> {
    kind.map(|kind| match child.code() {
        Some(code) if kind.kind() == StartKind::Failed => kind.with_exit(i64::from(code)),
        _ => kind,
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
