//! Which processes hold a file open: what names the process behind a lock whose words a
//! program keeps locked from every other's reading (Windows' byte-range locks are no
//! advice, as Unix's are: Muse's `.session.lock` there, T-792).

use std::path::Path;

/// The processes that have the file `path` open, as Windows' Restart Manager (a
/// documented API) says them: none where it says nothing of it, and none on Unix, where a
/// lock keeps no reader out and its file says who holds it.
pub fn holders(path: &Path) -> Vec<u32> {
    #[cfg(windows)]
    {
        windows::holders(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Vec::new()
    }
}

#[cfg(windows)]
mod windows {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::RestartManager::{
        RmEndSession, RmGetList, RmRegisterResources, RmStartSession, CCH_RM_SESSION_KEY,
        RM_PROCESS_INFO,
    };

    /// How many holders one file is asked for at most: a lock has one, a log a few.
    const MOST: usize = 64;

    pub(super) fn holders(path: &Path) -> Vec<u32> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
        let mut session = 0u32;
        #[allow(unsafe_code)]
        // SAFETY: the session's key is written into `key`, ours, as long as the API asks;
        // the one file's name is `wide`, ended by its 0, alive across the call; RmGetList
        // writes at most `room` entries into `found`, ours, and the counts into ours. The
        // session is ended once, whatever came of it.
        unsafe {
            if RmStartSession(&mut session, 0, key.as_mut_ptr()) != ERROR_SUCCESS {
                return Vec::new();
            }
            let names = [wide.as_ptr()];
            let mut found = vec![RM_PROCESS_INFO::default(); MOST];
            let (mut needed, mut room, mut reasons) = (0u32, MOST as u32, 0u32);
            let listed = RmRegisterResources(
                session,
                1,
                names.as_ptr(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
            ) == ERROR_SUCCESS
                && matches!(
                    RmGetList(
                        session,
                        &mut needed,
                        &mut room,
                        found.as_mut_ptr(),
                        &mut reasons
                    ),
                    ERROR_SUCCESS | ERROR_MORE_DATA
                );
            RmEndSession(session);
            if !listed {
                return Vec::new();
            }
            found
                .iter()
                .take((room as usize).min(MOST))
                .map(|holder| holder.Process.dwProcessId)
                .collect()
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::holders;

    /// A file a child holds open is held by that child, and one nobody holds by none.
    #[test]
    #[allow(clippy::disallowed_methods)] // The test's own program, ended by it.
    fn a_file_a_process_holds_open_is_held_by_it() {
        let folder = tempfile::tempdir().unwrap();
        let held = folder.path().join("held.txt");
        std::fs::write(&held, "").unwrap();
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&held)
            .unwrap();
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "ping -n 30 127.0.0.1 >nul"])
            .stdout(file)
            .spawn()
            .unwrap();
        // The child holds the file through its standard output; this process let its own go.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !holders(&held).contains(&child.id()) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(holders(&held).contains(&child.id()), "{:?}", holders(&held));
        child.kill().unwrap();
        child.wait().unwrap();
        let alone = folder.path().join("alone.txt");
        std::fs::write(&alone, "").unwrap();
        assert!(holders(&alone).is_empty());
    }
}
