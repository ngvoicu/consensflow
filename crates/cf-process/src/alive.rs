//! Whether a process is there (`process.kill(pid, 0)`, libuv's `uv_kill`):
//! how ConsensFlow tells a harness's live status file from one a process
//! that has gone left behind.

/// Whether the process `pid` is alive: one this user may not signal is
/// alive all the same, as Node's `EPERM` said. A pid no 32-bit integer
/// holds is none, as Node refused it; 0 is none here, where the system
/// takes it for the caller's own process group (Unix) or the caller itself
/// (Windows), never another process.
pub fn alive(pid: u32) -> bool {
    if pid == 0 || i32::try_from(pid).is_err() {
        return false;
    }
    #[cfg(unix)]
    {
        unix::alive(pid)
    }
    #[cfg(windows)]
    {
        windows::alive(pid)
    }
}

#[cfg(unix)]
mod unix {
    /// `kill(pid, 0)`: there, or there and this user's to signal not.
    pub(super) fn alive(pid: u32) -> bool {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        #[allow(unsafe_code)]
        // SAFETY: kill takes two integers and touches no memory of ours; the
        // signal 0 asks only whether the process is there.
        let answered = unsafe { libc::kill(pid, 0) };
        answered == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_PRIVILEGE_NOT_HELD, HANDLE,
        STILL_ACTIVE, WAIT_FAILED, WAIT_TIMEOUT, WIN32_ERROR,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, WaitForSingleObject, PROCESS_QUERY_INFORMATION,
        PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };

    /// Whether a failure is one libuv names `EPERM` (`uv_translate_sys_error`,
    /// `src/win/error.c`): the process is there, and this user may not
    /// touch it.
    fn denied(error: WIN32_ERROR) -> bool {
        matches!(error, ERROR_ACCESS_DENIED | ERROR_PRIVILEGE_NOT_HELD)
    }

    /// libuv's `uv_kill(pid, 0)` (`src/win/process.c` of the libuv Node
    /// v26.8.1 bundles): the process opened as it opens it, then alive while
    /// it has not exited, which its exit code says unless the process exited
    /// with the very code that means still active (259): its handle then
    /// says whether it was signalled.
    pub(super) fn alive(pid: u32) -> bool {
        #[allow(unsafe_code)]
        // SAFETY: OpenProcess takes plain values and returns a handle or
        // null; GetLastError reads this thread's own error.
        let handle = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            #[allow(unsafe_code)]
            // SAFETY: as above.
            return denied(unsafe { GetLastError() });
        }
        let alive = running(handle);
        #[allow(unsafe_code)]
        // SAFETY: the handle is open, and closed once, here.
        unsafe {
            CloseHandle(handle);
        }
        alive
    }

    /// Whether the open process `handle` has not exited.
    fn running(handle: HANDLE) -> bool {
        let mut status = 0;
        #[allow(unsafe_code)]
        // SAFETY: the handle is open, and `status` outlives the call.
        if unsafe { GetExitCodeProcess(handle, &mut status) } == 0 {
            #[allow(unsafe_code)]
            // SAFETY: reads this thread's own error.
            return denied(unsafe { GetLastError() });
        }
        if status != STILL_ACTIVE as u32 {
            return false;
        }
        #[allow(unsafe_code)]
        // SAFETY: the handle is open, and was opened with SYNCHRONIZE.
        match unsafe { WaitForSingleObject(handle, 0) } {
            WAIT_TIMEOUT => true,
            #[allow(unsafe_code)]
            // SAFETY: reads this thread's own error.
            WAIT_FAILED => denied(unsafe { GetLastError() }),
            // Signalled: it exited, its code 259; anything else is libuv's
            // unknown error, which is not `EPERM`.
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_alive_and_one_long_gone_is_not() {
        assert!(alive(std::process::id()));
        // macOS's pids stop below it; Linux's and Windows' never reach it here.
        assert!(!alive(999_999));
        assert!(!alive(0));
        assert!(!alive(u32::MAX), "no 32-bit integer holds it");
    }

    #[cfg(unix)]
    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it waits for.
    fn a_child_reaped_is_gone() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!alive(pid));
    }

    #[cfg(windows)]
    #[test]
    #[allow(clippy::disallowed_methods)] // The test starts what it waits for.
    fn a_child_that_exited_with_the_code_still_active_reads_as_is_gone() {
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "exit 259"])
            .spawn()
            .unwrap();
        let pid = child.id();
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(259));
        // The test still holds its handle, so the process is there to open.
        assert!(!alive(pid));
    }
}
