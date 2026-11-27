//! What the machine itself says of a process, and how a check holds one to end it: on
//! macOS `proc_pidinfo` (when it began, to the microsecond) and a signal by its number;
//! on Windows a handle to the process, on which its creation time is read and through
//! which it is ended; on Linux `ps` and a signal. The one place a live check calls the
//! system without a program between, in a few calls, each said safe where it is made.

use std::io;

use super::{Held, Holds};
#[cfg(all(unix, not(target_os = "macos")))]
use super::{Identity, Running};
#[cfg(unix)]
use crate::Error;
#[cfg(all(unix, not(target_os = "macos")))]
use crate::Result;

#[cfg(all(target_os = "macos", test))]
pub(super) use mac::lstart_second;
#[cfg(target_os = "macos")]
pub(super) use mac::{identity, processes, working_folder};
#[cfg(windows)]
pub(super) use win::{identity, processes};

/// The folder a process runs in is read on a Mac only: the one place a check needs it is the
/// run of a Pi or Devin worker on a Mac (`orphans_in_folder`), and Windows and Linux hold what
/// a window starts in other ways (the pane's job) or are no machine of ours.
#[cfg(not(target_os = "macos"))]
pub(super) fn working_folder(_pid: u32) -> Option<std::path::PathBuf> {
    None
}

/// What holds a process for a check that ends it, on this machine.
pub(super) struct Native;

/// The processes `ps` lists, as Linux says when each began: to the second.
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn processes() -> Result<Vec<Running>> {
    super::ps(None)
}

/// Which process has the number `pid` now, as `ps` says it on Linux.
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn identity(pid: u32) -> Result<Option<Identity>> {
    Ok(super::ps(Some(pid))?
        .into_iter()
        .map(|process| process.identity)
        .find(|identity| identity.pid == pid))
}

/// macOS and Linux give no handle to a process: it is held by its number, and when it
/// began is read again right before the signal ([`Number`]).
#[cfg(unix)]
impl Holds for Native {
    fn hold(&self, pid: u32) -> io::Result<Option<Box<dyn Held>>> {
        Ok(Some(Box::new(Number(pid))))
    }
}

/// The unique identifier of the process `pid` as the pane host reads it: a Mac's alone (Linux has
/// none, and a process it chose was proved by no identifier).
#[cfg(target_os = "macos")]
fn unique_of(pid: u32) -> Option<u64> {
    i32::try_from(pid)
        .ok()
        .and_then(cf_panes::read_identity)
        .map(|read| read.unique)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn unique_of(_pid: u32) -> Option<u64> {
    None
}

/// A process held by its number.
#[cfg(unix)]
struct Number(u32);

#[cfg(unix)]
impl Held for Number {
    fn started(&self) -> io::Result<Option<u64>> {
        identity(self.0)
            .map(|found| found.map(|identity| identity.started))
            .map_err(|failed: Error| io::Error::other(failed.to_string()))
    }

    fn unique(&self) -> io::Result<Option<u64>> {
        Ok(unique_of(self.0))
    }

    fn end(&self) -> io::Result<()> {
        // Known limit (the chief's decision, T-1542): the check of its start and this signal are
        // two calls, and a process that exits between them and whose number the system gives
        // another at once would be the one signalled; macOS signals a process by its number
        // alone, and Windows, which holds a handle, has no such gap.
        match crate::process::signal(self.0, crate::process::Signal::Kill) {
            // One that ended by itself in between is ended.
            Err(failed) if failed.raw_os_error() != Some(nix::libc::ESRCH) => Err(failed),
            _ => Ok(()),
        }
    }

    fn pause(&self, pause: super::Pause) -> io::Result<()> {
        let signal = match pause {
            super::Pause::Stop => crate::process::Signal::Stop,
            super::Pause::Continue => crate::process::Signal::Continue,
        };
        crate::process::signal(self.0, signal)
    }
}

/// Windows ends a process through a handle to it ([`win::Handle`]).
#[cfg(windows)]
impl Holds for Native {
    fn hold(&self, pid: u32) -> io::Result<Option<Box<dyn Held>>> {
        Ok(win::open_to_end(pid)?.map(|handle| Box::new(handle) as Box<dyn Held>))
    }
}

#[cfg(windows)]
impl Held for win::Handle {
    fn started(&self) -> io::Result<Option<u64>> {
        self.created()
    }

    fn unique(&self) -> io::Result<Option<u64>> {
        // The handle is the proof: it holds the process, and its creation time is checked on it.
        Ok(None)
    }

    fn end(&self) -> io::Result<()> {
        self.terminate()
    }

    fn pause(&self, _pause: super::Pause) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows has no stop of a process",
        ))
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::io;

    use nix::libc;

    use super::super::{ps, refined_by_bsd, Bsd, Identity, Link, Running};
    use crate::{Error, Result};

    /// Every process `ps` lists, each with when it began as the system says it to the
    /// microsecond.
    pub(in super::super) fn processes() -> Result<Vec<Running>> {
        Ok(refined_by_bsd(ps(None)?, bsd, lstart_second))
    }

    /// The second a start (`micros`, since 1970) falls in, as `ps`'s `lstart` says it: the
    /// machine's own time of day, as `YYYYMMDDhhmmss` ([`super::super::ps`]). None where it is no
    /// time.
    pub(in super::super) fn lstart_second(micros: u64) -> Option<u64> {
        let at = jiff::Timestamp::from_microsecond(i64::try_from(micros).ok()?).ok()?;
        at.to_zoned(jiff::tz::TimeZone::system())
            .strftime("%Y%m%d%H%M%S")
            .to_string()
            .parse()
            .ok()
    }

    /// Which process has the number `pid` now: none where none has.
    pub(in super::super) fn identity(pid: u32) -> Result<Option<Identity>> {
        let read = bsd(pid).map_err(|source| Error::Program {
            action: "read the process with",
            program: "proc_pidinfo".to_owned(),
            source,
        })?;
        Ok(read.map(|bsd| Identity {
            pid,
            started: bsd.started,
        }))
    }

    /// What `proc_pidinfo` says of the process `pid`, as one read: its parent, when it began, and
    /// the identifiers the pane host reads ([`cf_panes::read_identity`]: its own unique identifier
    /// and its forker's), the read of its short information made again after them. A process
    /// whose number another has in between, or that execs in between, is not read: none. A process
    /// the host will not read (another user's) has the rest and proves nothing ([`Link::Unread`]).
    pub(in super::super) fn bsd(pid: u32) -> io::Result<Option<Bsd>> {
        let Some(first) = short(pid)? else {
            return Ok(None);
        };
        let identity = i32::try_from(pid).ok().and_then(cf_panes::read_identity);
        let Some(second) = short(pid)? else {
            return Ok(None);
        };
        if first != second {
            return Ok(None);
        }
        let link = match identity {
            // The host's read and this one name one birth, to the microsecond.
            Some(read) if read.born == (first.started / 1_000_000, first.started % 1_000_000) => {
                Link::Forked {
                    own: read.unique,
                    forker: read.starter,
                }
            }
            Some(_) => return Ok(None),
            None => Link::Unread,
        };
        Ok(Some(Bsd { link, ..first }))
    }

    /// What `proc_pidinfo` (`PROC_PIDTBSDINFO`) says of the process `pid`: none where no
    /// process has the number, or the one that has it has ended and waits to be reaped,
    /// which nothing ends.
    fn short(pid: u32) -> io::Result<Option<Bsd>> {
        let Ok(number) = i32::try_from(pid) else {
            return Ok(None);
        };
        #[allow(unsafe_code)]
        // SAFETY: proc_bsdinfo is plain numbers and arrays of them, for which all zeroes
        // is a value.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).unwrap_or(i32::MAX);
        #[allow(unsafe_code)]
        // SAFETY: the buffer is `info`, writable for the `size` bytes passed with it, and
        // proc_pidinfo writes no more than that.
        let read = unsafe {
            libc::proc_pidinfo(
                number,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        if read <= 0 {
            let failed = io::Error::last_os_error();
            return if failed.raw_os_error() == Some(libc::ESRCH) {
                Ok(None)
            } else {
                Err(failed)
            };
        }
        if read != size || info.pbi_pid != pid {
            return Err(io::Error::other(format!(
                "proc_pidinfo said {read} bytes of process {} for {pid}",
                info.pbi_pid
            )));
        }
        if info.pbi_status == libc::SZOMB {
            return Ok(None);
        }
        Ok(Some(Bsd {
            ppid: info.pbi_ppid,
            started: microseconds(info.pbi_start_tvsec, info.pbi_start_tvusec),
            link: Link::Unread,
        }))
    }

    /// The folder the process `pid` runs in, as `proc_pidinfo` (`PROC_PIDVNODEPATHINFO`) says
    /// it: the real path, with no link in it. None where no process has the number, where the
    /// system will not say (a process that is not ours) and where the answer is no whole path.
    pub(in super::super) fn working_folder(pid: u32) -> Option<std::path::PathBuf> {
        use std::os::unix::ffi::OsStringExt;

        let number = i32::try_from(pid).ok()?;
        #[allow(unsafe_code)]
        // SAFETY: proc_vnodepathinfo is plain numbers and arrays of them, for which all zeroes
        // is a value.
        let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
        let size = i32::try_from(std::mem::size_of::<libc::proc_vnodepathinfo>()).ok()?;
        #[allow(unsafe_code)]
        // SAFETY: the buffer is `info`, writable for the `size` bytes passed with it, and
        // proc_pidinfo writes no more than that.
        let read = unsafe {
            libc::proc_pidinfo(
                number,
                libc::PROC_PIDVNODEPATHINFO,
                0,
                (&mut info as *mut libc::proc_vnodepathinfo).cast(),
                size,
            )
        };
        if read != size {
            return None;
        }
        // The path is in a block of characters (libc spells it as 32 rows of 32), ended by a 0.
        let bytes: Vec<u8> = info
            .pvi_cdir
            .vip_path
            .iter()
            .flatten()
            .map(|letter| *letter as u8)
            .take_while(|letter| *letter != 0)
            .collect();
        (!bytes.is_empty()).then(|| std::path::PathBuf::from(std::ffi::OsString::from_vec(bytes)))
    }

    /// When a process began, as `proc_pidinfo` says it (seconds since 1970 and the
    /// microseconds of the second), in microseconds since 1970.
    pub(in super::super) fn microseconds(seconds: u64, micros: u64) -> u64 {
        seconds * 1_000_000 + micros
    }
}

#[cfg(windows)]
mod win {
    use std::io;

    use windows_sys::Wdk::System::SystemInformation::NtQuerySystemInformation;
    use windows_sys::Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessBasicInformation, ProcessCommandLineInformation,
    };
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, UNICODE_STRING, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
        WaitForSingleObject, PROCESS_ACCESS_RIGHTS, PROCESS_BASIC_INFORMATION, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };

    use super::super::{
        refined_by_handle, table_of_system, text_in_buffer, with_cygwin_parents, Identity, Opened,
        Running,
    };
    use crate::process::Run;
    use crate::{Error, Result};

    /// Every process of Windows' table, each with its creation time and its creator as its own
    /// handle says them ([`refined_by_handle`]), and what Git Bash says of the parent of a Cygwin
    /// program whose own has gone, as a description only ([`with_cygwin_parents`]). Every handle
    /// stays open until all of them are read, so that no number of the reading is another
    /// process's before the end of it: one reading, with exact creation times, which the edges of
    /// the window's tree are proved by ([`super::super::descending_from`]).
    pub(in super::super) fn processes() -> Result<Vec<Running>> {
        let held: std::cell::RefCell<std::collections::HashMap<u32, Handle>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
        let listed = refined_by_handle(system_table()?, |pid| {
            match open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)? {
                Some(handle) => {
                    let opened = handle.opened()?;
                    held.borrow_mut().insert(pid, handle);
                    Ok(opened)
                }
                None => Ok(None),
            }
        });
        // The handles are still open: each says what its image is and what it was started with
        // (the table says neither), and, after `ps -W` was read, its creation time again. A
        // process that cannot be opened has neither: it is none the window's own, which no
        // command it runs can be told by.
        let listed = listed
            .into_iter()
            .map(|mut process| {
                if let Some(handle) = held.borrow().get(&process.identity.pid) {
                    process.exe = handle.image_path();
                    process.command = handle.command_line().unwrap_or_default();
                }
                process
            })
            .collect();
        let rows = with_cygwin_parents(listed, cygwin_ps, |pid| {
            held.borrow()
                .get(&pid)
                .and_then(|handle| handle.created().ok().flatten())
        });
        drop(held);
        Ok(rows)
    }

    // The table's offsets are of a 64-bit Windows: a build for another is refused here.
    const _: () = assert!(std::mem::size_of::<usize>() == 8);

    /// The class `SystemProcessInformation` of `NtQuerySystemInformation`.
    const SYSTEM_PROCESS_INFORMATION: i32 = 5;
    /// `STATUS_INFO_LENGTH_MISMATCH` and `STATUS_BUFFER_TOO_SMALL`: the room was not enough.
    const TOO_SMALL: [i32; 2] = [0xC000_0004_u32 as i32, 0xC000_0023_u32 as i32];

    /// Every process of the machine as the system's own table says it (number, creator, creation
    /// time), in one call and with no program started: read in a buffer that grows to what the
    /// system says it needs (the table grows between two calls, so room is added to it).
    fn system_table() -> Result<Vec<Running>> {
        let mut room = 1usize << 20;
        for _ in 0..8 {
            // Words, so that the entries laid over it are aligned.
            let mut buffer = vec![0u64; room.div_ceil(8)];
            let mut needed = 0u32;
            #[allow(unsafe_code)]
            // SAFETY: the buffer is writable for the size passed with it.
            let status = unsafe {
                NtQuerySystemInformation(
                    SYSTEM_PROCESS_INFORMATION,
                    buffer.as_mut_ptr().cast(),
                    u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX),
                    &mut needed,
                )
            };
            if status >= 0 {
                // SAFETY of the view: the buffer is initialised, and its bytes are read at their
                // length.
                #[allow(unsafe_code)]
                let bytes = unsafe {
                    std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), buffer.len() * 8)
                };
                return Ok(table_of_system(bytes));
            }
            if !TOO_SMALL.contains(&status) {
                return Err(Error::Program {
                    action: "list the processes with",
                    program: "NtQuerySystemInformation".to_owned(),
                    source: io::Error::other(format!("status {status:#x}")),
                });
            }
            room = room.max(needed as usize).saturating_add(1 << 18);
        }
        Err(Error::Program {
            action: "list the processes with",
            program: "NtQuerySystemInformation".to_owned(),
            source: io::Error::other("the table kept growing"),
        })
    }

    /// What `ps -W` of the folder `folder` says (Git Bash's own, which lists the Cygwin
    /// parent of each of its programs): none where no `ps` is there or it did not answer.
    fn cygwin_ps(folder: &str) -> Option<String> {
        let folder = std::path::Path::new(folder);
        let ps = folder.join("ps.exe");
        // Git Bash's `ps` runs on a Cygwin layer that sits beside it: one without is not run.
        let layer = ["msys-2.0.dll", "cygwin1.dll"]
            .iter()
            .any(|dll| folder.join(dll).is_file());
        if !ps.is_file() || !layer {
            return None;
        }
        let ran = Run::new(ps).args(["-W"]).run().ok()?;
        (ran.code == Some(0)).then_some(ran.stdout)
    }

    /// Which process has the number `pid` now, read on a handle to it: none where none
    /// has, or the one that has it has exited.
    pub(in super::super) fn identity(pid: u32) -> Result<Option<Identity>> {
        let read = open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)
            .and_then(|handle| handle.map_or(Ok(None), |handle| handle.created()))
            .map_err(|source| Error::Program {
                action: "read the process with",
                program: "OpenProcess".to_owned(),
                source,
            })?;
        Ok(read.map(|started| Identity { pid, started }))
    }

    /// An open handle to a process: while it is open, the process's number is no other
    /// process's, though the process may end.
    pub(in super::super) struct Handle(HANDLE);

    /// Opens the process `pid` to be read and ended: none where no process has the number.
    pub(in super::super) fn open_to_end(pid: u32) -> io::Result<Option<Handle>> {
        open(
            pid,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
        )
    }

    /// Opens the process `pid` with `rights`: none where no process has the number.
    fn open(pid: u32, rights: PROCESS_ACCESS_RIGHTS) -> io::Result<Option<Handle>> {
        #[allow(unsafe_code)]
        // SAFETY: OpenProcess takes plain values and returns a handle or null.
        let handle = unsafe { OpenProcess(rights, 0, pid) };
        if handle.is_null() {
            let failed = io::Error::last_os_error();
            return if failed.raw_os_error() == i32::try_from(ERROR_INVALID_PARAMETER).ok() {
                Ok(None)
            } else {
                Err(failed)
            };
        }
        Ok(Some(Handle(handle)))
    }

    impl Handle {
        /// Whether the process has exited: its handle is signalled once it has.
        fn exited(&self) -> bool {
            #[allow(unsafe_code)]
            // SAFETY: the handle is open, with the right to wait on it; a wait of no time
            // only asks.
            let waited = unsafe { WaitForSingleObject(self.0, 0) };
            waited == WAIT_OBJECT_0
        }

        /// When the process began, in 100 ns since 1601 (its creation time): none where it
        /// has exited.
        pub(in super::super) fn created(&self) -> io::Result<Option<u64>> {
            if self.exited() {
                return Ok(None);
            }
            let (mut created, mut exited, mut kernel, mut user) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            #[allow(unsafe_code)]
            // SAFETY: the handle is open, and the four structures are writable.
            let read = unsafe {
                GetProcessTimes(self.0, &mut created, &mut exited, &mut kernel, &mut user)
            };
            if read == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Some(
                (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
            ))
        }

        /// When the process was created and the number of the process that created it, read on
        /// this handle: none where it has exited. The creator is none where the system will
        /// not say.
        pub(in super::super) fn opened(&self) -> io::Result<Option<Opened>> {
            let Some(created) = self.created()? else {
                return Ok(None);
            };
            let mut information = PROCESS_BASIC_INFORMATION::default();
            let mut written = 0u32;
            #[allow(unsafe_code)]
            // SAFETY: the handle is open with the right to query it, and the structure is the one
            // the information class names, at its size.
            let status = unsafe {
                NtQueryInformationProcess(
                    self.0,
                    ProcessBasicInformation,
                    (&mut information as *mut PROCESS_BASIC_INFORMATION).cast(),
                    std::mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32,
                    &mut written,
                )
            };
            let creator = (status >= 0)
                .then(|| u32::try_from(information.InheritedFromUniqueProcessId).ok())
                .flatten();
            Ok(Some(Opened { created, creator }))
        }

        /// The whole path the process's program runs from, as the system keeps it: none where it
        /// will not say.
        fn image_path(&self) -> Option<String> {
            // The path a program may have is longer than the one most have.
            for room in [1_024usize, 32_768] {
                let mut path = vec![0u16; room];
                let mut length = u32::try_from(room).ok()?;
                #[allow(unsafe_code)]
                // SAFETY: the buffer is writable for `length` units, which the system lowers to
                // the units it wrote.
                let read = unsafe {
                    QueryFullProcessImageNameW(
                        self.0,
                        PROCESS_NAME_WIN32,
                        path.as_mut_ptr(),
                        &mut length,
                    )
                };
                if read != 0 {
                    return Some(String::from_utf16_lossy(&path[..length as usize]));
                }
            }
            None
        }

        /// The command line the process was started with, as the system keeps it and says it to
        /// a handle with the right to query: no memory of the process is read. None where it
        /// will not say.
        fn command_line(&self) -> Option<String> {
            let mut needed = 0u32;
            #[allow(unsafe_code)]
            // SAFETY: no buffer and no room for one: the call only says how much it needs.
            unsafe {
                NtQueryInformationProcess(
                    self.0,
                    ProcessCommandLineInformation,
                    std::ptr::null_mut(),
                    0,
                    &mut needed,
                )
            };
            if (needed as usize) < std::mem::size_of::<UNICODE_STRING>() {
                return None;
            }
            // Words, so that the string laid over it is aligned.
            let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
            #[allow(unsafe_code)]
            // SAFETY: the buffer is writable for the size passed with it.
            let status = unsafe {
                NtQueryInformationProcess(
                    self.0,
                    ProcessCommandLineInformation,
                    buffer.as_mut_ptr().cast(),
                    u32::try_from(buffer.len() * 8).ok()?,
                    &mut needed,
                )
            };
            if status < 0 {
                return None;
            }
            let text = buffer.as_ptr().cast::<UNICODE_STRING>();
            #[allow(unsafe_code)]
            // SAFETY: on success the buffer holds a UNICODE_STRING at its start (it is at least
            // that big, checked above), read by value.
            let (length, chars) = unsafe { ((*text).Length as usize, (*text).Buffer) };
            // The text is the kernel's to place, in the buffer given: where it says it lies outside
            // that, or is not whole units, it is no command line.
            let (offset, units) = text_in_buffer(
                buffer.as_ptr() as usize,
                buffer.len() * 8,
                std::mem::size_of::<UNICODE_STRING>(),
                length,
                chars as usize,
            )?;
            #[allow(unsafe_code)]
            // SAFETY: `text_in_buffer` put `units` 16-bit units at `offset` inside the buffer, on a
            // 2-byte boundary, and the buffer is alive and not written meanwhile.
            let units = unsafe {
                std::slice::from_raw_parts(
                    buffer.as_ptr().cast::<u8>().add(offset).cast::<u16>(),
                    units,
                )
            };
            Some(String::from_utf16_lossy(units))
        }

        /// Ends the process, through this handle: one that exited meanwhile is ended.
        pub(in super::super) fn terminate(&self) -> io::Result<()> {
            #[allow(unsafe_code)]
            // SAFETY: the handle is open, with the right to end the process.
            let ended = unsafe { TerminateProcess(self.0, 1) };
            if ended != 0 || self.exited() {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            #[allow(unsafe_code)]
            // SAFETY: the handle was opened by `open`, is not null, and is closed only here.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests;
