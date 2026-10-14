//! The guard on the real signals of a test build. The bound ([`super::bound`]) keeps a wrong
//! selection from signalling what is not the window's, but it judges by the table of the
//! system's processes the tree was given, and a fault in that table (a bug in what builds
//! it, or a plant of one: one erroneous edit attaching every process of the machine to the
//! program) admits what the table says. A test of real processes that is wrong that way
//! signals the user's own programs before any assertion can say so (2026-10-14, six times).
//! So in every build that tests with real signals (this crate under `cfg(test)`, or with
//! the `test-support` feature, which the app and the headless helper turn on for their
//! tests), the real system's senders ([`super::native`]'s `signal` and `signal_group`) ask
//! this guard first, at the moment of sending, and a target it does not allow is not sent
//! to: the test panics, and nothing was sent.
//!
//! **No plant may edit this file.** It is what makes a plant of the snapshot, the
//! selection or the bound safe to run on real processes, and a plant that weakens it
//! takes that away from every run beside it. The tests of the guard send no signal.
//!
//! The guard asks the system itself, by queries of its own: it shares no code with
//! [`super::native::processes`], the table, the selection or the bound, and reads no table at
//! all, only the process it is asked of and what that process descends from. It allows a
//! target that
//!
//! - descends from the test process (`std::process::id()`): the target's parent, and that
//!   one's, and so on up (`proc_pidinfo` with `PROC_PIDTBSDINFO`, `pbi_ppid`) reach it;
//! - or is in the session of a program of a pane that this process started: a job a shell
//!   left running is a child of the system's init, and its session keeps the number of the
//!   program that began it (a session's number is not given to another process while the
//!   session has a member). The programs are noted as the tree attaches to them
//!   ([`note_program`]), by number and by when each began, and one whose number a process
//!   that began later holds is not that program;
//! - or was registered by the test that started it ([`register`]), by number and by when it
//!   began: a detached job that is none of the above (it left the session, and its parent
//!   has gone).
//!
//! A process group is signalled whole only when each of its members passes. A target that
//! is gone is allowed: there is nothing to reach, and the signal itself goes to none that
//! has taken its number. A process that is none of those is a stranger: the user's shell, a
//! build, the process that started the test, the sentinel a test keeps outside its windows
//! (`crate::pty::Sentinel`), which is why that one is started detached.

use std::io;
use std::sync::{Mutex, PoisonError};

/// How far up from a target the guard looks for the test process: more than any tree of a
/// test is deep, and an end to the walk if a ring of parents were ever read.
const DEPTH: usize = 64;

/// How many members of a process group the guard reads: a group of a test has a few, and
/// one that has more is not looked at whole, so it is refused.
const MEMBERS: usize = 1024;

/// When a process began, as the system says it: seconds and microseconds.
type Began = (u64, u64);

/// What the system says of a process now.
struct Seen {
    parent: i32,
    began: Began,
    name: String,
}

/// The pane programs this process started, by number and by when each began.
static PROGRAMS: Mutex<Vec<(i32, Began)>> = Mutex::new(Vec::new());

/// What tests registered: processes a test started that none of the rest allows.
static REGISTERED: Mutex<Vec<(i32, Began)>> = Mutex::new(Vec::new());

/// The process `pid`, as `proc_pidinfo` says it: none where there is none (it has gone, or
/// the system will not say).
fn seen(pid: i32) -> Option<Seen> {
    if pid <= 0 {
        return None;
    }
    // SAFETY: proc_pidinfo writes a fixed-size native struct into this buffer, and the
    // structure is plain data that zeroes make whole.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer is `info`, writable for the `size` bytes passed with it, and
    // proc_pidinfo writes no more than that.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    if read != size || info.pbi_pid != pid as u32 {
        return None;
    }
    let long = info.pbi_name.map(|byte| byte as u8);
    let short = info.pbi_comm.map(|byte| byte as u8);
    let chosen = if long[0] == 0 { &short[..] } else { &long[..] };
    let end = chosen
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(chosen.len());
    Some(Seen {
        parent: info.pbi_ppid as i32,
        began: (info.pbi_start_tvsec, info.pbi_start_tvusec),
        name: String::from_utf8_lossy(&chosen[..end]).into_owned(),
    })
}

/// The session `pid` is in: none where the system will not say.
fn session_of(pid: i32) -> Option<i32> {
    // SAFETY: getsid takes a number and reads no memory of ours; a process that is not
    // there answers -1.
    let session = unsafe { libc::getsid(pid) };
    (session > 0).then_some(session)
}

/// Whether `parent`, the process that started a target, is `root` or descends from it: its
/// parent, and that one's, reach `root`. The target itself is not read again: it may be going
/// as it is looked at (the close is ending it).
fn descends_from(parent: i32, root: i32) -> bool {
    let mut at = parent;
    for _ in 0..DEPTH {
        if at == root {
            return true;
        }
        if at <= 1 {
            return false;
        }
        let Some(process) = seen(at) else {
            return false;
        };
        at = process.parent;
    }
    false
}

/// Whether `pid` is in the session of a pane program this process started: its session is
/// that program's number, and no other process has taken the number since.
fn in_a_session_of_ours(pid: i32) -> bool {
    let Some(session) = session_of(pid) else {
        return false;
    };
    let programs = PROGRAMS.lock().unwrap_or_else(PoisonError::into_inner);
    is_ours(session, &programs, seen(session).map(|holder| holder.began))
}

/// Whether the session `session` is one of a program of `programs`: the number of one, which
/// the process that holds it now (`holder`: when it began) is, or no process holds it any
/// more (the program has gone, and what it left holds the session). A process that holds the
/// number and began at another time is another, whose session is its own: a number is given
/// again once a session has no member.
fn is_ours(session: i32, programs: &[(i32, Began)], holder: Option<Began>) -> bool {
    programs
        .iter()
        .filter(|(program, _)| *program == session)
        .any(|(_, began)| holder.is_none_or(|now| now == *began))
}

/// Whether a test registered `pid` as it began.
fn registered(pid: i32, began: Began) -> bool {
    REGISTERED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(&(pid, began))
}

/// Notes a pane's program, started by this process: the tree attaches to it as it opens.
pub(super) fn note_program(pid: i32) {
    if let Some(program) = seen(pid) {
        PROGRAMS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((pid, program.began));
    }
}

/// Registers the process `pid` as one a test started, which the host may signal though it
/// descends from nothing of the test's and is in no session of a pane's: a detached job
/// that left its session and lost its parent. It is allowed as long as it is the process
/// that began when it was registered.
#[cfg(test)]
pub(crate) fn register(pid: i32) {
    if let Some(process) = seen(pid) {
        register_began(pid, process.began);
    }
}

/// [`register`], with the time the process began.
#[cfg(test)]
fn register_began(pid: i32, began: Began) {
    REGISTERED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((pid, began));
}

/// Why `pid` is not allowed, or none where it is: it has gone, descends from the test
/// process, is in a session of a pane program this process started, or was registered. A
/// process that has gone by the time it would be refused is gone: the close that is sending
/// signals may be ending it as it is looked at, and there is nothing left to reach.
fn refusal(pid: i32) -> Option<String> {
    let process = seen(pid)?;
    let root = std::process::id() as i32;
    if descends_from(process.parent, root)
        || in_a_session_of_ours(pid)
        || registered(pid, process.began)
    {
        return None;
    }
    let session = session_of(pid);
    // Still there, and the process it was.
    if seen(pid).is_none_or(|now| now.began != process.began) {
        return None;
    }
    Some(format!(
        "pid {pid} ({}, started by {}, in session {session:?}) does not descend from the test \
         process {root}, is in no session of a pane program this process started and was not \
         registered",
        process.name, process.parent,
    ))
}

/// Says the guard stopped a signal, and stops the test: nothing was sent. A test that is
/// panicking already (a table that is dropped as it unwinds closes its windows) is not made
/// to panic twice, which would end the whole run: it is told the signal failed, and the
/// message is said in its output.
#[allow(clippy::print_stderr)] // Said in the output of the test that failed.
fn refused(signal: i32, whom: &str, why: &str) -> io::Error {
    let said = format!("the test guard stopped signal {signal} to {whom}: {why}; nothing was sent");
    if std::thread::panicking() {
        eprintln!("{said}");
        return io::Error::new(io::ErrorKind::PermissionDenied, said);
    }
    panic!("{said}")
}

/// Before a signal to the process `pid`: the test panics, and nothing is sent, where `pid`
/// is not allowed.
pub(super) fn before_sending(pid: i32, signal: i32) -> io::Result<()> {
    match refusal(pid) {
        Some(why) => Err(refused(signal, &format!("the process {pid}"), &why)),
        None => Ok(()),
    }
}

/// Before a signal to the process group `group`: the test panics, and nothing is sent,
/// where any member of it is not allowed.
pub(super) fn before_sending_to_group(group: i32, signal: i32) -> io::Result<()> {
    let whom = format!("the process group {group}");
    let members = members_of(group);
    if members.len() >= MEMBERS {
        return Err(refused(
            signal,
            &whom,
            "it has more members than the guard reads",
        ));
    }
    for member in members {
        if let Some(why) = refusal(member) {
            return Err(refused(signal, &whom, &why));
        }
    }
    Ok(())
}

/// The members of the process group `group`, as the system lists them: at most [`MEMBERS`]
/// of them, which a longer list fills.
fn members_of(group: i32) -> Vec<i32> {
    let mut pids = vec![0i32; MEMBERS];
    let size = (pids.len() * std::mem::size_of::<i32>()) as libc::c_int;
    // SAFETY: the buffer is `pids`, writable for the `size` bytes passed with it, and
    // proc_listpgrppids writes no more than that.
    let count = unsafe { libc::proc_listpgrppids(group, pids.as_mut_ptr().cast(), size) };
    pids.truncate(usize::try_from(count).unwrap_or(0));
    pids
}

#[cfg(test)]
mod tests;
