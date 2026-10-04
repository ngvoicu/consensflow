//! `cf` as a process: a reader that went away is no failure, and a command
//! handed to the CLI's Node sources keeps its arguments and its exit code.

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

#[cfg(unix)]
#[test]
fn hands_a_command_it_does_not_answer_to_the_cli_beside_it_arguments_and_exit_code_whole() {
    let dir = tempfile::tempdir().unwrap();
    let cf = dir.path().join("cf");
    std::fs::copy(env!("CARGO_BIN_EXE_cf"), &cf).unwrap();
    // The CLI beside it, as the runtime the app names runs it: here sh, a script.
    std::fs::write(dir.path().join("cf.mjs"), "printf '%s|' \"$@\"; exit 3\n").unwrap();
    let ran = Command::new(&cf)
        .args(["catalog", "--harness", "two words"])
        .env_remove("CONSENSFLOW_TOKEN")
        .env("CONSENSFLOW_NODE", "/bin/sh")
        .output()
        .unwrap();
    assert_eq!(ran.status.code(), Some(3));
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout),
        "catalog|--harness|two words|"
    );
}
