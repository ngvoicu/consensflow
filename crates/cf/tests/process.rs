//! `cf` as a process: a reader that went away is no failure.

// The tests start cf themselves.
#![allow(clippy::disallowed_methods)]

use std::process::{Command, Stdio};

#[test]
fn ends_quietly_and_well_when_its_reader_has_gone() {
    // `cf … | head`: the pipe's reading end is closed before cf writes.
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let ran = Command::new(env!("CARGO_BIN_EXE_cf"))
        .arg("help")
        .env("CONSENSFLOW_TOKEN", "tok")
        .env_remove("CONSENSFLOW_URL")
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(ran.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ran.stderr), "");
}
