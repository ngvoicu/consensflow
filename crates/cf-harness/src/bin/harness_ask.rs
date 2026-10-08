//! What the evals and the live tools ask of the harness code
//! (`cf_harness::tooling`), for `tests/rust-harness.mjs`. It is no adapter,
//! and ships with nothing: it is built only with the `test-support` feature.
//!
//! The question is one JSON object on the first line of stdin, as `common`
//! says, and `tooling` says what each `op` asks and answers.

#![forbid(unsafe_code)]

mod common;

use cf_harness::tooling::answer;
use common::serve;

fn main() -> std::process::ExitCode {
    serve(|asked| async move { answer(&asked) })
}
