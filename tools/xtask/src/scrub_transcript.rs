//! `cargo xtask scrub-transcript <transcript.jsonl> <fixture.jsonl>` (landing S14.4):
//! a fixture of a Claude transcript that a live check kept
//! (`cargo xtask live stops --transcripts <folder>`): every record stays, in its
//! order, and what is personal or opaque in it is replaced. The reader of the
//! records and the tests built on them read what is left, which is each record's
//! type, ids, time and conversation. It is how the files of
//! `crates/cf-harness/tests/fixtures/claude` were made, and
//! `crates/cf-harness/tests/fixtures.rs` holds them to being so.
//!
//! What goes:
//!
//! - the session id, which becomes `$SESSION` (a test puts its own there), wherever
//!   in a record it is written;
//! - the folder the window ran in (`/work/app`), the git branch (`main`);
//! - an account's, organization's and bridge's ids;
//! - what an attachment carried (a hook's command and output, the skills, the
//!   environment, the instructions, the e-mail address of the user): only its type,
//!   and a hook's name, event, exit code and duration, are kept;
//! - the request ids the API gave, and the signature of a thinking block.
//!
//! The records a message is made of (the user's words, the assistant's, its model
//! and usage) stay as Claude wrote them, in the order and the spelling it wrote
//! them (a record is read and written as JSON, its keys where they were).

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};

use crate::context::Context;
use crate::dispatch::{Command, Console, Failure, Run};

pub const COMMANDS: &[Command] = &[Command {
    words: &["scrub-transcript"],
    about: "Take out of a Claude transcript what identifies a person, to make a fixture of it",
    usage: "<transcript.jsonl> <fixture.jsonl>",
    run: Run::Native(scrub_transcript),
}];

/// The folder of the user's, and what stands for it.
const FOLDER: &str = "/work/app";

/// What names an account or its bridge: the fields that are opaque ids of one.
const OPAQUE: [&str; 3] = [
    "bridgeSessionId",
    "ownerAccountUuid",
    "ownerOrganizationUuid",
];

/// What replaces what is taken out.
const SCRUBBED: &str = "[scrubbed]";

/// What a hook's attachment keeps of its run.
const HOOK_FACTS: [&str; 5] = [
    "hookName",
    "hookEvent",
    "exitCode",
    "durationMs",
    "timedOut",
];

/// Why a transcript was not made a fixture.
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
    #[error("no record names its session")]
    NoSession,
}

fn scrub_transcript(_: &Context, args: &[OsString], console: &mut Console) -> Result<i32, Failure> {
    let [from, to] = args else {
        return Err(Failure::Usage(
            "scrub-transcript takes the transcript to scrub and the fixture to make: \
             <transcript.jsonl> <fixture.jsonl>"
                .into(),
        ));
    };
    match make(Path::new(from), Path::new(to)) {
        Ok(said) => {
            writeln!(console.out, "{said}")?;
            Ok(0)
        }
        Err(error) => {
            writeln!(console.err, "scrub-transcript: {error}")?;
            Ok(1)
        }
    }
}

/// Makes the fixture `to` of the transcript `from`, and says what was kept where.
fn make(from: &Path, to: &Path) -> Result<String, Error> {
    let transcript = fs::read_to_string(from).map_err(|cause| Error::Read {
        path: from.display().to_string(),
        cause,
    })?;
    let fixture = scrub(&transcript)?;
    fs::write(to, &fixture.text).map_err(|cause| Error::Write {
        path: to.display().to_string(),
        cause,
    })?;
    Ok(format!(
        "{} records of {} kept in {}",
        fixture.records,
        fixture.session,
        to.display()
    ))
}

/// A transcript made a fixture.
#[derive(Debug, PartialEq, Eq)]
struct Fixture {
    /// How many records it holds.
    records: usize,
    /// The session the records are of, which is no longer named in them.
    session: String,
    /// The records, a line each.
    text: String,
}

/// The fixture of `transcript`: its records (a line each, blank lines left out),
/// each scrubbed. A line that is none is said by its number in the transcript.
fn scrub(transcript: &str) -> Result<Fixture, Error> {
    let mut records = Vec::new();
    for (at, line) in transcript.split('\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let line_number = at + 1;
        match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(record)) => records.push(record),
            Ok(_) => return Err(Error::NotRecord { line: line_number }),
            Err(cause) => {
                return Err(Error::NotJson {
                    line: line_number,
                    cause,
                })
            }
        }
    }
    // The first record that names one, as the JavaScript found it: its words, even empty ones.
    let session = records
        .iter()
        .find_map(|record| record.get("sessionId").and_then(Value::as_str))
        .filter(|session| !session.is_empty())
        .ok_or(Error::NoSession)?
        .to_owned();
    let count = records.len();
    let lines: Vec<String> = records
        .into_iter()
        .map(|record| scrubbed(record).replace(&session, "$SESSION"))
        .collect();
    Ok(Fixture {
        records: count,
        session,
        text: lines.join("\n") + "\n",
    })
}

/// A record without what is personal in it, as the text of a line.
fn scrubbed(mut record: Map<String, Value>) -> String {
    let replace = |record: &mut Map<String, Value>, name: &str, with: Value| {
        if let Some(value) = record.get_mut(name) {
            *value = with;
        }
    };
    for name in ["sessionId", "session_id"] {
        replace(&mut record, name, json!("$SESSION"));
    }
    replace(&mut record, "cwd", json!(FOLDER));
    replace(&mut record, "gitBranch", json!("main"));
    for name in OPAQUE {
        replace(&mut record, name, json!(SCRUBBED));
    }
    replace(&mut record, "requestId", json!("req_scrubbed"));
    let kept = record
        .get("attachment")
        .filter(|found| truthy(found))
        .map(attachment);
    if let Some(kept) = kept {
        record.insert("attachment".into(), kept);
        // What was rendered of it carried what it did.
        for name in ["rendered", "renderedRole", "renderedBesideToolResult"] {
            record.shift_remove(name);
        }
    }
    for name in ["lastPrompt", "aiTitle"] {
        if record.get(name).is_some_and(Value::is_string) {
            replace(&mut record, name, json!(SCRUBBED));
        }
    }
    let blocks = record
        .get_mut("message")
        .and_then(|message| message.get_mut("content"))
        .and_then(Value::as_array_mut);
    for block in blocks.into_iter().flatten() {
        if let Some(signature) = block.get_mut("signature") {
            *signature = json!(SCRUBBED);
        }
    }
    Value::Object(record).to_string()
}

/// Whether JavaScript takes the value for true: what an `if (attachment)` does.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(truth) => *truth,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// What an attachment keeps: its type, and of a hook's the facts of its run.
fn attachment(found: &Value) -> Value {
    let mut kept = Map::new();
    let kind = found.get("type");
    if let Some(kind) = kind {
        kept.insert("type".into(), kind.clone());
    }
    if matches!(
        kind.and_then(Value::as_str),
        Some("hook_success" | "hook_cancelled")
    ) {
        for name in HOOK_FACTS {
            if let Some(fact) = found.get(name) {
                kept.insert(name.into(), fact.clone());
            }
        }
        kept.insert("command".into(), json!(SCRUBBED));
    }
    Value::Object(kept)
}

#[cfg(test)]
mod tests;
