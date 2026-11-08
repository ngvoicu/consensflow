//! What the app's tests share of starting programs: a command that holds nothing of the test
//! process's but its streams. The tests run side by side and open sockets and pipes; on macOS
//! std makes each close-on-exec in a second step, and a program a test started between the two
//! would hold what another test closed. The tests that start a program bare say why.

use std::ffi::OsStr;
use std::process::Command;

/// The test `name` run again in a child process of its own, with `marker` set in the child's
/// environment (the child knows itself by it), and what it said. For a test of what is the
/// whole process's: a TLS provider, which a test side by side installs for the rest.
pub(crate) fn in_a_child(name: &str, marker: &str) -> std::process::Output {
    command(std::env::current_exe().expect("the test binary"))
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(marker, "1")
        .env_remove("RUST_BACKTRACE")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run the test in a child")
}

/// What a child said, its output and its errors one after the other.
pub(crate) fn said(ran: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    )
}

/// The command a test starts `program` with, holding only its streams.
#[allow(clippy::disallowed_methods)] // The tests' one start of a program, holding only its streams.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    cf_process::hold_only_the_streams(&mut command);
    command
}

#[cfg(test)]
mod tests;
