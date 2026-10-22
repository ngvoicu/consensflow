//! What the app's tests share of starting programs: a command that holds nothing of the test
//! process's but its streams. The tests run side by side and open sockets and pipes; on macOS
//! std makes each close-on-exec in a second step, and a program a test started between the two
//! would hold what another test closed. The tests that start a program bare say why.

use std::ffi::OsStr;
use std::process::Command;

/// The command a test starts `program` with, holding only its streams.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    cf_process::hold_only_the_streams(&mut command);
    command
}

#[cfg(test)]
mod tests;
