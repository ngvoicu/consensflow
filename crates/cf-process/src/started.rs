//! A process a program has seen, by its number and when it began: together they name it
//! and no process that is given its number after it ends. What a program ends that it did
//! not keep the handle of (a number a report said, or a file) it ends by this, never by
//! the number alone: a number read a while ago may be another's now, and a number another
//! program wrote may be any process's at all (2026-10-28: a smoke ends what it started,
//! beside the live app, whose number is one like any other).

use crate::terminate::Ending;

/// A process of this user's, as it was when it was seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Started {
    #[cfg(unix)]
    identity: crate::table::Identity,
    #[cfg(not(unix))]
    pid: u32,
}

impl Started {
    /// The process `pid` is now: none where it is not this user's, is gone or is a zombie,
    /// or the system will not say when it began (Windows, where none is ever seen).
    pub fn of(pid: u32) -> Option<Self> {
        #[cfg(unix)]
        {
            let process = crate::table::process(i32::try_from(pid).ok()?)?;
            Some(Self {
                identity: process.identity,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
            None
        }
    }

    /// Its number.
    pub fn pid(&self) -> u32 {
        #[cfg(unix)]
        {
            u32::try_from(self.identity.pid).unwrap_or_default()
        }
        #[cfg(not(unix))]
        {
            self.pid
        }
    }

    /// Whether it began after `other` did, or as it did: a process `other` started, or one
    /// started after it, has; one that ran before it (the live app beside a test) has not.
    pub fn began_since(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            self.identity.born >= other.identity.born
        }
        #[cfg(not(unix))]
        {
            let _ = other;
            false
        }
    }

    /// Whether it began no later than `time`: a process that wrote a file was running when the
    /// file was last written, and one that began after that is another that took its number.
    /// False where the system does not say when it began on the clock (Windows).
    pub fn began_by(&self, time: std::time::SystemTime) -> bool {
        let Ok(since_epoch) = time.duration_since(std::time::UNIX_EPOCH) else {
            return false;
        };
        let at = u64::try_from(since_epoch.as_micros()).unwrap_or(u64::MAX);
        began_at(self).is_some_and(|began| began <= at)
    }

    /// What the system says of it now, where it is still the process seen.
    #[cfg(unix)]
    fn now(&self) -> Option<crate::table::Process> {
        crate::table::process(self.identity.pid).filter(|now| now.identity == self.identity)
    }

    /// Whether it is still running: its number is still its own.
    pub fn is_running(&self) -> bool {
        #[cfg(unix)]
        {
            self.now().is_some()
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// The process group it is in now, while it is still the process seen.
    pub fn group(&self) -> Option<u32> {
        #[cfg(unix)]
        {
            self.now().and_then(|now| u32::try_from(now.group).ok())
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    /// Ends it as `how` says, where it is still the process seen: whether it was signalled.
    /// A number another process has taken since is sent nothing.
    pub fn end(&self, how: Ending) -> bool {
        #[cfg(unix)]
        {
            crate::table::signal_if_same(self.identity, None, signal(how), &crate::table::process)
                == crate::table::Signalled::Sent
        }
        #[cfg(not(unix))]
        {
            let _ = how;
            false
        }
    }

    /// Ends the process group `group` as `how` says, where it is still the process seen
    /// and still in that group, which is not one that names more than a group (0 is the
    /// caller's own, 1 none): whether the group was signalled. A group that holds a
    /// process seen is a group whose number no other has taken: its number is held while
    /// any process is in it.
    pub fn end_group(&self, group: u32, how: Ending) -> bool {
        #[cfg(unix)]
        {
            let Some(leader) = i32::try_from(group).ok().filter(|leader| *leader > 1) else {
                return false;
            };
            if self.group() != Some(group) {
                return false;
            }
            #[allow(unsafe_code)]
            // SAFETY: kill takes two integers and touches no memory of ours; the group was
            // read as the one this process is in just now, so its number is no other's.
            unsafe {
                libc::kill(-leader, signal(how)) == 0
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (group, how);
            false
        }
    }
}

/// When `started` began, in microseconds since the epoch: macOS says it so; Linux says it in
/// clock ticks since the machine started, which `/proc/stat` says the time of.
#[cfg(target_os = "macos")]
fn began_at(started: &Started) -> Option<u64> {
    Some(started.identity.born)
}

/// As on macOS, from the ticks since the machine started and when it started.
#[cfg(target_os = "linux")]
fn began_at(started: &Started) -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let booted: u64 = stat
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    #[allow(unsafe_code)]
    // SAFETY: sysconf takes a number and reads no memory of ours.
    let ticks = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
    if ticks == 0 {
        return None;
    }
    let after_boot = started.identity.born.checked_mul(1_000_000)? / ticks;
    booted.checked_mul(1_000_000)?.checked_add(after_boot)
}

/// No other system says.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn began_at(_started: &Started) -> Option<u64> {
    None
}

#[cfg(unix)]
fn signal(how: Ending) -> i32 {
    match how {
        Ending::Asked => libc::SIGTERM,
        Ending::Forced => libc::SIGKILL,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::Duration;

    use crate::testing::{command, gone_within};

    /// A sleep of half a minute, its own group's leader where `own_group`.
    fn sleeper(own_group: bool) -> std::process::Command {
        let mut sleep = command("/bin/sleep");
        sleep.arg("30").stdin(Stdio::null());
        if own_group {
            sleep.process_group(0);
        }
        sleep
    }

    /// A process seen is ended while it is the one seen, and its group with it; a number
    /// that is now another process's is sent nothing.
    #[test]
    fn a_process_is_ended_by_its_number_and_when_it_began_and_never_another_of_its_number() {
        let mut first = sleeper(true).spawn().unwrap();
        let seen = Started::of(first.id()).unwrap();
        assert_eq!(seen.pid(), first.id());
        assert!(seen.is_running());
        assert_eq!(seen.group(), Some(first.id()));
        // The same number, said to have begun at another time: another process.
        let other = Started {
            identity: crate::table::Identity {
                born: seen.identity.born + 1,
                ..seen.identity
            },
        };
        assert!(!other.is_running());
        assert!(!other.end(Ending::Forced));
        assert!(!other.end_group(first.id(), Ending::Forced));
        assert!(
            first.try_wait().unwrap().is_none(),
            "another's number ended it"
        );
        let mut later = sleeper(false).spawn().unwrap();
        let since = Started::of(later.id()).unwrap();
        assert!(since.began_since(&seen) && !seen.began_since(&since));
        assert!(!seen.end_group(0, Ending::Forced) && !seen.end_group(1, Ending::Forced));
        assert!(seen.end_group(first.id(), Ending::Forced));
        assert!(gone_within(first.id(), Duration::from_secs(10)));
        first.wait().unwrap();
        assert!(!seen.is_running());
        assert!(
            !seen.end(Ending::Forced),
            "a reaped process is sent nothing"
        );
        assert!(since.end(Ending::Forced));
        assert!(gone_within(later.id(), Duration::from_secs(10)));
        later.wait().unwrap();
    }

    /// A process began by a time after its start, and not by one before it: this test's own,
    /// by the time a file it writes now is written, and not by the epoch's first second.
    #[test]
    fn a_process_began_by_a_time_after_its_start_and_not_before_it() {
        let own = Started::of(std::process::id()).unwrap();
        let folder = tempfile::tempdir().unwrap();
        let written = folder.path().join("written");
        std::fs::write(&written, "now").unwrap();
        let now = std::fs::metadata(&written).unwrap().modified().unwrap();
        assert!(own.began_by(now));
        assert!(!own.began_by(std::time::UNIX_EPOCH + Duration::from_secs(1)));
    }

    /// Only this user's processes are seen, and none that is not there.
    #[test]
    fn no_process_but_this_users_is_seen() {
        assert_eq!(Started::of(1), None, "launchd or init is root's");
        assert_eq!(Started::of(0), None);
        assert_eq!(Started::of(u32::MAX), None);
    }
}
