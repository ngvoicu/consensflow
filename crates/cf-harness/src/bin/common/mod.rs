//! What the test binaries that `tests/rust-channels.mjs` and
//! `tests/rust-harness.mjs` run have in common: a question on the first line
//! of stdin, and the answer as the last line of stdout. They are no adapters,
//! and ship with nothing: they are built only with the `test-support` feature.
//!
//! The last line out is `{"answered": value}`, as JavaScript returned it, or
//! `{"threw": message}` for a failure JavaScript threw, and the exit status is
//! then 1. The binaries that send through a channel also put the pane host to
//! the process that started them, which `pane_host` is, and only they have it.

use std::future::Future;
use std::io::{BufRead, Write};
use std::process::ExitCode;

use serde_json::{json, Value};

/// The next line of stdin, as JSON.
pub fn line() -> Result<Value, String> {
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|cause| cause.to_string())?;
    serde_json::from_str(&line).map_err(|cause| cause.to_string())
}

/// A line of stdout.
pub fn say(line: &Value) -> std::io::Result<()> {
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
