//! Ending a process this program started.

/// How a process is asked to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// SIGTERM: it may still tidy up.
    Asked,
    /// SIGKILL: it ends at once.
    Forced,
}

/// What an ending is sent to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reach {
    /// The process alone.
    Process,
    /// The process and what it started: on Unix the process group the process
    /// leads (a program `capture` runs does), so that an installer's children
    /// end with it; Windows ends the whole tree whichever is asked.
    Group,
}

/// Ends the process `pid`, one this program started and has not yet reaped:
/// on Unix with the signal `how` names; on Windows, which has no signals,
/// with its whole tree killed (`taskkill /T /F`) whatever `how` asks. On
/// Windows the call returns once `taskkill` has.
pub fn terminate(pid: u32, how: Ending) {
    end(pid, how, Reach::Process, true);
}

/// Ends what `pid` is the leader of, as `reach` says, one this program
/// started and has not yet reaped: while it is not reaped its id, and its
/// group's, are held, and name no other. On Windows `taskkill` is waited for
/// if `wait`, and else started and let go: a program on its way out must not
/// spend its last moments on a child's end, one `taskkill` after another.
pub(crate) fn end(pid: u32, how: Ending, reach: Reach, wait: bool) {
    #[cfg(unix)]
    {
        // Signalling waits for nothing.
        let _ = wait;
        let Some(target) = i32::try_from(pid).ok().and_then(|pid| target(pid, reach)) else {
            return;
        };
        let signal = match how {
            Ending::Asked => libc::SIGTERM,
            Ending::Forced => libc::SIGKILL,
        };
        #[allow(unsafe_code)]
        // SAFETY: kill takes two integers and touches no memory of ours; the
        // pid is a child of this process not yet reaped, so it names no other,
        // and a group it leads is held by it as long as it is not reaped.
        unsafe {
            libc::kill(target, signal);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = (how, reach);
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

/// What `kill` is given to reach `pid` as `reach` says: a negative number
/// names the group of that id. None where the number would name more than
/// asked: group 0 is this process's own, 1 is none, and -1 is every process.
#[cfg(unix)]
fn target(pid: i32, reach: Reach) -> Option<i32> {
    match reach {
        Reach::Process => Some(pid),
        Reach::Group if pid > 1 => Some(-pid),
        Reach::Group => None,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::testing::{gone_within, pids_written_to, Survivors};
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::time::Duration;

    #[test]
    fn a_group_is_named_by_its_leader_and_never_by_a_number_that_names_more() {
        assert_eq!(target(1234, Reach::Process), Some(1234));
        assert_eq!(target(1234, Reach::Group), Some(-1234));
        for pid in [i32::MIN, -1, 0, 1] {
            assert_eq!(target(pid, Reach::Group), None, "{pid}");
        }
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it ends.
    fn a_group_is_ended_whole_and_a_process_alone_is_not() {
        let dir = tempfile::tempdir().unwrap();
        for (reach, left_behind) in [(Reach::Process, true), (Reach::Group, false)] {
            // A shell that leads a group, starts a sleep, and waits for it:
            // the sleep's pid is written down.
            let file = dir.path().join(format!("{reach:?}"));
            let mut shell = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg("sleep 30 & echo $! > \"$0\"; wait")
                .arg(&file)
                .process_group(0)
                .spawn()
                .unwrap();
            let sleep = pids_written_to(&file, 1)[0];
            // What the process alone leaves is ended as the test ends.
            let _left = Survivors(vec![sleep]);
            end(shell.id(), Ending::Forced, reach, false);
            assert_eq!(shell.wait().unwrap().signal(), Some(libc::SIGKILL));
            let within = Duration::from_millis(if left_behind { 300 } else { 20_000 });
            assert_eq!(gone_within(sleep, within), !left_behind, "{reach:?}");
        }
    }

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
            end(child.id(), how, Reach::Process, false);
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
        end(child.id(), Ending::Forced, Reach::Process, false);
        assert!(started.elapsed() < Duration::from_secs(5));
        // `taskkill` goes on without anyone waiting for it: the child ends.
        assert!(child.wait().is_ok());
        assert!(started.elapsed() < Duration::from_secs(20));
    }
}
