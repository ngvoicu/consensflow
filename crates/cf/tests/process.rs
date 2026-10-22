//! `cf` as a process: a reader that went away is no failure.

// The tests start cf themselves.
#![allow(clippy::disallowed_methods)]

use std::process::{Command, Stdio};

mod common;

#[test]
fn ends_quietly_and_well_when_its_reader_has_gone() {
    // `cf … | head`: the pipe's reading end is closed before cf writes.
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let mut cf = Command::new(env!("CARGO_BIN_EXE_cf"));
    // The tests run side by side: a `cf` left the other end of another test's pipe would hold it.
    cf_process::hold_only_the_streams(&mut cf);
    let ran = cf
        .arg("help")
        .env("CONSENSFLOW_TOKEN", "tok")
        .env("CONSENSFLOW_TOOL_FOLDERS", "")
        .env_remove("CONSENSFLOW_URL")
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(ran.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ran.stderr), "");
}

/// The `cf` a test starts holds nothing of the test's but its streams: a stand-in started as the
/// tests start `cf` says whether it has a descriptor left open and not close-on-exec.
#[cfg(target_os = "macos")]
#[test]
fn the_cf_a_test_starts_holds_nothing_of_the_tests_but_its_streams() {
    use std::os::fd::AsRawFd;

    use nix::fcntl::{fcntl, FcntlArg, FdFlag};

    let stray = std::fs::File::open("/dev/null").unwrap();
    fcntl(stray.as_raw_fd(), FcntlArg::F_SETFD(FdFlag::empty())).unwrap();
    let test = format!(
        "if [ -e /dev/fd/{} ]; then echo held; else echo clear; fi",
        stray.as_raw_fd()
    );
    let ran = common::cf_at(std::path::Path::new("/bin/sh"), &["-c", &test], &[], "");
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "clear\n");
}
