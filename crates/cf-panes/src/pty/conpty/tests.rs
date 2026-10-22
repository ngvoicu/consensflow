//! The field's shape (`issues/windows-job-object-is-assigned-after-the-start.md`): a
//! launcher, as a `.cmd` shim or an npm wrapper is, that starts a child at once, and the
//! window's close, which ends that child. Then what the reviews of that work found (T-167,
//! T-168): a close that may wait for ever on an output held unread, an environment value
//! that is not Unicode, a library let go, a NUL in the program, an empty environment.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::sync::{mpsc, Arc};
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

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

use super::closes::{self, Closed};
use super::{environment_block, wide, Host, HOLD_STARTED};
use crate::pty::conpty_test::{open, powershell, read_until, system};
use crate::pty::{serial_pty_test, PaneEnvironment, PaneTable, WATCH_FAILS};

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

/// How long a close is given before the test says it waited for ever.
const CLOSE_BOUND: Duration = Duration::from_secs(10);

/// A program that prints far more than a pipe holds, at once, and waits.
fn chatty() -> Vec<String> {
    powershell("1..4000 | ForEach-Object { 'x' * 200 }; Start-Sleep 1000")
}

fn size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// What `work` gives, unless it takes longer than [`CLOSE_BOUND`]: a close that waits for
/// ever is left waiting on a thread of its own, and the test fails.
fn within_the_bound<T: Send + 'static>(what: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, received) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    received
        .recv_timeout(CLOSE_BOUND)
        .unwrap_or_else(|_| panic!("{what} waited longer than {CLOSE_BOUND:?}"))
}

/// What of `console` has been closed, once it has been: the pane is ended on the closer's
/// thread, or on a reaper's.
fn closed(console: u64) -> Vec<Closed> {
    let deadline = Instant::now() + CLOSE_BOUND;
    loop {
        let closed = closes::of(console);
        if closed.contains(&Closed::Console) || Instant::now() > deadline {
            return closed;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A window whose exit watcher cannot start is ended as it opens, while its output is a
/// reader no one reads and its program has printed more than the pipe holds. Before
/// Windows 11 24H2 a console closed while its output was held open waited for its host,
/// which waited to write: the reader and the master's end of the output go first, then
/// the console, and the open fails in its bound. (235547a0 closed the console with the
/// master's end of the output open, and the reader after the close.)
#[test]
fn a_window_whose_exit_watcher_fails_lets_go_of_its_output_before_its_console_is_closed() {
    let _pty_guard = serial_pty_test();
    let console = closes::upcoming();
    let table = Arc::new(PaneTable::new());
    let opening = Arc::clone(&table);
    let opened = within_the_bound("a window that failed as it opened", move || {
        WATCH_FAILS.set(true);
        let opened = opening.open_streamed(
            &std::env::temp_dir(),
            &chatty(),
            PaneEnvironment::new(&HashMap::new(), &[]),
            size(),
            1 << 20,
        );
        WATCH_FAILS.set(false);
        opened.map(|_| ()).map_err(|error| error.to_string())
    });
    assert!(
        opened.is_err_and(|error| error.contains("planted")),
        "the window opened"
    );
    assert_eq!(
        closed(console),
        [Closed::Output, Closed::Output, Closed::Console],
        "the reader's and the master's ends of the output go before the console"
    );
}

/// A window closed before the cursor query its console asks first is answered, with its
/// output unread past a small backlog: the close ends in its bound.
#[test]
fn a_window_closed_before_its_cursor_query_is_answered_closes_in_its_bound() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let (key, _output) = open(&table, &chatty(), &HashMap::new(), 64);
    std::thread::sleep(Duration::from_secs(2));
    let closing = Arc::clone(&table);
    within_the_bound(
        "the close of a window whose cursor query was not answered",
        move || closing.kill(&key).map_err(|error| error.to_string()),
    )
    .expect("close the window");
}

/// A variable of the host's whose value is not Unicode (a lone surrogate) reaches the
/// window's program as the host has it, as portable-pty passed it on. (235547a0 left it
/// out.)
#[test]
fn a_value_that_is_not_unicode_reaches_the_program_as_it_is() {
    let _pty_guard = serial_pty_test();
    let value = OsString::from_wide(&[0xD800, u16::from(b'A')]);
    std::env::set_var("CF_CONPTY_SURROGATE", &value);
    let table = Arc::new(PaneTable::new());
    let (key, output) = open(
        &table,
        &powershell(
            "$v = [Environment]::GetEnvironmentVariable('CF_CONPTY_SURROGATE'); \
             if ($null -eq $v) { Write-Output 'CODES=none#' } else { \
             Write-Output (\"CODES=\" + (($v.ToCharArray() | ForEach-Object { '{0:X}' -f [int]$_ }) -join ',') + '#') }",
        ),
        &HashMap::new(),
        1 << 20,
    );
    let text = read_until(&table, &key, &output, "#");
    std::env::remove_var("CF_CONPTY_SURROGATE");
    let _ = table.kill(&key);
    assert!(text.contains("CODES=D800,41#"), "{text:?}");
}

/// A library that has no pseudoconsole is let go, not kept for the life of the app.
/// (235547a0 kept it.)
#[test]
fn a_library_without_a_pseudoconsole_is_let_go() {
    let name = wide(std::ffi::OsStr::new("msimg32.dll"));
    // SAFETY: a NUL-terminated name; the call only asks.
    let loaded = || !unsafe { GetModuleHandleW(name.as_ptr()) }.is_null();
    assert!(!loaded(), "the test needs a library it has not loaded");
    assert!(Host::load("msimg32.dll").is_none());
    assert!(
        !loaded(),
        "the library without a pseudoconsole stayed loaded"
    );
}

/// A program named with a NUL in it is refused: the system would read the name as far
/// as the NUL, and start another program. (235547a0 started `cmd.exe`.)
#[test]
fn a_program_with_a_nul_in_its_name_is_refused() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let opened = table.open_streamed(
        &std::env::temp_dir(),
        &[format!("{}\0x", system("cmd.exe"))],
        PaneEnvironment::new(&HashMap::new(), &[]),
        size(),
        1 << 20,
    );
    let refused = match opened {
        Ok(streamed) => {
            let _ = table.kill(&streamed.key);
            None
        }
        Err(error) => Some(error.to_string()),
    };
    assert!(
        refused
            .as_deref()
            .is_some_and(|error| error.contains("NUL")),
        "{refused:?}"
    );
}

/// An environment with nothing in it is ended by two NULs, as Windows reads a block.
/// (235547a0 wrote one.)
#[test]
fn an_empty_environment_is_two_nuls() {
    let mut command = portable_pty::CommandBuilder::new("program");
    command.env_clear();
    assert_eq!(environment_block(&command), [0, 0]);
}
