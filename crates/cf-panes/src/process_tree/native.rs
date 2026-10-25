//! What the system says of the processes of a Mac (the `proc_*` calls, `getsid` and
//! `sysctl`): each one's number, when it began, the process that started it, the group
//! and the session it is in, the program it runs, and the arguments and environment it
//! was started with. A process may end while it is looked at, so what the system will
//! not say is none, and one that is not ours, that is a zombie or that ended is no
//! process here.
use std::io;

use super::{Identity, Looked, Process, OWNER_ENV};

// XNU proc_info_private.h declares this 56-byte structure as an API. A process's own unique
// ID is its alone, and a reused PID cannot match it. The parent's is the identifier of the
// process that was its parent when it was forked, and again at each exec (T-384), and the
// system keeps it when that process ends: a job that has exec'd by then names its starter
// for good, one that execs after the starter has gone names init's (1). Load does not change
// it; it makes the starter's exit beat the job's exec more often (T-375). So it is no proof
// of who started a process whose starter has gone (`super::bound`; held by
// `the_identifier_recorded_for_the_process_that_started_a_job_is_its_parent_at_its_last_exec`
// in tests/headless.rs).
#[repr(C)]
#[derive(Default)]
struct NativeIdentity {
    uuid: [u8; 16],
    unique: u64,
    parent: u64,
    version: i32,
    parent_version: i32,
    reserved: [u64; 2],
}

pub(super) fn process(pid: i32) -> Option<Process> {
    if pid <= 1 || pid == std::process::id() as i32 {
        return None;
    }
    // SAFETY: proc_pidinfo writes a fixed-size native struct into this buffer.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info);
    // SAFETY: the buffer is `info`, writable for the `size` bytes passed with
    // it, and proc_pidinfo writes no more than that.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size as i32,
        )
    };
    if read != size as i32
        || info.pbi_pid != pid as u32
        // SAFETY: getuid takes no arguments, reads no memory of ours and
        // cannot fail.
        || info.pbi_uid != unsafe { libc::getuid() }
        || info.pbi_status == libc::SZOMB
    {
        return None;
    }
    let mut native = NativeIdentity::default();
    let size = std::mem::size_of_val(&native);
    // SAFETY: flavor 17 returns the exact native 56-byte identity structure.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            17,
            0,
            (&mut native as *mut NativeIdentity).cast(),
            size as i32,
        )
    };
    if read != size as i32 || native.unique == 0 {
        return None;
    }
    Some(Process {
        identity: Identity {
            pid,
            born: (info.pbi_start_tvsec, info.pbi_start_tvusec),
            unique: native.unique,
        },
        parent: native.parent,
        group: info.pbi_pgid as i32,
        session: session_of(pid),
        name: name_of(&info),
    })
}

/// The session a process is in, which it keeps whatever becomes of the process that
/// started it: none where the system will not say.
fn session_of(pid: i32) -> Option<i32> {
    // SAFETY: getsid takes a number and reads no memory of ours; a process that is
    // not there answers -1.
    let session = unsafe { libc::getsid(pid) };
    (session > 0).then_some(session)
}

/// Whether no process of any user has the number `pid`: the system answers a signal 0 to it
/// (which delivers nothing) with `ESRCH`, and only that. A number that is not a process's
/// (1 or less) is held, as is one the system answers otherwise: a zombie holds its number (the
/// signal succeeds), and so does a process of another user (`EPERM`), such as the root `login`
/// that leads a terminal's session.
pub(super) fn nobody_holds(pid: i32) -> bool {
    if pid <= 1 {
        return false;
    }
    // SAFETY: kill takes two integers and touches no memory of ours; signal 0 delivers
    // nothing, it asks whether there is a process to deliver to.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return false;
    }
    io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// The path of the program a process runs, as the system keeps it, whole whatever
/// it holds: empty where the system will not say.
pub(super) fn exe_of(pid: i32) -> String {
    let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer is `path`, writable for the size passed with it, and
    // proc_pidpath writes no more than that.
    let written = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
    match usize::try_from(written) {
        Ok(length) if length > 0 => String::from_utf8_lossy(&path[..length.min(path.len())])
            .trim_end_matches('\0')
            .to_owned(),
        _ => String::new(),
    }
}

/// What the system calls a process: its long name, or the short one.
fn name_of(info: &libc::proc_bsdinfo) -> String {
    let long = info.pbi_name.map(|byte| byte as u8);
    let short = info.pbi_comm.map(|byte| byte as u8);
    let chosen = if long[0] == 0 { &short[..] } else { &long[..] };
    let end = chosen
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(chosen.len());
    String::from_utf8_lossy(&chosen[..end]).into_owned()
}

/// Sends `signal` to the process `identity` names, and to none that has taken its
/// number: one that is gone is no error.
pub(super) fn signal(identity: Identity, signal: i32) -> io::Result<()> {
    // Do not act on a PID reused since discovery, including the original root.
    if process(identity.pid).map(|p| p.identity) != Some(identity) {
        return Ok(());
    }
    // A test build asks the guard, which reads the system by queries of its own, last.
    #[cfg(any(test, feature = "test-support"))]
    super::guard::before_sending(identity.pid, signal)?;
    // SAFETY: kill takes two integers and touches no memory of ours; the pid
    // was checked above to still be the process this tree discovered.
    if unsafe { libc::kill(identity.pid, signal) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

/// Sends `signal` to every process of the group `group`: one that is gone is no error.
/// A number that names more than a group is refused (0 is this process's own group, 1
/// is none, and -1 is every process).
pub(super) fn signal_group(group: i32, signal: i32) -> io::Result<()> {
    if group <= 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{group} names no process group of a pane"),
        ));
    }
    // A test build asks the guard, which reads the system by queries of its own, last.
    #[cfg(any(test, feature = "test-support"))]
    super::guard::before_sending_to_group(group, signal)?;
    // SAFETY: kill takes two integers and touches no memory of ours; the number was
    // checked above to name a group alone.
    if unsafe { libc::kill(-group, signal) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

/// Every process of ours there is now.
pub(super) fn processes() -> io::Result<Vec<Process>> {
    for _ in 0..3 {
        // SAFETY: a null buffer of size 0 only asks how many processes there
        // are; nothing is written.
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        if count <= 0 {
            return Err(io::Error::last_os_error());
        }
        let mut pids = vec![0i32; count as usize + 256];
        let size = (pids.len() * std::mem::size_of::<i32>()) as i32;
        // SAFETY: the buffer is `pids`, writable for the `size` bytes passed
        // with it, and proc_listallpids writes no more than that.
        let read = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), size) };
        if read <= 0 {
            return Err(io::Error::last_os_error());
        }
        if read as usize >= pids.len() {
            continue;
        }
        return Ok(pids[..read as usize]
            .iter()
            .filter_map(|pid| process(*pid))
            .collect());
    }
    Err(io::Error::other("could not snapshot pane processes"))
}

/// What the system says of the process `pid` now ([`looked_of`]): `proc_pidinfo` answers
/// ESRCH for a zombie as for none of that number (probed on Darwin 27).
pub(super) fn looked(pid: i32) -> Looked {
    // SAFETY: proc_pidinfo writes a fixed-size native struct into this buffer.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info);
    // SAFETY: the buffer is `info`, writable for the `size` bytes passed with it, and
    // proc_pidinfo writes no more than that.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size as i32,
        )
    };
    let owner = if read == size as i32 {
        Ok(info.pbi_uid)
    } else {
        Err(io::Error::last_os_error().raw_os_error())
    };
    // SAFETY: getuid takes no arguments, reads no memory of ours and cannot fail.
    let mine = unsafe { libc::getuid() };
    looked_of(owner, mine, || process(pid).is_some(), || gone(pid))
}

/// Whether no process has the number `pid` now, or one that ended (`proc_pidinfo` answers
/// ESRCH).
fn gone(pid: i32) -> bool {
    // SAFETY: proc_pidinfo writes a fixed-size native struct into this buffer.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info);
    // SAFETY: the buffer is `info`, writable for the `size` bytes passed with it, and
    // proc_pidinfo writes no more than that.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size as i32,
        )
    };
    read != size as i32 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// What the reads of a process say of it: its owner as its first read says (`owner`, or the
/// error that read failed with), the user this host runs as (`mine`), whether a whole read
/// of it then reads one of ours (`read`), and whether it is gone after (`gone`). It is
/// another user's only where its owner says so, or the system refuses to read it to us
/// (EPERM, which it answers for a process not ours, as launchd: probed on Darwin 27; ours
/// it always reads) (Astraeus on eb9f25ef); one that ended is ended; one of ours a whole
/// read did not read, or whose read failed otherwise, changed while it was read, and is
/// looked at again.
pub(super) fn looked_of(
    owner: Result<u32, Option<i32>>,
    mine: u32,
    read: impl FnOnce() -> bool,
    gone: impl FnOnce() -> bool,
) -> Looked {
    match owner {
        Err(Some(libc::ESRCH)) => Looked::Ended,
        Err(Some(libc::EPERM)) => Looked::Unreadable,
        Err(_) => Looked::Changed,
        Ok(owner) if owner != mine => Looked::Unreadable,
        Ok(_) if read() => Looked::Readable,
        Ok(_) if gone() => Looked::Ended,
        Ok(_) => Looked::Changed,
    }
}

/// The numbers of the processes `pid` started that the system names now
/// (`proc_listchildpids`), which it says of a process of another user, and of one that
/// ended and was not reaped, as of any: what [`process`] cannot read is among them.
pub(super) fn children(pid: i32) -> io::Result<Vec<i32>> {
    for _ in 0..3 {
        // SAFETY: a null buffer of size 0 only asks how many there may be; nothing is
        // written.
        let count = unsafe { libc::proc_listchildpids(pid, std::ptr::null_mut(), 0) };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut pids = vec![0i32; count as usize + 64];
        let size = (pids.len() * std::mem::size_of::<i32>()) as i32;
        // SAFETY: the buffer is `pids`, writable for the `size` bytes passed with it, and
        // proc_listchildpids writes no more than that.
        let read = unsafe { libc::proc_listchildpids(pid, pids.as_mut_ptr().cast(), size) };
        if read < 0 {
            return Err(io::Error::last_os_error());
        }
        if read as usize >= pids.len() {
            continue;
        }
        return Ok(pids[..read as usize]
            .iter()
            .copied()
            .filter(|pid| *pid > 0)
            .collect());
    }
    Err(io::Error::other("could not list a process's children"))
}

pub(super) fn has_marker(pid: i32, marker: &str) -> bool {
    // No arguments or environment values are logged, retained, or returned to
    // the app: the environment is only read for the marker.
    read_procargs(pid, |bytes| environment_has_marker(bytes, marker)).unwrap_or(false)
}

/// What the system keeps of a process's arguments and environment
/// (`KERN_PROCARGS2`): the argument count, the program's path, and the
/// arguments and the environment, each ended by a NUL. None for a process that
/// cannot be read (it ended, or it is not ours to read).
pub(super) fn procargs(pid: i32) -> Option<Vec<u8>> {
    read_procargs(pid, <[u8]>::to_vec)
}

thread_local! {
    /// The buffer what the system keeps of a process's arguments and environment is read
    /// into, made once for a thread (macOS bounds them by `ARG_MAX`, a megabyte) and used
    /// for every read on it: a report reads the marker of each of its candidates.
    static ARGUMENTS: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// `read` of what the system keeps of the process `pid`'s arguments and environment
/// ([`procargs`]), in this thread's buffer: none where it cannot be read.
fn read_procargs<T>(pid: i32, read: impl FnOnce(&[u8]) -> T) -> Option<T> {
    ARGUMENTS.with(|buffer| {
        let mut bytes = buffer.borrow_mut();
        if bytes.is_empty() {
            #[cfg(test)]
            BUFFERS_MADE.with(|made| made.set(made.get() + 1));
            bytes.resize(1024 * 1024, 0);
        }
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut size = bytes.len();
        // SAFETY: `mib` holds the `mib.len()` name entries passed with it, and
        // `bytes` is writable for the `size` bytes passed with it, which sysctl
        // lowers to what it wrote. Nothing is set: the new value is null, of length 0.
        let result = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                bytes.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (result == 0).then(|| read(&bytes[..size.min(bytes.len())]))
    })
}

#[cfg(test)]
thread_local! {
    /// How many buffers for a process's arguments and environment this thread has made.
    static BUFFERS_MADE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The fields after the program's path and its padding: the arguments, as many
/// as the count says, and then the environment, as far as the first empty field.
fn fields_of(bytes: &[u8]) -> Option<(usize, impl Iterator<Item = &[u8]> + '_)> {
    let count = bytes
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .map(i32::from_ne_bytes)?;
    if count <= 0 || count as usize > bytes.len() {
        return None;
    }
    let tail = &bytes[4..];
    let executable_end = tail.iter().position(|b| *b == 0)?;
    let tail = &tail[executable_end..];
    let arguments_start = tail.iter().position(|b| *b != 0)?;
    Some((
        count as usize,
        tail[arguments_start..].split(|b: &u8| *b == 0),
    ))
}

fn environment_has_marker(bytes: &[u8], marker: &str) -> bool {
    let Some((count, mut fields)) = fields_of(bytes) else {
        return false;
    };
    for _ in 0..count {
        if fields.next().is_none() {
            return false;
        }
    }
    let expected = format!("{OWNER_ENV}={marker}");
    fields
        .take_while(|field| !field.is_empty())
        .any(|field| field == expected.as_bytes())
}

/// The command line a process was started with: its arguments, one blank
/// apart. None where the arguments cannot be read or there are none.
pub(super) fn command_line(bytes: &[u8]) -> Option<String> {
    let (count, fields) = fields_of(bytes)?;
    let arguments: Vec<String> = fields
        .take(count)
        .map(|argument| String::from_utf8_lossy(argument).into_owned())
        .collect();
    (!arguments.is_empty()).then(|| arguments.join(" "))
}

#[cfg(test)]
mod tests;

/// The mach time base (`mach_timebase_info_data_t`): a tick is `numer / denom` ns.
#[repr(C)]
struct TimeBase {
    numer: u32,
    denom: u32,
}

unsafe extern "C" {
    /// The awake clock (libSystem), which stops while the machine sleeps and no setting
    /// of the time of day moves, in ticks.
    fn mach_absolute_time() -> u64;
    /// The continuous clock (libSystem): the awake clock and the time the machine slept,
    /// in ticks.
    fn mach_continuous_time() -> u64;
    /// The time base the two clocks tick in.
    fn mach_timebase_info(base: *mut TimeBase) -> libc::c_int;
}

/// Microseconds of `ticks` of the mach time base.
fn micros(ticks: u64) -> i64 {
    static BASE: std::sync::OnceLock<(u64, u64)> = std::sync::OnceLock::new();
    let (numer, denom) = *BASE.get_or_init(|| {
        let mut base = TimeBase { numer: 0, denom: 0 };
        // SAFETY: mach_timebase_info writes the two numbers of `base`, which it is given.
        unsafe { mach_timebase_info(&mut base) };
        (u64::from(base.numer.max(1)), u64::from(base.denom.max(1)))
    });
    i64::try_from(u128::from(ticks) * u128::from(numer) / u128::from(denom) / 1_000)
        .unwrap_or(i64::MAX)
}

/// The clocks now (`background::clock::Read`), each in microseconds: the wall clock read
/// between two reads of the awake and continuous ones.
pub(crate) fn clocks() -> crate::background::Read {
    // SAFETY: the mach clocks take no arguments and touch no memory of ours.
    let (awake, continuous) = unsafe { (mach_absolute_time(), mach_continuous_time()) };
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes the timespec it is given.
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut now) };
    // SAFETY: as above.
    let (awake_after, continuous_after) = unsafe { (mach_absolute_time(), mach_continuous_time()) };
    crate::background::Read {
        wall: now.tv_sec * 1_000_000 + now.tv_nsec / 1_000,
        awake: (micros(awake), micros(awake_after)),
        continuous: (micros(continuous), micros(continuous_after)),
    }
}

/// When the process `pid` began by the awake clock (`proc_pid_rusage`'s
/// `ri_proc_start_abstime`), in microseconds: none where the system will not say.
pub(crate) fn awake_start(pid: u32) -> Option<i64> {
    let pid = i32::try_from(pid).ok()?;
    // SAFETY: rusage_info_v2 is plain numbers, for which all zeroes is a value.
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    // SAFETY: proc_pid_rusage writes a rusage_info_v2 for RUSAGE_INFO_V2 into the buffer,
    // which is one.
    let read = unsafe {
        libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V2,
            (&mut info as *mut libc::rusage_info_v2).cast(),
        )
    };
    (read == 0 && info.ri_proc_start_abstime != 0).then(|| micros(info.ri_proc_start_abstime))
}
