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
//! process it was, then its group (`end`); each process is read again right before
//! it is signalled, and signalled only where it is still the one meant
//! (`table::signal_if_same`). One that cannot be held to be signalled (Linux, where no
//! pidfd of it can be opened) is sent nothing, and is left to its group's end; where it
//! outlived that, the capture says it (`Group::unpinned`). A capture dropped (a stop)
//! asks it so too, and waits that moment.
//!
//! What is out of reach: a process that starts a session of its own, and is no
//! longer linked to the program by the time it is looked for. A grandchild that
//! started its own session and whose parent ended before the program was asked;
//! and a child started while the program is asked (a SIGTERM handler that starts a
//! program which calls `setsid`, then exits), which was not there when the program's
//! descendants were noted, and has been given to another by the time they are
//! forced. Either is in no session of the run's, descended from it no more, and
//! noted never: it is let go, and may write after the run.

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
    /// Held whole, the processes that could not be held to be forced, and why.
    #[cfg(unix)]
    unpinned: RefCell<Vec<(Identity, String)>>,
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
            #[cfg(unix)]
            unpinned: RefCell::default(),
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
        let unpinned = sweep(leader, &self.asked.borrow(), &table::process);
        let mut kept = self.unpinned.borrow_mut();
        for (identity, why) in unpinned {
            if !kept.iter().any(|(kept, _)| *kept == identity) {
                kept.push((identity, why));
            }
        }
    }

    /// Why what the program left is not all ended, where a process of it that could not
    /// be held to be forced (`sweep`) is still the process it was a moment after its
    /// group was ended: each named, and why it could not be held.
    pub(crate) fn unpinned(&self) -> Result<(), String> {
        #[cfg(unix)]
        {
            let until = std::time::Instant::now() + crate::execute::FORCE_AFTER;
            loop {
                let left: Vec<String> = self
                    .unpinned
                    .borrow()
                    .iter()
                    .filter(|(identity, _)| {
                        table::process(identity.pid).is_some_and(|now| now.identity == *identity)
                    })
                    .map(|(identity, why)| format!("process {} ({why})", identity.pid))
                    .collect();
                if left.is_empty() {
                    return Ok(());
                }
                if std::time::Instant::now() >= until {
                    return Err(format!(
                        "{} could not be held to be forced to end, and had not ended {} s after \
                         its group was",
                        left.join(", "),
                        crate::execute::FORCE_AFTER.as_secs()
                    ));
                }
                std::thread::sleep(ENDED_EVERY);
            }
        }
        #[cfg(not(unix))]
        Ok(())
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

/// `Group::sweep`'s, of the program `leader`, given the processes `noted` as it was
/// asked, and how a process is read again before it is signalled (`read`, the system's
/// but for a test): one found by the session must be in it still, and every one born
/// when it was. Those that could not be held to be signalled, and why.
#[cfg(unix)]
fn sweep(
    leader: i32,
    noted: &[Identity],
    read: &dyn Fn(i32) -> Option<table::Process>,
) -> Vec<(Identity, String)> {
    let mut unpinned = Vec::new();
    for _ in 0..3 {
        let now = table::processes();
        let mut doomed: Vec<(Identity, Option<i32>)> = now
            .iter()
            .filter(|process| process.session == leader)
            .map(|process| (process.identity, Some(leader)))
            .chain(
                table::descendants(&now, leader)
                    .iter()
                    .map(|process| (process.identity, None)),
            )
            .chain(
                noted
                    .iter()
                    .filter(|noted| now.iter().any(|process| process.identity == **noted))
                    .map(|noted| (*noted, None)),
            )
            .filter(|(identity, _)| identity.pid != leader)
            .collect();
        doomed.sort_unstable_by_key(|(identity, session)| (identity.pid, session.is_none()));
        doomed.dedup_by_key(|(identity, _)| *identity);
        // One that could not be held is found again, and is no more to sweep for.
        doomed.retain(|(identity, _)| !unpinned.iter().any(|(held, _)| held == identity));
        if doomed.is_empty() {
            break;
        }
        for (identity, session) in doomed {
            if let table::Signalled::Unpinned(why) =
                table::signal_if_same(identity, session, libc::SIGKILL, read)
            {
                unpinned.push((identity, why));
            }
        }
    }
    unpinned
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

    #[test]
    fn a_process_noted_or_found_is_signalled_only_where_it_is_still_the_one_it_was() {
        // Both reviewers' look at round 6: a process noted as the program was asked may
        // end, and another take its number, before the force. Read again as born another
        // time, or in another session, it is no process of the run's: it is sent nothing.
        let mut noted = sleeper();
        let _left = Survivors(vec![noted.id()]);
        let pid = i32::try_from(noted.id()).unwrap();
        let read = table::process(pid).expect("the sleep is in the table");
        let reborn = |pid: i32| {
            table::process(pid).map(|now| table::Process {
                identity: Identity {
                    born: now.identity.born + 1,
                    ..now.identity
                },
                ..now
            })
        };
        // A leader no process is in the session of, nor descended from: only what was
        // noted is looked at.
        sweep(i32::MAX, &[read.identity], &reborn);
        assert!(
            table::signal_if_same(
                read.identity,
                Some(read.session + 1),
                libc::SIGKILL,
                &table::process
            ) == table::Signalled::NotSent,
            "found by a session it is not in"
        );
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            noted.try_wait().unwrap().is_none(),
            "it was sent nothing: it is not the process noted"
        );
        // Read again as the process it was, it is forced.
        sweep(i32::MAX, &[read.identity], &table::process);
        assert_eq!(noted.wait().unwrap().signal(), Some(libc::SIGKILL));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it ends.
    fn a_process_found_in_the_session_is_signalled_only_while_it_is_in_it_still() {
        // Round 7, decision 5: a process found in the program's session is read again right
        // before it is signalled, and must be in that session still, though it descends
        // from the program too. The program leads a session, and a sleep is in it.
        let mut leader = std::process::Command::new("/usr/bin/perl")
            .args([
                "-MPOSIX",
                "-e",
                "POSIX::setsid(); exec '/bin/sh', '-c', '/bin/sleep 30 & wait'",
            ])
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let session = i32::try_from(leader.id()).unwrap();
        let mut left = Survivors(vec![leader.id()]);
        let until = std::time::Instant::now() + Duration::from_secs(5);
        let member = loop {
            let found = table::processes()
                .into_iter()
                .find(|process| process.session == session && process.identity.pid != session);
            if let Some(found) = found {
                break u32::try_from(found.identity.pid).unwrap();
            }
            assert!(
                std::time::Instant::now() < until,
                "the sleep is in the session"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        left.0.push(member);
        let elsewhere = |pid: i32| {
            table::process(pid).map(|now| table::Process {
                session: now.session + 1,
                ..now
            })
        };
        sweep(session, &[], &elsewhere);
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            crate::alive(member),
            "read in another session, it was sent nothing"
        );
        sweep(session, &[], &table::process);
        assert!(
            crate::testing::gone_within(member, Duration::from_secs(5)),
            "in it still, it is forced"
        );
        leader.wait().unwrap();
    }
}
