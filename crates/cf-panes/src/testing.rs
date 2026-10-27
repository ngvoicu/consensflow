//! What the pane host's tests share of starting programs: a command that holds nothing of the
//! test process's but its streams. The tests run side by side and open sockets, pipes and
//! terminals; on macOS each is made close-on-exec in a second step, and a program a test started
//! between the two would hold what another test closed (a terminal that never hangs up).

use std::ffi::OsStr;
use std::process::Command;

/// The command a test starts `program` with, holding only its streams.
#[allow(clippy::disallowed_methods)] // A test's own programs.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    cf_process::hold_only_the_streams(&mut command);
    command
}

/// A program a test starts holds nothing of the test's but its streams: here a descriptor is
/// left open and not close-on-exec as it starts, and it says whether it has it.
#[cfg(target_os = "macos")]
#[test]
fn a_program_a_test_starts_holds_nothing_of_the_tests_but_its_streams() {
    use std::os::fd::AsRawFd;

    let stray = std::fs::File::open("/dev/null").expect("a file");
    let fd = stray.as_raw_fd();
    // SAFETY: `fcntl` is asked to clear the flags of a descriptor this test holds.
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
    let said = command("/bin/sh")
        .args([
            "-c".to_owned(),
            format!("if [ -e /dev/fd/{fd} ]; then echo held; else echo clear; fi"),
        ])
        .output()
        .expect("the shell runs");
    assert_eq!(String::from_utf8_lossy(&said.stdout), "clear\n");
}
