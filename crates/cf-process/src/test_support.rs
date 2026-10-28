//! What other crates' tests share of this crate's, to show that a program they start holds
//! nothing of theirs but its streams: a descriptor left open and not close-on-exec, as std leaves
//! one for a moment as it makes it, and whether a process holds it.

/// A file of this test's own that it holds open and not close-on-exec: a program started now
/// would be left it. Its name is its own, so that a program is found holding it only where it was
/// left it: a `/dev/null` was the stray before, and a program that opened its own at the same
/// number (node holds one at 11) was taken for one left it (the loopback server, 1 run in 10 on
/// integration B's round 2, T-800). The file goes when the stray is dropped.
#[cfg(target_os = "macos")]
pub struct Stray {
    file: std::fs::File,
    path: std::path::PathBuf,
}

/// The stray ([`Stray`]), open.
#[cfg(target_os = "macos")]
#[allow(clippy::missing_panics_doc, clippy::expect_used)] // A test's own: a failure is the test's.
#[must_use]
pub fn stray() -> Stray {
    use std::os::fd::AsRawFd;
    use std::sync::atomic::{AtomicU64, Ordering};

    use nix::fcntl::{fcntl, FcntlArg, FdFlag};

    static MADE: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "cf-stray-{}-{}",
        std::process::id(),
        MADE.fetch_add(1, Ordering::Relaxed)
    ));
    let stray = std::fs::File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("a file");
    fcntl(stray.as_raw_fd(), FcntlArg::F_SETFD(FdFlag::empty())).expect("its flags");
    Stray { file: stray, path }
}

#[cfg(target_os = "macos")]
impl Stray {
    /// The number this process holds it at.
    #[must_use]
    pub fn fd(&self) -> i32 {
        use std::os::fd::AsRawFd;

        self.file.as_raw_fd()
    }

    /// Whether the process `pid` holds this stray, at any number, as `lsof` says.
    #[allow(clippy::missing_panics_doc, clippy::expect_used)] // A test's own: a failure is the test's.
    #[allow(clippy::disallowed_methods)] // `lsof`, asked what the process holds.
    #[must_use]
    pub fn held_by(&self, pid: u32) -> bool {
        let held = std::process::Command::new("/usr/sbin/lsof")
            .args(["-a", "-p", &pid.to_string(), "-Fn"])
            .output()
            .expect("lsof");
        let name = self
            .path
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        String::from_utf8_lossy(&held.stdout)
            .lines()
            .any(|line| line.starts_with('n') && line.ends_with(&format!("/{name}")))
    }
}

#[cfg(target_os = "macos")]
impl Drop for Stray {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    /// The premise every test of a start's descriptors stands on: a program started bare is left
    /// the stray, and `held_by` says so.
    #[cfg(target_os = "macos")]
    #[test]
    #[allow(clippy::disallowed_methods)] // Started bare: the premise.
    fn a_program_started_bare_holds_the_stray_and_lsof_says_so() {
        let stray = super::stray();
        let mut sleep = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let held = stray.held_by(sleep.id());
        let _ = sleep.kill();
        let _ = sleep.wait();
        assert!(held, "a program started bare does not hold the stray");
    }

    /// A program that opens a `/dev/null` of its own at the number the stray has here is not one
    /// that was left the stray (node holds one at 11, and the loopback server was taken for one
    /// left a `/dev/null` stray whenever the stray was 11: T-800).
    #[cfg(target_os = "macos")]
    #[test]
    #[allow(clippy::disallowed_methods)] // The case's own program, holding only its streams.
    fn a_program_holding_a_file_of_its_own_at_the_strays_number_does_not_hold_the_stray() {
        let stray = super::stray();
        let fd = stray.fd();
        let mut command = std::process::Command::new("/bin/sh");
        command.arg("-c").arg(format!(
            "exec {fd}</dev/null; echo ready; while kill -0 $PPID 2>/dev/null; do /bin/sleep 0.2; done"
        ));
        command.stdout(std::process::Stdio::piped());
        crate::hold_only_the_streams(&mut command);
        let mut program = command.spawn().unwrap();
        let mut ready = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(program.stdout.as_mut().unwrap()),
            &mut ready,
        )
        .unwrap();
        let held = stray.held_by(program.id());
        let _ = program.kill();
        let _ = program.wait();
        assert_eq!(ready, "ready\n");
        assert!(!held, "a /dev/null of its own was taken for the stray");
    }
}
