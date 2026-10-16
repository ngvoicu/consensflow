//! What the live tests ask of the engine's texts (`cf_engine::testing::ask`), through
//! `live::ask` of `crates/cf-e2e`. It is no part of the daemon, and ships with nothing:
//! it is built only with the `test-support` feature.
//!
//! The question is one JSON object on the first line of stdin, and `testing::ask`
//! says what each `op` asks and answers. The last line of stdout is
//! `{"answered": value}`, or `{"threw": message}` for a failure, and the exit status
//! is then 1, as `harness-ask` answers.

#![forbid(unsafe_code)]

use std::io::{BufRead, Write};
use std::process::ExitCode;

use cf_engine::testing::ask::answer;
use serde_json::{json, Value};

/// The question on the first line of stdin.
fn question() -> Result<Value, String> {
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|cause| cause.to_string())?;
    serde_json::from_str(&line).map_err(|cause| cause.to_string())
}

fn main() -> ExitCode {
    let (last, code) = match question().and_then(|asked| answer(&asked)) {
        Ok(answered) => (json!({ "answered": answered }), ExitCode::SUCCESS),
        Err(threw) => (json!({ "threw": threw }), ExitCode::FAILURE),
    };
    match writeln!(std::io::stdout(), "{last}") {
        Ok(()) => code,
        Err(_) => ExitCode::FAILURE,
    }
}
