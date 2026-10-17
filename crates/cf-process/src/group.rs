//! What a program run to its end (`capture`, and `execute` with it) leads: on
//! Unix a process group of its own, so that ending it ends what it started
//! too (the children of Homebrew and of npm, which an update runs through),
//! not the program alone. Windows has no groups: `taskkill /T` ends a tree,
//! and the job ends every child this process has when this process ends.
//!
//! The group is the program's to end until the program has been waited for,
//! and not after: a program that is waited for is reaped, and the id it led
//! the group by is free for the system to give to another as soon as nothing
//! is left of the group. Until then every way the program ends reaches the
//! group: this process's exit path (the [`Ender`] handed out), its time
//! running out, and the wait for it dropped. What the program left running
//! once it was waited for is let go of, as with a program that exited after
//! it detached its children.
//!
//! A program held whole (`capture_whole`) leads a session of its own on Unix,
//! and its group with it: every process it starts is in that session, though it
//! is put in a group of its own (nextest gives each test one), unless it starts a
//! session of its own. It is not waited for until what is left of it has been
//! ended: asked first (SIGTERM to its group, which nextest passes on to its tests),
//! given a moment, then forced, every process of its session, every process still
//! descended from it, and every one that was when it was asked and is still the
//! process it was, then its group (`end`). A capture dropped (a stop) asks it so
//! too, and waits that moment.

use std::cell::Cell;
#[cfg(unix)]
use std::cell::RefCell;
use std::rc::Rc;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use crate::table::{self, Identity};
use crate::terminate::{end, Reach};
use crate::{Ender, Ending};

/// How often a program asked to end is looked at for whether it has.
#[cfg(unix)]
pub(crate) const ENDED_EVERY: Duration = Duration::from_millis(10);

/// Starts `command` as the leader of a process group of its own, which has
/// its pid for an id.
#[cfg(unix)]
pub(crate) fn lead(command: &mut tokio::process::Command) {
    command.process_group(0);
}

/// Windows has no groups: its job and `taskkill /T` hold the tree.
#[cfg(not(unix))]
pub(crate) fn lead(_command: &mut tokio::process::Command) {}

/// Starts `command` as the leader of a session of its own, held whole, and of its
/// group, both of which have its pid for an id (`setsid`, in the child before it runs
/// the program: a process that leads a group already may not).
#[cfg(unix)]
pub(crate) fn lead_whole(command: &mut tokio::process::Command) {
    #[allow(unsafe_code)]
    // SAFETY: the closure runs in the child between fork and exec, and calls setsid
    // alone, which is async-signal-safe, touches no memory of ours and takes no lock.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// Starts `command` held whole on Windows: suspended, its window hidden, so that it
/// is put in its job before it runs (`job::Job::of` lets it run).
#[cfg(windows)]
pub(crate) fn lead_whole(command: &mut tokio::process::Command) {
    use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
    command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
}

/// The program a capture started, and the group it leads: the capture's to
/// end until it has [released](Group::release) them, and ended by this one's
/// drop before.
pub(crate) struct Group {
    pid: Option<u32>,
    /// Whether the program has been waited for: its pid, and the id of its
    /// group, may be another's now. The enders hold it weakly, so that a
    /// capture dropped is told to them as no program to end.
    exited: Rc<Cell<bool>>,
    /// Whether it is held whole, the leader of a session of its own (Unix).
    #[cfg(unix)]
    whole: bool,
    /// Held whole, the processes descended from it when it was asked to end.
    #[cfg(unix)]
    asked: RefCell<Vec<Identity>>,
}

impl Group {
    /// The group of the program `pid`, which has just started, held whole or not.
    pub(crate) fn new(pid: Option<u32>, whole: bool) -> Self {
        #[cfg(not(unix))]
        let _ = whole;
        Self {
            pid,
            exited: Rc::default(),
            #[cfg(unix)]
            whole,
            #[cfg(unix)]
            asked: RefCell::default(),
        }
    }

    /// What ends the group later, should this process be ending.
    pub(crate) fn ender(&self) -> Ender {
        Ender::new(self.pid, Reach::Group, &self.exited)
    }

    /// Asks or forces the program, and what it started, to end: on Unix every
    /// process of the group gets the signal `how` names (SIGTERM is what
    /// Node's `kill` sends, to the program alone); on Windows the whole tree
    /// goes whatever `how` asks, where Node ends the program alone (and a
    /// `.cmd`'s own program, left running, keeps its streams open). Nothing
    /// is sent once it was waited for. Held whole on Unix, what it started is
    /// noted as it is asked, and forced with it (the module's doc).
    pub(crate) fn end(&self, how: Ending) {
        let (Some(pid), false) = (self.pid, self.exited.get()) else {
            return;
        };
        #[cfg(unix)]
        if self.whole {
            match how {
                Ending::Asked => self.note(pid),
                Ending::Forced => self.sweep(pid),
            }
        }
        end(pid, how, Reach::Group, true);
    }

    /// Notes the processes descended from the program `pid`, which it is about to be
    /// asked to end: the table links them to it while it runs.
    #[cfg(unix)]
    fn note(&self, pid: u32) {
        let Ok(leader) = i32::try_from(pid) else {
            return;
        };
        let below = table::descendants(&table::processes(), leader);
        self.asked
            .borrow_mut()
            .extend(below.iter().map(|process| process.identity));
    }

    /// Forces every process of the session the program `pid` leads, every one still
    /// descended from it, and every one noted as it was asked that is still the process
    /// it was, to end: but the program, which goes with its group. Each pass of the
    /// table that found any is followed by another, three at most, for what one of them
    /// started meanwhile.
    #[cfg(unix)]
    fn sweep(&self, pid: u32) {
        let Ok(leader) = i32::try_from(pid) else {
            return;
        };
        let noted = self.asked.borrow().clone();
        for _ in 0..3 {
            let now = table::processes();
            let mut doomed: Vec<Identity> = now
                .iter()
                .filter(|process| process.session == leader)
                .chain(&table::descendants(&now, leader))
                .map(|process| process.identity)
                .chain(
                    noted
                        .iter()
                        .filter(|noted| now.iter().any(|process| process.identity == **noted))
                        .copied(),
                )
                .filter(|identity| identity.pid != leader)
                .collect();
            doomed.sort_unstable_by_key(|identity| identity.pid);
            doomed.dedup();
            if doomed.is_empty() {
                break;
            }
            for identity in doomed {
                #[allow(unsafe_code)]
                // SAFETY: kill takes two integers and touches no memory of ours; the
                // pid is above 1 (the table has none other) and was just read as a
                // process of this run's.
                unsafe {
                    libc::kill(identity.pid, libc::SIGKILL);
                }
            }
        }
    }

    /// Whether the program has ended, asked without waiting for it: it is not
    /// reaped, and so its pid and its group's id are still its own. Not where it
    /// cannot be told.
    #[cfg(unix)]
    pub(crate) fn ended(&self) -> bool {
        let Some(pid) = self.pid.and_then(|pid| libc::id_t::try_from(pid).ok()) else {
            return false;
        };
        if self.exited.get() {
            return false;
        }
        #[allow(unsafe_code)]
        // SAFETY: waitid writes only the siginfo_t it is given, ours and zeroed;
        // WNOHANG returns at once, and WNOWAIT leaves the program to be reaped by
        // the wait for it.
        unsafe {
            let mut info: libc::siginfo_t = std::mem::zeroed();
            let asked = libc::waitid(
                libc::P_PID,
                pid,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            );
            asked == 0 && info.si_pid() != 0
        }
    }

    /// The program has been waited for: nothing is sent to its pid, nor to
    /// its group's id, after this.
    pub(crate) fn release(&self) {
        self.exited.set(true);
    }
}

impl Drop for Group {
    /// A capture dropped while its program runs: `kill_on_drop` ends the
    /// program, and this ends what it started with it. Dropped before the
    /// child that holds the program (declared after it), the group is
    /// signalled while the program is not yet reaped. Held whole, it is asked
    /// first, and given a moment to end (`FORCE_AFTER`), the thread waiting. On
    /// Windows the program alone goes, as `kill_on_drop` ends it (held whole, its
    /// job takes the rest).
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            if self.whole && self.pid.is_some() && !self.exited.get() {
                self.end(Ending::Asked);
                let until = std::time::Instant::now() + crate::execute::FORCE_AFTER;
                while !self.ended() && std::time::Instant::now() < until {
                    std::thread::sleep(ENDED_EVERY);
                }
            }
            self.end(Ending::Forced);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::Child;
    use std::time::Duration;

    use super::*;
    use crate::testing::Survivors;

    /// A sleep that leads a group of its own: a signal sent to the process or
    /// to its group ends it.
    #[allow(clippy::disallowed_methods, clippy::unwrap_used)] // The test starts what it ends.
    fn sleeper() -> Child {
        std::process::Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap()
    }

    #[test]
    fn what_a_group_holds_is_ended_by_its_end_and_by_its_drop() {
        let mut ended = sleeper();
        Group::new(Some(ended.id()), false).end(Ending::Forced);
        assert_eq!(ended.wait().unwrap().signal(), Some(libc::SIGKILL));

        // Dropped before it was released: the capture was cancelled.
        let mut dropped = sleeper();
        drop(Group::new(Some(dropped.id()), false));
        assert_eq!(dropped.wait().unwrap().signal(), Some(libc::SIGKILL));
    }

    #[test]
    fn once_released_nothing_is_sent_by_its_end_its_ender_or_its_drop() {
        let mut waited_for = sleeper();
        let _left = Survivors(vec![waited_for.id()]);
        let group = Group::new(Some(waited_for.id()), false);
        let ender = group.ender();
        assert!(ender.running());
        group.release();
        assert!(!ender.running());
        group.end(Ending::Forced);
        ender.force();
        drop(group);
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            waited_for.try_wait().unwrap().is_none(),
            "it was sent nothing: its id may be another's"
        );
        waited_for.kill().unwrap();
        waited_for.wait().unwrap();
    }

    #[test]
    fn a_group_dropped_is_no_one_s_to_end() {
        let mut sleeper = sleeper();
        let _left = Survivors(vec![sleeper.id()]);
        let ender = Group::new(Some(sleeper.id()), false).ender();
        // The group went with its drop, which ended the program: the ender
        // that outlives it has no program to end.
        assert_eq!(sleeper.wait().unwrap().signal(), Some(libc::SIGKILL));
        assert!(!ender.running());
        ender.force();
    }
}
