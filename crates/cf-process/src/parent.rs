//! Which process started a process: what tells a program's child from another's where a
//! program cannot become the one it starts (Windows has no `exec`: a launcher runs what it
//! launches as its child).

/// The process that started `pid` (on Unix, the one it was given to if that one ended):
/// none where the system says nothing of `pid`, as of one that has gone.
pub fn parent(pid: u32) -> Option<u32> {
    #[cfg(unix)]
    {
        let process = crate::table::process(i32::try_from(pid).ok()?)?;
        u32::try_from(process.parent).ok()
    }
    #[cfg(windows)]
    {
        windows::parent(pid)
    }
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    /// The parent a snapshot of the system's processes (Toolhelp32, a documented API)
    /// says `pid` has. Windows keeps the number of the process that started it, which
    /// may have ended since.
    pub(super) fn parent(pid: u32) -> Option<u32> {
        let size = u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).ok()?;
        #[allow(unsafe_code)]
        // SAFETY: the snapshot's handle is closed once, below, whatever came of the walk;
        // Process32FirstW and Process32NextW write the entry they are given, ours, whose
        // size it says, and no other memory of ours.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut entry = PROCESSENTRY32W {
                dwSize: size,
                ..PROCESSENTRY32W::default()
            };
            let mut found = None;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                if entry.th32ProcessID == pid {
                    found = Some(entry.th32ParentProcessID);
                    break;
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            found
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parent;

    /// A child this process starts names it as its parent, on every platform; a process
    /// that is not there has none.
    #[test]
    #[allow(clippy::disallowed_methods)] // The test's own program, ended by it.
    fn a_child_s_parent_is_the_process_that_started_it() {
        let mut command = if cfg!(windows) {
            let mut command = std::process::Command::new("cmd.exe");
            command.args(["/d", "/c", "ping -n 3 127.0.0.1 >nul"]);
            command
        } else {
            let mut command = std::process::Command::new("sleep");
            command.arg("2");
            command
        };
        let mut child = command.spawn().unwrap();
        assert_eq!(parent(child.id()), Some(std::process::id()));
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(parent(u32::MAX - 1), None);
    }
}
