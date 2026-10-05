//! Whether a process is there (`process.kill(pid, 0)`, libuv's `uv_kill`):
//! how ConsensFlow tells a harness's live status file from one a process
//! that has gone left behind.

/// Whether the process `pid` is alive: one this user may not signal is
/// alive all the same, as Node's `EPERM` said.
pub fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        if pid <= 0 {
            // 0 and the negatives name process groups, never one process.
            return false;
        }
        #[allow(unsafe_code)]
        // SAFETY: kill takes two integers and touches no memory of ours; the
        // signal 0 asks only whether the process is there.
        let answered = unsafe { libc::kill(pid, 0) };
        answered == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        windows::alive(pid)
    }
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_TERMINATE,
    };

    /// libuv's: a process it cannot open for want of rights is alive; one
    /// it opens is alive while it has not exited.
    pub(super) fn alive(pid: u32) -> bool {
        #[allow(unsafe_code)]
        // SAFETY: OpenProcess takes plain values and returns a handle or
        // null; the handle is closed below, and GetLastError reads this
        // thread's own error.
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_INFORMATION, 0, pid) };
        if handle.is_null() {
            #[allow(unsafe_code)]
            // SAFETY: as above.
            let error = unsafe { GetLastError() };
            return error == ERROR_ACCESS_DENIED;
        }
        let mut status = 0;
        #[allow(unsafe_code)]
        // SAFETY: the handle is open, and `status` outlives the call.
        let read = unsafe { GetExitCodeProcess(handle, &mut status) };
        #[allow(unsafe_code)]
        // SAFETY: the handle is open and closed once, here.
        unsafe {
            CloseHandle(handle);
        }
        read != 0 && status == STILL_ACTIVE as u32
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
}
