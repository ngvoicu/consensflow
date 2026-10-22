//! What the system says of the processes of a Mac (the `proc_*` calls, `getsid` and
//! `sysctl`): each one's number, when it began, the process that started it, the group
//! and the session it is in, the program it runs, and the arguments and environment it
//! was started with. A process may end while it is looked at, so what the system will
//! not say is none, and one that is not ours, that is a zombie or that ended is no
//! process here.
use std::io;

use super::{Identity, Looked, Process, OWNER_ENV};

// XNU proc_info_private.h declares this 56-byte structure as an API. The
// original parent's unique ID survives reparenting; a reused PID cannot match.
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

/// What the system says of the process `pid` now: one of ours that runs and a look reads
/// ([`process`]), one that ended (a zombie, or none of that number), or one it will not
/// say (another user's: `proc_pidinfo` fails, or names another owner).
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
    if read != size as i32 {
        return match io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Looked::Ended,
            _ => Looked::Unreadable,
        };
    }
    if info.pbi_status == libc::SZOMB {
        return Looked::Ended;
    }
    if process(pid).is_some() {
        Looked::Readable
    } else {
        Looked::Unreadable
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
    procargs(pid).is_some_and(|bytes| environment_has_marker(&bytes, marker))
}

/// What the system keeps of a process's arguments and environment
/// (`KERN_PROCARGS2`): the argument count, the program's path, and the
/// arguments and the environment, each ended by a NUL. None for a process that
/// cannot be read (it ended, or it is not ours to read).
pub(super) fn procargs(pid: i32) -> Option<Vec<u8>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    // macOS bounds argv + environment by ARG_MAX.
    let mut bytes = vec![0u8; 1024 * 1024];
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
    if result != 0 {
        return None;
    }
    bytes.truncate(size);
    Some(bytes)
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
