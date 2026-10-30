//! `cargo xtask scrub-muse-session <session.jsonl> <fixture.jsonl>`: a fixture of a
//! Muse Code session log that `crates/cf-harness/tests/fixtures/muse/record.py` recorded on Muse's
//! own fake model (`--provider echo`), in a throwaway home of the macOS temporary
//! folder. Every record stays, in its order, with every key; what is not the
//! conversation's is replaced. It is how the files of
//! `crates/cf-harness/tests/fixtures/muse` were made, and
//! `crates/cf-harness/tests/fixtures.rs` holds them to being so.
//!
//! What goes:
//!
//! - the throwaway home (`/var/folders/<a>/<b>/T/muse-rec-<x>`, `/private` before it
//!   or not), which becomes `/muse-home`, and any other place in the temporary
//!   folder, which becomes `/tmp/`;
//! - Muse's runtime socket (`/tmp/tbh-<uid>-rt/muse/ms-<profile>.sock`, `/private`
//!   before it or not, and the `.lease` beside it), which names the user's uid and a
//!   profile's id, and becomes [`SOCKET`];
//! - Muse's own words, which are Meta's and no part of a conversation (the base
//!   prompt, the context it builds for each request, its diagnostics of that
//!   context): every text longer than [`LONGEST`] of a record of
//!   [`MUSE_S_OWN`], of a context block Muse wrote itself (one no hook wrote), and
//!   of the frame that begins the log. Each becomes `[scrubbed: <n> bytes]`.
//!
//! The user's words, the model's, a hook's context, and every id, kind and time
//! stay as Muse wrote them (a record is read and written as JSON, its keys where
//! they were).

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};

pub const COMMANDS: &[Command] = &[Command {
    words: &["scrub-muse-session"],
    about: "Take Muse's own words and the recorder's folders out of a Muse session log, to make a fixture of it",
    usage: "<session.jsonl> <fixture.jsonl>",
    run: Run::Native(scrub_muse_session),
}];

/// What the recorder's throwaway home becomes.
const HOME: &str = "/muse-home";

/// The name the recorder gives its throwaway home in the temporary folder.
const RECORDER_HOME: &str = "muse-rec-";

/// What Muse's runtime socket becomes.
const SOCKET: &str = "/tmp/muse-runtime/ms.sock";

/// The folder Muse's runtime socket is in, by the user's uid: `/tmp/tbh-<uid>-rt/`.
const RUNTIME: &str = "/tmp/tbh-";

/// The longest text of Muse's own that stays.
const LONGEST: usize = 64;

/// The run events that hold Muse's own words: what it sends the model and what it
/// says of the context it built.
const MUSE_S_OWN: [&str; 4] = [
    "model_request_configured",
    "model_input_trace_recorded",
    "context_block_diagnostic",
    "provider_request_options_configured",
];

/// Why a log was not made a fixture.
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("could not read {path}: {cause}")]
    Read { path: String, cause: std::io::Error },
    #[error("could not write {path}: {cause}")]
    Write { path: String, cause: std::io::Error },
    #[error("line {line} is not JSON: {cause}")]
    NotJson {
        line: usize,
        cause: serde_json::Error,
    },
    #[error("line {line} is no record: a record is a JSON object")]
    NotRecord { line: usize },
}

fn scrub_muse_session(
    _: &Context,
    args: &[OsString],
    console: &mut Console,
) -> Result<i32, Failure> {
    let [from, to] = args else {
        return Err(Failure::Usage(
            "scrub-muse-session takes the session log to scrub and the fixture to make: \
             <session.jsonl> <fixture.jsonl>"
                .into(),
        ));
    };
    match make(Path::new(from), Path::new(to)) {
        Ok(records) => {
            writeln!(
                console.out,
                "{records} records kept in {}",
                Path::new(to).display()
            )?;
            Ok(0)
        }
        Err(error) => {
            writeln!(console.err, "scrub-muse-session: {error}")?;
            Ok(1)
        }
    }
}

/// Makes the fixture `to` of the log `from`, and says how many records it holds.
fn make(from: &Path, to: &Path) -> Result<usize, Error> {
    let log = fs::read_to_string(from).map_err(|cause| Error::Read {
        path: from.display().to_string(),
        cause,
    })?;
    let (records, text) = scrub(&log)?;
    fs::write(to, text).map_err(|cause| Error::Write {
        path: to.display().to_string(),
        cause,
    })?;
    Ok(records)
}

/// The fixture of `log`: its records, a line each (blank lines left out), each
/// scrubbed, and how many there are. A line that is none is said by its number.
fn scrub(log: &str) -> Result<(usize, String), Error> {
    let mut lines = Vec::new();
    for (at, line) in log.split('\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let line_number = at + 1;
        match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(record)) => lines.push(scrubbed(record).to_string()),
            Ok(_) => return Err(Error::NotRecord { line: line_number }),
            Err(cause) => {
                return Err(Error::NotJson {
                    line: line_number,
                    cause,
                })
            }
        }
    }
    let records = lines.len();
    Ok((records, lines.join("\n") + "\n"))
}

/// A record with the recorder's folders and Muse's own words taken out.
fn scrubbed(record: Map<String, Value>) -> Value {
    let muse_s_own = is_muse_s_own(&record);
    let mut record = Value::Object(record);
    strings(&mut record, &mut |text| {
        let placed = sockets(&places(text));
        if muse_s_own && placed.len() > LONGEST {
            format!("[scrubbed: {} bytes]", placed.len())
        } else {
            placed
        }
    });
    record
}

/// Whether a record holds Muse's own words: the frame that begins a log (which keeps
/// copies of records), a run event of [`MUSE_S_OWN`], or a context block that no hook
/// wrote.
fn is_muse_s_own(record: &Map<String, Value>) -> bool {
    if record.contains_key("retained_frame") {
        return true;
    }
    let event = record
        .get("payload")
        .and_then(|payload| payload.get("event"));
    let field = |name: &str| {
        event
            .and_then(|event| event.get(name))
            .and_then(Value::as_str)
    };
    match field("kind") {
        Some(kind) if MUSE_S_OWN.contains(&kind) => true,
        Some("context_block_updated") => field("source") != Some("runtime_hook"),
        _ => false,
    }
}

/// Every text in `value`, as `change` changes it.
fn strings(value: &mut Value, change: &mut impl FnMut(&str) -> String) {
    match value {
        Value::String(text) => *text = change(text),
        Value::Array(values) => values.iter_mut().for_each(|value| strings(value, change)),
        Value::Object(fields) => fields.values_mut().for_each(|value| strings(value, change)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// `text` with every place in the macOS temporary folder replaced: the recorder's
/// home becomes [`HOME`], anything else in the folder `/tmp/`. The folder is
/// `/var/folders/<a>/<b>/T/`, written with `/private` before it or not.
fn places(text: &str) -> String {
    const FOLDERS: &str = "/var/folders/";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(FOLDERS) {
        let (before, from) = rest.split_at(at);
        let Some(end) = temporary_folder(from) else {
            out.push_str(&rest[..at + FOLDERS.len()]);
            rest = &rest[at + FOLDERS.len()..];
            continue;
        };
        out.push_str(before.strip_suffix("/private").unwrap_or(before));
        let after = &from[end..];
        rest = match after.strip_prefix(RECORDER_HOME) {
            Some(named) => {
                out.push_str(HOME);
                named.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_')
            }
            None => {
                out.push_str("/tmp/");
                after
            }
        };
    }
    out.push_str(rest);
    out
}

/// `text` with every path of Muse's runtime socket replaced by [`SOCKET`]: a path from
/// `/tmp/tbh-` (with `/private` before it or not) to the first character no path of it
/// has.
fn sockets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(RUNTIME) {
        let (before, from) = rest.split_at(at);
        out.push_str(before.strip_suffix("/private").unwrap_or(before));
        out.push_str(SOCKET);
        let end = from
            .find(|c: char| !(c.is_ascii_alphanumeric() || "/._-".contains(c)))
            .unwrap_or(from.len());
        rest = &from[end..];
    }
    out.push_str(rest);
    out
}

/// Where the temporary folder that `text` begins with ends: after
/// `/var/folders/<a>/<b>/T`, and its `/` when one follows. None where `text` begins
/// with no such folder.
fn temporary_folder(text: &str) -> Option<usize> {
    let mut end = "/var/folders/".len();
    for _ in 0..2 {
        let part = text[end..].find('/')?;
        if part == 0 {
            return None;
        }
        end += part + 1;
    }
    let tail = &text[end..];
    if tail == "T" {
        return Some(end + 1);
    }
    tail.strip_prefix("T/").map(|_| end + 2)
}

#[cfg(test)]
mod tests;
