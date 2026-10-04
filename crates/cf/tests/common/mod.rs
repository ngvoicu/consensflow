//! `cf` as the tests run it: the binary, with none of ConsensFlow's
//! variables from a window this test may itself run in, and only those the
//! case gives.

// The tests' own helper: a failure in it is the test's.
#![allow(clippy::expect_used)]

use std::io::Write;
use std::process::{Command, Output, Stdio};

/// `cf args` with `env` added and `input` on its standard input.
#[allow(clippy::disallowed_methods)] // The tests start cf themselves.
pub fn cf<S: AsRef<str>>(args: &[S], env: &[(&str, &str)], input: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cf"));
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy();
        if ["CONSENSFLOW_", "CF_", "CHISEL_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            command.env_remove(&*name);
        }
    }
    let mut child = command
        .args(args.iter().map(AsRef::as_ref))
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cf starts");
    let mut stdin = child.stdin.take().expect("its standard input");
    // A command that reads no input may have ended before it is written.
    let _ = stdin.write_all(input.as_bytes());
    drop(stdin);
    child.wait_with_output().expect("cf ends")
}
