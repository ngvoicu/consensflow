//! The processes of this user there are now, as the system says them (Unix): each
//! one's number and when it began, which together name it and no process that takes
//! its number after it; the process that started it, and the group and the session it
//! is in. A process may end while it is looked at: what the system will not say of one
//! is no process here. What a run held whole left is found here (`group`).

/// A process, by its number and when it began.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) pid: i32,
    pub(crate) born: u64,
}

/// A process, and where it stands among the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Process {
    pub(crate) identity: Identity,
    /// The process that started it, or the one it was given to when that one ended.
    pub(crate) parent: i32,
    pub(crate) group: i32,
    pub(crate) session: i32,
}

/// Every process of this user there is now: none where the system will not say.
#[cfg(target_os = "macos")]
pub(crate) fn processes() -> Vec<Process> {
    for _ in 0..3 {
        #[allow(unsafe_code)]
        // SAFETY: a null buffer of size 0 only asks how many processes there are;
        // nothing is written.
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        let Ok(count) = usize::try_from(count) else {
            return Vec::new();
        };
        let mut pids = vec![0i32; count + 256];
        let Ok(size) = i32::try_from(pids.len() * std::mem::size_of::<i32>()) else {
            return Vec::new();
        };
        #[allow(unsafe_code)]
        // SAFETY: the buffer is `pids`, writable for the `size` bytes passed with it,
        // and proc_listallpids writes no more than that.
        let read = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), size) };
        let Ok(read) = usize::try_from(read) else {
            return Vec::new();
        };
        if read >= pids.len() {
            continue;
        }
        return pids[..read]
            .iter()
            .filter_map(|pid| process(*pid))
            .collect();
    }
    Vec::new()
}

/// What the system says of the process `pid`: none where it is not this user's, it
/// is a zombie, or it is gone.
#[cfg(target_os = "macos")]
pub(crate) fn process(pid: i32) -> Option<Process> {
    if pid <= 1 {
        return None;
    }
    #[allow(unsafe_code)]
    // SAFETY: proc_bsdinfo is plain data, for which zeroes are a value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = i32::try_from(std::mem::size_of_val(&info)).ok()?;
    #[allow(unsafe_code)]
    // SAFETY: the buffer is `info`, writable for the `size` bytes passed with it, and
    // proc_pidinfo writes no more than that.
    let read =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    #[allow(unsafe_code)]
    // SAFETY: getuid takes no arguments, reads no memory of ours and cannot fail.
    let mine = info.pbi_uid == unsafe { libc::getuid() };
    if read != size || info.pbi_pid != u32::try_from(pid).ok()? || !mine {
        return None;
    }
    if info.pbi_status == libc::SZOMB {
        return None;
    }
    #[allow(unsafe_code)]
    // SAFETY: getsid takes a number and reads no memory of ours; a process that is not
    // there answers -1.
    let session = unsafe { libc::getsid(pid) };
    Some(Process {
        identity: Identity {
            pid,
            born: info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec,
        },
        parent: i32::try_from(info.pbi_ppid).ok()?,
        group: i32::try_from(info.pbi_pgid).ok()?,
        session,
    })
}

/// Every process there is now that `/proc` shows: none where it cannot be read.
#[cfg(target_os = "linux")]
pub(crate) fn processes() -> Vec<Process> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        .filter_map(process)
        .collect()
}

/// What `/proc/<pid>/stat` says of the process `pid`: its fields after the program's
/// name in parentheses (which may hold anything) are its state, the process that
/// started it, its group and its session, and, nineteenth, when it began.
#[cfg(target_os = "linux")]
pub(crate) fn process(pid: i32) -> Option<Process> {
    if pid <= 1 {
        return None;
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<&str> = stat
        .get(stat.rfind(')')? + 1..)?
        .split_whitespace()
        .collect();
    if fields.first() == Some(&"Z") {
        return None;
    }
    let number = |at: usize| fields.get(at)?.parse::<i64>().ok();
    Some(Process {
        identity: Identity {
            pid,
            born: u64::try_from(number(19)?).ok()?,
        },
        parent: i32::try_from(number(1)?).ok()?,
        group: i32::try_from(number(2)?).ok()?,
        session: i32::try_from(number(3)?).ok()?,
    })
}

/// How long a look at a group or a session waits before it looks again. macOS reads a group with
/// one call; Linux reads the `stat` of every process in `/proc` for one, a pass that costs a good
/// deal on a machine with many, so the next is not begun at once there.
#[cfg(unix)]
pub(crate) const BETWEEN_PASSES: std::time::Duration = if cfg!(target_os = "linux") {
    std::time::Duration::from_millis(10)
} else {
    std::time::Duration::from_millis(1)
};

/// The processes of the group `group` there are now that are not zombies (a zombie is in its
/// group until it is reaped, and is nothing to end): none where the system will not say.
#[cfg(target_os = "macos")]
pub(crate) fn in_group(group: i32) -> Vec<Process> {
    let mut pids = vec![0i32; 4096];
    let Ok(size) = i32::try_from(pids.len() * std::mem::size_of::<i32>()) else {
        return Vec::new();
    };
    #[allow(unsafe_code)]
    // SAFETY: the buffer is `pids`, writable for the `size` bytes passed with it, and
    // proc_listpgrppids writes no more than that: it answers how many it wrote.
    let read = unsafe { libc::proc_listpgrppids(group, pids.as_mut_ptr().cast(), size) };
    let Ok(read) = usize::try_from(read) else {
        return Vec::new();
    };
    pids.iter()
        .take(read.min(pids.len()))
        .filter_map(|pid| process(*pid))
        .collect()
}

/// As on macOS, from what `/proc` shows.
#[cfg(target_os = "linux")]
pub(crate) fn in_group(group: i32) -> Vec<Process> {
    processes()
        .into_iter()
        .filter(|process| process.group == group)
        .collect()
}

/// No other Unix says its processes here.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn in_group(_group: i32) -> Vec<Process> {
    Vec::new()
}

/// Whether the process `pid`, which `kill(pid, 0)` finds, has ended and waits to be reaped:
/// the system tells nothing of a process so (`proc_pidinfo` answers no bytes), as it tells of
/// every other.
#[cfg(all(test, target_os = "macos"))]
pub(crate) fn is_zombie(pid: i32) -> bool {
    #[allow(unsafe_code)]
    // SAFETY: proc_bsdinfo is plain data, for which zeroes are a value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let Ok(size) = i32::try_from(std::mem::size_of_val(&info)) else {
        return false;
    };
    #[allow(unsafe_code)]
    // SAFETY: the buffer is `info`, writable for the `size` bytes passed with it, and
    // proc_pidinfo writes no more than that.
    let read =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    read != size || info.pbi_status == libc::SZOMB
}

/// As on macOS, from what `/proc/<pid>/stat` says.
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn is_zombie(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rfind(')')
            .and_then(|at| stat.get(at + 1..))
            .and_then(|rest| rest.split_whitespace().next().map(|state| state == "Z"))
            .unwrap_or(false)
    })
}

/// No other Unix says.
#[cfg(all(test, not(any(target_os = "macos", target_os = "linux"))))]
pub(crate) fn is_zombie(_pid: i32) -> bool {
    false
}

/// No other Unix says its processes here.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn processes() -> Vec<Process> {
    Vec::new()
}

/// Nor any one of them.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn process(_pid: i32) -> Option<Process> {
    None
}

/// What came of a signal meant for a process (`signal_if_same`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Signalled {
    /// It was still the process meant, and was sent the signal.
    Sent,
    /// It was not (it is gone, or its number is another's), or the system would not
    /// send it: nothing was sent.
    NotSent,
    /// It could not be held to be signalled (Linux: no pidfd of it could be opened), so
    /// it was sent nothing, whatever it is now: why. macOS holds no process so, and
    /// signals it by its number.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    Unpinned(String),
}

/// Sends `signal` to the process `identity` names, where `read` (the system's word,
/// `process`, but for a test) says right before that it is still that process: born
/// when it was, and in the session `session` where one is given (it was found by the
/// session). Nothing is sent to a process that is not the one meant, whose number
/// another has taken since (both reviewers' look at round 6).
///
/// On Linux the process is first held by a pidfd, so that the process read is the one
/// signalled whatever happens to its number; where none can be opened (an older kernel,
/// no descriptor left), it is sent nothing (both reviewers' look at round 7: a number
/// signalled then could be another's). macOS has none: a process that ends, is reaped
/// and whose number is given to another in the moment between the read and the signal
/// is the window left there.
pub(crate) fn signal_if_same(
    identity: Identity,
    session: Option<i32>,
    signal: i32,
    read: &dyn Fn(i32) -> Option<Process>,
) -> Signalled {
    #[cfg(test)]
    if UNPINNABLE.get() {
        return Signalled::Unpinned("the test pins nothing".to_owned());
    }
    #[cfg(target_os = "linux")]
    return signal_pinned(identity, session, signal, read, Pidfd::open);
    #[cfg(not(target_os = "linux"))]
    {
        if !same(identity, session, read) {
            return Signalled::NotSent;
        }
        #[allow(unsafe_code)]
        // SAFETY: kill takes two integers and touches no memory of ours; the number was
        // read as the process meant just now (the window left is said above).
        let sent = unsafe { libc::kill(identity.pid, signal) == 0 };
        if sent {
            Signalled::Sent
        } else {
            Signalled::NotSent
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Whether no process is pinned to be signalled on this thread, as where Linux opens
    /// no pidfd: for the tests of what is left of a run so.
    pub(crate) static UNPINNABLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether `read` says the process `identity` names is still that one, and in the
/// session `session` where one is given.
fn same(identity: Identity, session: Option<i32>, read: &dyn Fn(i32) -> Option<Process>) -> bool {
    read(identity.pid).is_some_and(|now| {
        now.identity == identity && session.is_none_or(|session| now.session == session)
    })
}

/// A process held by a handle of its own, which signals it whatever is given its
/// number: the handle goes when this is dropped.
#[cfg(any(target_os = "linux", test))]
pub(crate) trait Pinned {
    /// Sends `signal` to the process held: whether it was sent.
    fn send(&self, signal: i32) -> bool;
}

/// `signal_if_same` where a process is held to be signalled (`pin`, Linux's pidfd but for
/// a test): held first, then read, then signalled through what holds it. One that cannot
/// be held is sent nothing, and why is said; one gone before it is held, nothing.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn signal_pinned<P: Pinned>(
    identity: Identity,
    session: Option<i32>,
    signal: i32,
    read: &dyn Fn(i32) -> Option<Process>,
    pin: impl FnOnce(i32) -> std::io::Result<P>,
) -> Signalled {
    let pinned = match pin(identity.pid) {
        Ok(pinned) => pinned,
        Err(gone) if gone.raw_os_error() == Some(libc::ESRCH) => return Signalled::NotSent,
        Err(why) => return Signalled::Unpinned(format!("no pidfd of it could be opened: {why}")),
    };
    if same(identity, session, read) && pinned.send(signal) {
        Signalled::Sent
    } else {
        Signalled::NotSent
    }
}

/// A process held by a pidfd.
#[cfg(target_os = "linux")]
struct Pidfd(libc::c_int);

#[cfg(target_os = "linux")]
impl Pidfd {
    fn open(pid: i32) -> std::io::Result<Self> {
        #[allow(unsafe_code)]
        // SAFETY: pidfd_open takes a pid and flags and touches no memory of ours; it
        // answers a descriptor of ours, or -1.
        let opened = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        match libc::c_int::try_from(opened) {
            Ok(pidfd) if pidfd >= 0 => Ok(Self(pidfd)),
            _ => Err(std::io::Error::last_os_error()),
        }
    }
}

#[cfg(target_os = "linux")]
impl Pinned for Pidfd {
    fn send(&self, signal: i32) -> bool {
        #[allow(unsafe_code)]
        // SAFETY: the descriptor is ours, open until this is dropped; no siginfo is
        // passed, and the call writes no memory of ours.
        let sent = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.0,
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        sent == 0
    }
}

#[cfg(target_os = "linux")]
impl Drop for Pidfd {
    fn drop(&mut self) {
        #[allow(unsafe_code)]
        // SAFETY: the descriptor is ours, and closed once.
        unsafe {
            libc::close(self.0);
        }
    }
}

/// The processes of `table` started by `root`, and by those, and so on down, as the
/// table links them now: a process whose parent ended before it was given to another,
/// and is linked to `root` no more.
pub(crate) fn descendants(table: &[Process], root: i32) -> Vec<Process> {
    let mut found: Vec<Process> = Vec::new();
    let mut parents = vec![root];
    while let Some(parent) = parents.pop() {
        for process in table.iter().filter(|process| process.parent == parent) {
            if !found.contains(process) && process.identity.pid != root {
                found.push(*process);
                parents.push(process.identity.pid);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::CommandExt;
    use std::time::Duration;

    use super::*;
    use crate::spawning::StartedUnderTheLock as _;
    use crate::testing::{pids_written_to, Survivors};

    /// A process "held" by its number alone, as a test holds it.
    struct Numbered(i32);

    impl Pinned for Numbered {
        fn send(&self, signal: i32) -> bool {
            #[allow(unsafe_code)]
            // SAFETY: kill takes two integers and touches no memory of ours.
            unsafe {
                libc::kill(self.0, signal) == 0
            }
        }
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it ends.
    fn a_process_that_cannot_be_held_to_be_signalled_is_sent_nothing_and_why_is_said() {
        // Both reviewers' look at round 7: where Linux opened no pidfd of a process, it
        // was read and then signalled by its number, which may be another's by then.
        let mut sleep = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn_locked()
            .unwrap();
        let _left = Survivors(vec![sleep.id()]);
        let pid = i32::try_from(sleep.id()).unwrap();
        let read = process(pid).expect("the sleep is in the table");
        let refused = signal_pinned(read.identity, None, libc::SIGKILL, &process, |_| {
            Err::<Numbered, _>(std::io::Error::from_raw_os_error(libc::EMFILE))
        });
        assert_eq!(
            refused,
            Signalled::Unpinned(format!(
                "no pidfd of it could be opened: {}",
                std::io::Error::from_raw_os_error(libc::EMFILE)
            ))
        );
        std::thread::sleep(Duration::from_millis(300));
        assert!(sleep.try_wait().unwrap().is_none(), "it was sent nothing");
        // Held, it is read again, and is sent nothing where it is not the one it was.
        let reborn = |pid: i32| {
            process(pid).map(|now| Process {
                identity: Identity {
                    born: now.identity.born + 1,
                    ..now.identity
                },
                ..now
            })
        };
        let other = signal_pinned(read.identity, None, libc::SIGKILL, &reborn, |pid| {
            Ok(Numbered(pid))
        });
        assert_eq!(other, Signalled::NotSent);
        std::thread::sleep(Duration::from_millis(300));
        assert!(sleep.try_wait().unwrap().is_none(), "it was sent nothing");
        // Gone before it could be held: there is nothing to send to, nor to say.
        let gone = signal_pinned(read.identity, None, libc::SIGKILL, &process, |_| {
            Err::<Numbered, _>(std::io::Error::from_raw_os_error(libc::ESRCH))
        });
        assert_eq!(gone, Signalled::NotSent);
        // Held, and read as it was, it is signalled through what holds it.
        let sent = signal_pinned(read.identity, None, libc::SIGKILL, &process, |pid| {
            Ok(Numbered(pid))
        });
        assert_eq!(sent, Signalled::Sent);
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(sleep.wait().unwrap().signal(), Some(libc::SIGKILL));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it reads.
    fn a_process_is_found_with_its_parent_group_and_session_and_what_it_started_below_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pids");
        // A shell that leads a group of its own, starts a sleep, and waits for it.
        let mut shell = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("sleep 30 & echo $! > \"$0\"; wait")
            .arg(&file)
            .process_group(0)
            .spawn_locked()
            .unwrap();
        let sleep = i32::try_from(pids_written_to(&file, 1)[0]).unwrap();
        let _left = Survivors(vec![u32::try_from(sleep).unwrap(), shell.id()]);
        let leader = i32::try_from(shell.id()).unwrap();
        let table = processes();
        let found = table
            .iter()
            .find(|process| process.identity.pid == sleep)
            .expect("the sleep is in the table");
        assert_eq!(found.parent, leader);
        assert_eq!(found.group, leader, "in the group its shell leads");
        #[allow(unsafe_code)]
        // SAFETY: getsid takes a number and reads no memory of ours.
        let own = unsafe { libc::getsid(0) };
        assert_eq!(found.session, own, "in this process's session");
        let below: Vec<i32> = descendants(&table, leader)
            .iter()
            .map(|process| process.identity.pid)
            .collect();
        assert_eq!(below, [sleep]);
        // The same process, read again, is born when it was.
        std::thread::sleep(Duration::from_millis(20));
        let again = processes()
            .into_iter()
            .find(|process| process.identity.pid == sleep)
            .unwrap();
        assert_eq!(again.identity, found.identity);
        shell.kill().unwrap();
        shell.wait().unwrap();
    }
}
