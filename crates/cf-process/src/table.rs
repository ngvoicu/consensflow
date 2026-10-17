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

/// Sends `signal` to the process `identity` names, where `read` (the system's word,
/// `process`, but for a test) says right before that it is still that process: born
/// when it was, and in the session `session` where one is given (it was found by the
/// session). Whether it was sent: nothing is sent to a process that is not the one
/// meant, whose number another has taken since (both reviewers' look at round 6).
///
/// On Linux the process is first held by a pidfd, so that the process read is the one
/// signalled whatever happens to its number. macOS has none: a process that ends, is
/// reaped and whose number is given to another in the moment between the read and the
/// signal is the window left there.
pub(crate) fn signal_if_same(
    identity: Identity,
    session: Option<i32>,
    signal: i32,
    read: &dyn Fn(i32) -> Option<Process>,
) -> bool {
    let same = || {
        read(identity.pid).is_some_and(|now| {
            now.identity == identity && session.is_none_or(|session| now.session == session)
        })
    };
    #[cfg(target_os = "linux")]
    {
        #[allow(unsafe_code)]
        // SAFETY: pidfd_open takes a pid and flags and touches no memory of ours; it
        // answers a descriptor of ours, or -1.
        let opened = unsafe { libc::syscall(libc::SYS_pidfd_open, identity.pid, 0) };
        if let Ok(pidfd) = libc::c_int::try_from(opened) {
            if pidfd >= 0 {
                let sent = same() && {
                    #[allow(unsafe_code)]
                    // SAFETY: the descriptor is ours, open until it is closed below; no
                    // siginfo is passed, and the call writes no memory of ours.
                    let sent = unsafe {
                        libc::syscall(
                            libc::SYS_pidfd_send_signal,
                            pidfd,
                            signal,
                            std::ptr::null::<libc::siginfo_t>(),
                            0,
                        )
                    };
                    sent == 0
                };
                #[allow(unsafe_code)]
                // SAFETY: the descriptor is ours, and closed once.
                unsafe {
                    libc::close(pidfd);
                }
                return sent;
            }
        }
    }
    if !same() {
        return false;
    }
    #[allow(unsafe_code)]
    // SAFETY: kill takes two integers and touches no memory of ours; the number was read
    // as the process meant just now (the window left is said above).
    unsafe {
        libc::kill(identity.pid, signal) == 0
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
    use crate::testing::{pids_written_to, Survivors};

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
            .spawn()
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
