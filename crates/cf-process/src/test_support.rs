//! What other crates' tests share of this crate's, to show that a program they start holds
//! nothing of theirs but its streams: a descriptor left open and not close-on-exec, as std leaves
//! one for a moment as it makes it, and whether a process holds it.

/// A `/dev/null` this process holds, open and not close-on-exec: a program started now would be
/// left it.
#[cfg(target_os = "macos")]
#[allow(clippy::missing_panics_doc, clippy::expect_used)] // A test's own: a failure is the test's.
#[must_use]
pub fn stray() -> std::fs::File {
    use std::os::fd::AsRawFd;

    use nix::fcntl::{fcntl, FcntlArg, FdFlag};

    let stray = std::fs::File::open("/dev/null").expect("a file");
    fcntl(stray.as_raw_fd(), FcntlArg::F_SETFD(FdFlag::empty())).expect("its flags");
    stray
}

/// Whether the process `pid` holds `/dev/null` at `fd`, as `lsof` says.
#[cfg(target_os = "macos")]
#[allow(clippy::missing_panics_doc, clippy::expect_used)] // A test's own: a failure is the test's.
#[allow(clippy::disallowed_methods)] // `lsof`, asked what the process holds.
#[must_use]
pub fn holds_null(pid: u32, fd: i32) -> bool {
    let held = std::process::Command::new("/usr/sbin/lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", &fd.to_string()])
        .output()
        .expect("lsof");
    String::from_utf8_lossy(&held.stdout).contains("/dev/null")
}

#[cfg(test)]
mod tests {
    /// The premise every test of a start's descriptors stands on: a program started bare is left
    /// the stray, and `holds_null` says so.
    #[cfg(target_os = "macos")]
    #[test]
    #[allow(clippy::disallowed_methods)] // Started bare: the premise.
    fn a_program_started_bare_holds_the_stray_and_lsof_says_so() {
        use std::os::fd::AsRawFd;

        let stray = super::stray();
        let mut sleep = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let held = super::holds_null(sleep.id(), stray.as_raw_fd());
        let _ = sleep.kill();
        let _ = sleep.wait();
        assert!(held, "a program started bare does not hold the stray");
    }
}
