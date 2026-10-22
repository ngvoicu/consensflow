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

#[cfg(target_os = "macos")]
pub(super) use mac::{identity, processes};
#[cfg(windows)]
pub(super) use win::{identity, processes};

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

    fn end(&self) -> io::Result<()> {
        match crate::process::signal(self.0, crate::process::Signal::Kill) {
            // One that ended by itself in between is ended.
            Err(failed) if failed.raw_os_error() != Some(nix::libc::ESRCH) => Err(failed),
            _ => Ok(()),
        }
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

    fn end(&self) -> io::Result<()> {
        self.terminate()
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::io;

    use nix::libc;

    use super::super::{ps, refined_by_bsd, Bsd, Identity, Running};
    use crate::{Error, Result};

    /// Every process `ps` lists, each with when it began as the system says it to the
    /// microsecond.
    pub(in super::super) fn processes() -> Result<Vec<Running>> {
        Ok(refined_by_bsd(ps(None)?, bsd))
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

    /// What `proc_pidinfo` (`PROC_PIDTBSDINFO`) says of the process `pid`: none where no
    /// process has the number, or the one that has it has ended and waits to be reaped,
    /// which nothing ends.
    pub(in super::super) fn bsd(pid: u32) -> io::Result<Option<Bsd>> {
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
        }))
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

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_ACCESS_RIGHTS,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };

    use super::super::{refined_by_handle, with_cygwin_parents, Identity, Running};
    use crate::process::Run;
    use crate::{Error, Result};

    /// Every process of Windows' table, each with its creation time as its own handle says
    /// it ([`refined_by_handle`]), and the parent Git Bash says for a Cygwin program whose
    /// own has gone ([`with_cygwin_parents`]).
    pub(in super::super) fn processes() -> Result<Vec<Running>> {
        let listed = refined_by_handle(super::super::table()?, |pid| {
            match open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)? {
                Some(handle) => handle.created(),
                None => Ok(None),
            }
        });
        Ok(with_cygwin_parents(listed, cygwin_ps))
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
        let ran = Run::new(ps).args(["-W"]).inheriting_env().run().ok()?;
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
