//! What the test binaries that `tests/rust-channels.mjs` runs have in common:
//! a question on the first line of stdin, the pane host asked of the process
//! that started them, and the answer as the last line of stdout. They are no
//! adapters, and ship with nothing: they are built only with the
//! `test-support` feature.
//!
//! The pane host is the process on the other end (`Asking`). A request to it
//! is a line `{"ask": {"op", "body"}}` on stdout, and its answer one line on
//! stdin: `{"answer": value}`, or `{"throws": message, "error": word}` for a
//! host that never answered. The last line out is `{"answered": value}`, as
//! JavaScript returned it, or `{"threw": message}` for a failure JavaScript
//! threw, and the exit status is then 1.

// Each test binary uses the part of this that it needs.
#![allow(dead_code)]

use std::future::Future;
use std::io::{BufRead, Write};
use std::process::ExitCode;

use cf_harness::contract::{HostError, Pane, PaneHost, Work};
use serde_json::{json, Value};

/// A text field of `object`.
pub fn text<'a>(object: &'a Value, name: &str) -> Result<&'a str, String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("the question names no {name}"))
}

/// The pane a send is claimed through.
pub fn pane(asked: &Value) -> Result<Pane, String> {
    Ok(Pane {
        id: text(asked, "pane")?.to_owned(),
        generation: asked
            .get("generation")
            .and_then(Value::as_u64)
            .ok_or("the question names no generation")?,
    })
}

/// The pane host: the process that started this one, asked for each request
/// and waited for. A send asks for its claim before it begins any wait of its
/// own, so nothing else is held up while this one waits.
pub struct Asking;

impl PaneHost for Asking {
    fn request<'a>(&'a self, op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>> {
        Box::pin(async move { ask(op, &body) })
    }
}

/// One request to the process that started this one, and its answer.
fn ask(op: &str, body: &Value) -> Result<Value, HostError> {
    let failed = |cause: &dyn std::fmt::Display| HostError {
        error: None,
        message: cause.to_string(),
    };
    say(&json!({ "ask": { "op": op, "body": body } })).map_err(|cause| failed(&cause))?;
    let said = line().map_err(|cause| failed(&cause))?;
    match said.get("throws").and_then(Value::as_str) {
        Some(message) => Err(HostError {
            error: said.get("error").and_then(Value::as_str).map(str::to_owned),
            message: message.to_owned(),
        }),
        None => Ok(said.get("answer").cloned().unwrap_or(Value::Null)),
    }
}

/// The next line of stdin, as JSON.
fn line() -> Result<Value, String> {
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|cause| cause.to_string())?;
    serde_json::from_str(&line).map_err(|cause| cause.to_string())
}

/// A line of stdout.
fn say(line: &Value) -> std::io::Result<()> {
    writeln!(std::io::stdout(), "{line}")
}

/// The question on the first line of stdin, run to its answer on a runtime of
/// its own, which is the last line of stdout.
pub fn serve<Answer>(run: impl FnOnce(Value) -> Answer) -> ExitCode
where
    Answer: Future<Output = Result<Value, String>>,
{
    let outcome = line().and_then(|asked| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|cause| cause.to_string())?;
        runtime.block_on(run(asked))
    });
    let (last, code) = match outcome {
        Ok(answered) => (json!({ "answered": answered }), ExitCode::SUCCESS),
        Err(threw) => (json!({ "threw": threw }), ExitCode::FAILURE),
    };
    match say(&last) {
        Ok(()) => code,
        Err(_) => ExitCode::FAILURE,
    }
}
