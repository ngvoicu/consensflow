//! Ending a process this program started.

use crate::job::Job;

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
    /// end with it; Windows ends the whole job the program runs in whichever is
    /// asked.
    Group,
}

/// Ends the process `pid`, one this program started and has not yet reaped:
/// on Unix with the signal `how` names; on Windows, which has no signals,
/// with its whole tree killed (`taskkill /T /F`) whatever `how` asks. On
/// Windows the call returns once `taskkill` has. A process started through
/// this crate is ended by its owner (`Child::terminate`, `Ender::force`,
/// `Owned::end`), which holds the job it runs in: `taskkill` ends what it has
/// read of a tree, and leaves what the tree starts meanwhile (zeewin, T-322)
/// and what has lost its parent.
pub fn terminate(pid: u32, how: Ending) {
    end(pid, how, Reach::Process, true, &Job::unheld());
}

/// Ends what `pid` is the leader of, as `reach` says, one this program
/// started and has not yet reaped: while it is not reaped its id, and its
/// group's, are held, and name no other. On Windows the program's `job` is
/// ended, whole and at once: everything in it, whatever is started as it ends,
/// and nothing is waited for. A program with no job (none could be made for it,
/// or only its pid is known) is ended by `taskkill /T /F`, waited for if `wait`,
/// and else started and let go: a program on its way out must not spend its
/// last moments on a child's end, one `taskkill` after another.
pub(crate) fn end(pid: u32, how: Ending, reach: Reach, wait: bool, job: &Job) {
    #[cfg(unix)]
    {
        // Signalling waits for nothing but what a group forced to end is looked at again for
        // (`force_stragglers`), and a group holds what a job would.
        let _ = (wait, job);
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
        #[cfg(test)]
        AFTER_SIGNAL.with(|hook| {
            if let Some(hook) = &*hook.borrow() {
                hook();
            }
        });
        if how == Ending::Forced && reach == Reach::Group {
            force_stragglers(target);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = (how, reach);
        if job.terminate() {
            return;
        }
        // Started outside the lock every other start of this crate is under
        // (`spawning`): Windows has no pipe made in two steps for a start to inherit, and
        // `taskkill` has no stream of this process's, all three being null.
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

/// How long a group forced to end is looked at again for what was started as the signal went
/// round: a program that keeps starting programs is in the middle of starting one when it is
/// sent the signal, and the one it was starting is in the group, and was not there to be sent it.
/// The program ends of it, and starts no more; this long is for it to be scheduled to (T-581:
/// at a load of 100, after the crate's tests had ended a program that kept starting `sleep`s, one
/// of them was found running, orphaned, in the group the program had led).
#[cfg(unix)]
const STRAGGLERS_WITHIN: std::time::Duration = std::time::Duration::from_millis(500);

/// Forces the group `target` names (`-pgid`) again for as long as a process of it that is not
/// a zombie is there, for [`STRAGGLERS_WITHIN`] at most. The group is held by its leader, which
/// the caller has not reaped: while it is not, no number it leads is another's.
#[cfg(unix)]
fn force_stragglers(target: i32) {
    let until = std::time::Instant::now() + STRAGGLERS_WITHIN;
    while !crate::table::in_group(-target).is_empty() && std::time::Instant::now() < until {
        std::thread::sleep(crate::table::BETWEEN_PASSES);
        #[allow(unsafe_code)]
        // SAFETY: as above: two integers, and the group is still held by its unreaped leader.
        unsafe {
            libc::kill(target, libc::SIGKILL);
        }
    }
}

#[cfg(all(test, unix))]
thread_local! {
    /// What a test does right after a signal went round (a program started as it did).
    static AFTER_SIGNAL: std::cell::RefCell<Option<Box<dyn Fn()>>> =
        const { std::cell::RefCell::new(None) };
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
    use crate::spawning::StartedUnderTheLock as _;
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
    fn a_group_is_ended_whole_and_a_process_alone_is_not() {
        let dir = tempfile::tempdir().unwrap();
        for (reach, left_behind) in [(Reach::Process, true), (Reach::Group, false)] {
            // A shell that leads a group, starts a sleep, and waits for it:
            // the sleep's pid is written down.
            let file = dir.path().join(format!("{reach:?}"));
            let mut shell = crate::testing::command("/bin/sh")
                .arg("-c")
                .arg("sleep 30 & echo $! > \"$0\"; wait")
                .arg(&file)
                .process_group(0)
                .spawn_locked()
                .unwrap();
            let sleep = pids_written_to(&file, 1)[0];
            // What the process alone leaves is ended as the test ends.
            let _left = Survivors(vec![sleep]);
            end(shell.id(), Ending::Forced, reach, false, &Job::unheld());
            assert_eq!(shell.wait().unwrap().signal(), Some(libc::SIGKILL));
            let within = Duration::from_millis(if left_behind { 300 } else { 20_000 });
            assert_eq!(gone_within(sleep, within), !left_behind, "{reach:?}");
        }
    }

    #[test]
    fn a_program_started_as_the_group_was_forced_to_end_is_forced_to_end_too() {
        // As a program that keeps starting programs is when it is sent the signal (T-581): the
        // one it was starting is in the group, and the signal did not find it there. A shell that
        // leads a group and waits for a sleep is forced to end; as the signal has gone round, a
        // sleep is started in its group, which the leader, a zombie not yet reaped, still holds.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pid");
        let mut shell = crate::testing::command("/bin/sh")
            .arg("-c")
            .arg("sleep 30 & echo $! > \"$0\"; wait")
            .arg(&file)
            .process_group(0)
            .spawn_locked()
            .unwrap();
        let sleep = pids_written_to(&file, 1)[0];
        let leader = shell.id();
        let started: std::rc::Rc<std::cell::RefCell<Vec<std::process::Child>>> = Default::default();
        let kept = std::rc::Rc::clone(&started);
        AFTER_SIGNAL.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                let late = crate::testing::command("/bin/sleep")
                    .arg("30")
                    .process_group(i32::try_from(leader).unwrap())
                    .spawn_locked()
                    .unwrap();
                kept.borrow_mut().push(late);
            }));
        });
        end(leader, Ending::Forced, Reach::Group, false, &Job::unheld());
        AFTER_SIGNAL.with(|hook| *hook.borrow_mut() = None);
        let mut late = started
            .borrow_mut()
            .pop()
            .expect("a program was started in the group");
        let _left = Survivors(vec![sleep, late.id()]);
        assert_eq!(shell.wait().unwrap().signal(), Some(libc::SIGKILL));
        let until = std::time::Instant::now() + Duration::from_secs(10);
        let ended = loop {
            if let Some(status) = late.try_wait().unwrap() {
                break Some(status);
            }
            if std::time::Instant::now() >= until {
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            ended.map(|status| status.signal()),
            Some(Some(libc::SIGKILL)),
            "the program started as the signal went round was not forced to end"
        );
    }

    #[test]
    fn ends_a_child_with_the_signal_asked_for() {
        for (how, signal) in [
            (Ending::Asked, libc::SIGTERM),
            (Ending::Forced, libc::SIGKILL),
        ] {
            let mut child = crate::testing::command("sleep")
                .arg("30")
                .spawn_locked()
                .unwrap();
            terminate(child.id(), how);
            assert_eq!(child.wait().unwrap().signal(), Some(signal), "{how:?}");
        }
    }

    #[test]
    fn the_end_that_waits_for_nothing_ends_a_child_just_the_same() {
        for (how, signal) in [
            (Ending::Asked, libc::SIGTERM),
            (Ending::Forced, libc::SIGKILL),
        ] {
            let mut child = crate::testing::command("sleep")
                .arg("30")
                .spawn_locked()
                .unwrap();
            end(child.id(), how, Reach::Process, false, &Job::unheld());
            assert_eq!(child.wait().unwrap().signal(), Some(signal), "{how:?}");
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::spawning::StartedUnderTheLock as _;
    use std::time::{Duration, Instant};

    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it ends.
    fn the_end_that_waits_for_nothing_returns_before_the_child_is_gone() {
        // Bare: a Windows test, and Windows hands a program no descriptor it is not asked to.
        let mut child = std::process::Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn_locked()
            .unwrap();
        let started = Instant::now();
        end(
            child.id(),
            Ending::Forced,
            Reach::Process,
            false,
            &Job::unheld(),
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        // `taskkill` goes on without anyone waiting for it: the child ends.
        assert!(child.wait().is_ok());
        assert!(started.elapsed() < Duration::from_secs(20));
    }
}
