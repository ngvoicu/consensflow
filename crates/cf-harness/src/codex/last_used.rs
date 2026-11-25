//! What a Codex thread ran on last (CHF-05): the newest of its rollout's `turn_context`
//! (`payload.model`, `payload.effort`, written as each turn begins) and its
//! `thread_settings_applied` events (`thread_settings.model`, `.reasoning_effort`, written
//! when a switch is applied, before any turn of it). Codex's window keeps both on a
//! flagless resume (T-445), but its headless resume runs the account's default (T-727), so
//! a reopen passes both.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read};
use std::ops::ControlFlow;
use std::path::Path;

use cf_base::env::Env;
use serde_json::Value;

use super::paths;
use crate::contract::Used;
use crate::shared::record::zstd;
use crate::shared::tail::{lines_back, TAIL_BOUND};

/// What thread `session` ran on last.
pub(crate) fn last_used(session: &str, env: &Env) -> Option<Used> {
    let file = paths::transcript(session, env).ok()??;
    used_in(&file, TAIL_BOUND).ok()?
}

/// What the rollout `file` says, within its last `bound` bytes.
pub(super) fn used_in(file: &Path, bound: u64) -> std::io::Result<Option<Used>> {
    if zstd::is_rollout(file) {
        return used_in_compressed(file, bound);
    }
    let mut found = None;
    lines_back(file, bound, |line| {
        found = settings_in_line(line);
        if found.is_some() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?;
    Ok(found)
}

/// Keep only the decoded tail and the byte before it, so the same bound and
/// whole-line rule as `lines_back` apply without materializing the rollout.
fn used_in_compressed(file: &Path, bound: u64) -> io::Result<Option<Used>> {
    let bound = usize::try_from(bound.min(TAIL_BOUND))
        .map_err(|_| io::Error::other("rollout tail bound does not fit memory"))?;
    let capacity = bound + 1;
    let mut tail = VecDeque::new();
    let mut decoder = zstd::decoder(File::open(file)?)?;
    let mut chunk = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = decoder.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if read >= capacity {
            tail.clear();
            tail.extend(&chunk[read - capacity..read]);
        } else {
            let discard = (tail.len() + read).saturating_sub(capacity);
            tail.drain(..discard);
            tail.extend(&chunk[..read]);
        }
    }
    let tail = tail.make_contiguous();
    let (before, tail) = if total > bound as u64 {
        (Some(tail[0]), &tail[1..])
    } else {
        (None, &tail[..])
    };
    let mut lines = tail.rsplit(|byte| *byte == b'\n').peekable();
    while let Some(line) = lines.next() {
        if lines.peek().is_none() && before.is_some_and(|byte| byte != b'\n') {
            break;
        }
        if let Some(used) = std::str::from_utf8(line).ok().and_then(settings_in_line) {
            return Ok(Some(used));
        }
    }
    Ok(None)
}

fn settings_in_line(line: &str) -> Option<Used> {
    if !line.contains("\"turn_context\"") && !line.contains("\"thread_settings_applied\"") {
        return None;
    }
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|entry| settings_of(&entry))
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
