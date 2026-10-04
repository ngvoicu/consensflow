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
/// with its whole tree killed (`taskkill /T /F`) whatever `how` asks.
pub fn terminate(pid: u32, how: Ending) {
    #[cfg(unix)]
    {
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
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
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
}
