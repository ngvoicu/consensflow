//! Ending a process this program started.

/// How a process is asked to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// SIGTERM: it may still tidy up.
    Asked,
    /// SIGKILL: it ends at once.
    Forced,
}

/// Ends the process `pid`, one this program started and has not yet reaped:
/// on Unix with the signal `how` names; on Windows, which has no signals,
/// with its whole tree killed (`taskkill /T /F`) whatever `how` asks. On
/// Windows the call returns once `taskkill` has.
pub fn terminate(pid: u32, how: Ending) {
    end(pid, how, true);
}

/// Ends the process `pid` as [`terminate`] does, and waits for nothing: on
/// Windows `taskkill` is started and let go, where [`terminate`] waits for
/// it. For a program on its way out, which must not spend its last moments
/// on a child's end, one `taskkill` after another.
pub fn terminate_without_waiting(pid: u32, how: Ending) {
    end(pid, how, false);
}

fn end(pid: u32, how: Ending, wait: bool) {
    #[cfg(unix)]
    {
        // Signalling waits for nothing.
        let _ = wait;
        let Ok(pid) = i32::try_from(pid) else {
            return;
        };
        let signal = match how {
            Ending::Asked => libc::SIGTERM,
            Ending::Forced => libc::SIGKILL,
        };
        #[allow(unsafe_code)]
        // SAFETY: kill takes two integers and touches no memory of ours; the
        // pid is a child of this process not yet reaped, so it names no other.
        unsafe {
            libc::kill(pid, signal);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = how;
        #[allow(clippy::disallowed_methods)] // The one place a process starts.
        let mut command = std::process::Command::new("taskkill");
        command
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        if wait {
            let _ = command.status();
        } else {
            // Not waited for: the handle is let go, and `taskkill` goes on.
            let _ = command.spawn();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it ends.
    fn ends_a_child_with_the_signal_asked_for() {
        for (how, signal) in [
            (Ending::Asked, libc::SIGTERM),
            (Ending::Forced, libc::SIGKILL),
        ] {
            let mut child = std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap();
            terminate(child.id(), how);
            assert_eq!(child.wait().unwrap().signal(), Some(signal), "{how:?}");
        }
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it ends.
    fn the_end_that_waits_for_nothing_ends_a_child_just_the_same() {
        for (how, signal) in [
            (Ending::Asked, libc::SIGTERM),
            (Ending::Forced, libc::SIGKILL),
        ] {
            let mut child = std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap();
            terminate_without_waiting(child.id(), how);
            assert_eq!(child.wait().unwrap().signal(), Some(signal), "{how:?}");
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it ends.
    fn the_end_that_waits_for_nothing_returns_before_the_child_is_gone() {
        let mut child = std::process::Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        terminate_without_waiting(child.id(), Ending::Forced);
        assert!(started.elapsed() < Duration::from_secs(5));
        // `taskkill` goes on without anyone waiting for it: the child ends.
        assert!(child.wait().is_ok());
        assert!(started.elapsed() < Duration::from_secs(20));
    }
}
