//! The field's shape (`issues/windows-job-object-is-assigned-after-the-start.md`): a
//! launcher, as a `.cmd` shim or an npm wrapper is, that starts a child at once, and the
//! window's close, which ends that child.

use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use portable_pty::PtySize;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

use super::HOLD_STARTED;
use crate::pty::conpty_test::{read_until, system};
use crate::pty::{serial_pty_test, PaneEnvironment, PaneTable};

/// How long the launcher's start is held once it has been made: where portable-pty's
/// launcher ran outside the job until the host put it in, and long past the moment `cmd`
/// takes to start its child.
const HOLD: Duration = Duration::from_secs(5);

/// A process the test found, held open by a handle of its own (so that its number stays
/// its own), and ended when the test is done with it, whatever the test found.
struct Found(HANDLE, u32);

impl Found {
    fn runs(&self) -> bool {
        // SAFETY: the handle is open until drop; a wait of no time only asks.
        unsafe { WaitForSingleObject(self.0, 0) != WAIT_OBJECT_0 }
    }
}

impl Drop for Found {
    fn drop(&mut self) {
        // SAFETY: the handle is open, with the right to end the process, and closed once.
        unsafe {
            TerminateProcess(self.0, 1);
            CloseHandle(self.0);
        }
    }
}

/// The process `parent` started that runs `program`, held open, once there is one.
fn child_of(parent: u32, program: &str) -> Found {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        // SAFETY: a snapshot of the system's processes, closed below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        assert_ne!(
            snapshot, INVALID_HANDLE_VALUE,
            "a snapshot of the processes"
        );
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..PROCESSENTRY32W::default()
        };
        let mut found = None;
        // SAFETY: the snapshot is open and `entry` writable, its size set.
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while more && found.is_none() {
            let length = entry
                .szExeFile
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(0);
            let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
            if entry.th32ParentProcessID == parent && name.eq_ignore_ascii_case(program) {
                found = Some(entry.th32ProcessID);
            }
            // SAFETY: as above.
            more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        // SAFETY: the snapshot was opened above and is closed once.
        unsafe { CloseHandle(snapshot) };
        if let Some(pid) = found {
            // SAFETY: the rights a look, a wait and an end need; closed by `Found`.
            let handle = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
                    0,
                    pid,
                )
            };
            assert!(!handle.is_null(), "open the child {pid}");
            return Found(handle, pid);
        }
        assert!(
            Instant::now() < deadline,
            "{parent} started no {program} in 30 s"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A launcher starts a child at once and waits, as a harness does. The child has a console
/// of its own (`start` without `/b`, as a detached child has none of the window's), so that
/// the end of the window's console does not end it: only its job does. The launcher's start
/// is held once it has been made: the child is in the window's job all the same, the job's
/// inventory lists it, and the close ends it. Where the launcher joined its job only after it
/// ran (portable-pty, then the host's `assign`), the child it started first was never the
/// job's: not listed, and it outlived the close.
#[test]
fn a_child_a_launcher_starts_at_once_is_the_jobs_and_the_close_ends_it() {
    let _pty_guard = serial_pty_test();
    let folder = std::env::temp_dir().join(format!("cf-conpty-launcher-{}", std::process::id()));
    fs::create_dir_all(&folder).expect("the launcher's folder");
    let launcher = folder.join("launcher.cmd");
    let ping = system("PING.EXE");
    fs::write(
        &launcher,
        format!(
            "@echo off\r\nstart \"\" /min \"{ping}\" -n 1000 127.0.0.1\r\necho LAUNCHED#\r\npause >nul\r\n"
        ),
    )
    .expect("write the launcher");

    let table = Arc::new(PaneTable::new());
    HOLD_STARTED.set(HOLD);
    let opened = table.open_streamed(
        &folder,
        &[launcher.display().to_string()],
        PaneEnvironment::new(&HashMap::new(), &[]),
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
        1 << 20,
    );
    HOLD_STARTED.set(Duration::ZERO);
    let streamed = opened.expect("open the launcher's window");
    let launched = streamed.pid.expect("the launcher's number");
    read_until(&table, &streamed.key, &streamed.output, "LAUNCHED#");
    let child = child_of(launched, "PING.EXE");
    assert!(child.runs(), "the launcher's child runs");

    let listed = table
        .lock_panes()
        .expect("the table")
        .get(&streamed.key)
        .expect("the launcher's pane")
        .job
        .members()
        .expect("the job's members")
        .iter()
        .any(|member| member.key.pid == child.1);

    table
        .kill(&streamed.key)
        .expect("close the launcher's window");
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.runs() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let survived = child.runs();
    drop(child);
    let _ = fs::remove_dir_all(&folder);
    assert!(
        listed && !survived,
        "the launcher's child: listed by the job {listed}, outlived the close {survived}"
    );
}
