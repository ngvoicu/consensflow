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
use super::environment::{block_of, environment_block, names_of, Unread, SAID};
use super::{wide, Host, HOLD_STARTED};
use crate::pty::conpty_test::{open, powershell, read_until, system, take};
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

/// The process `pid`, held open.
fn held(pid: u32) -> Found {
    // SAFETY: the rights a look, a wait and an end need; closed by `Found`.
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
            0,
            pid,
        )
    };
    assert!(!handle.is_null(), "open the process {pid}");
    Found(handle, pid)
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
            return held(pid);
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

    // The close ends the job, and waits for the launcher to be gone: one that is in no job
    // is never ended, and the close waits on it for ever. So it is waited for in a bound,
    // and the launcher and its child are ended after, whatever the close did.
    let launcher = held(launched);
    let (closing, key) = (Arc::clone(&table), streamed.key.clone());
    let (sender, closed) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(closing.kill(&key).map_err(|error| error.to_string()));
    });
    let ended = closed
        .recv_timeout(CLOSE_BOUND)
        .map(|closed| closed.expect("close the launcher's window"))
        .is_ok();
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.runs() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let survived = child.runs();
    drop((child, launcher));
    if !ended {
        let _ = closed.recv_timeout(CLOSE_BOUND);
    }
    let _ = fs::remove_dir_all(&folder);
    assert!(
        listed && ended && !survived,
        "the launcher's child: listed by the job {listed}, the close ended in its bound \
         {ended}, the child outlived the close {survived}"
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
        [
            Closed::Output,
            Closed::Input,
            Closed::Output,
            Closed::Console
        ],
        "the reader, the window's input and the master's end of the output go before the console"
    );
}

/// A window killed the moment it opens, its output unread past a small backlog, closes
/// in its bound, and lets go of its input and its output before its console is closed.
/// Before Windows 11 24H2 a console that waits on its input for an answer (the cursor
/// query of one made to inherit its cursor) waited for ever on a close that left its
/// input open. (dc3012bb closed the console before the window's input.)
#[test]
fn a_window_killed_as_it_opens_lets_go_of_its_input_before_its_console_and_closes_in_its_bound() {
    let _pty_guard = serial_pty_test();
    let console = closes::upcoming();
    let table = Arc::new(PaneTable::new());
    let (key, _output) = open(&table, &chatty(), &HashMap::new(), 64);
    let closing = Arc::clone(&table);
    within_the_bound("the close of a window killed as it opened", move || {
        closing.kill(&key).map_err(|error| error.to_string())
    })
    .expect("close the window");
    let closed = closed(console);
    let at = |what: Closed| closed.iter().position(|one| *one == what);
    assert!(
        at(Closed::Input) < at(Closed::Console) && at(Closed::Input).is_some(),
        "the window's input goes before its console: {closed:?}"
    );
}

/// A window's console does not ask where the cursor is: a pane is a new window, whose
/// cursor starts at the top left, and a console that asks waits on its input until it is
/// told, which a close before the answer made wait for ever before Windows 11 24H2.
/// (dc3012bb's consoles asked, `ESC [ 6 n`, before the program could print.)
#[test]
fn a_windows_console_does_not_ask_where_the_cursor_is() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let (key, output) = open(
        &table,
        &powershell("Write-Output 'READY#'; Start-Sleep 1000"),
        &HashMap::new(),
        1 << 20,
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut seen = Vec::new();
    while !String::from_utf8_lossy(&seen).contains("READY#") {
        let left = deadline.saturating_duration_since(Instant::now());
        let chunk = output.recv_timeout(left).expect("the window's output");
        seen.extend_from_slice(&chunk.bytes);
        // Answered all the same, so that a console that asks goes on to the program's text.
        take(&table, &key, &chunk);
    }
    let _ = table.kill(&key);
    assert!(
        !seen.windows(4).any(|bytes| bytes == b"\x1b[6n"),
        "the console asked where the cursor is: {:?}",
        String::from_utf8_lossy(&seen)
    );
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

/// A window asked to start in a folder with a NUL in its name is refused: the folder is
/// no folder, and the program would start in the user's. (dc3012bb started it there.)
#[test]
fn a_folder_with_a_nul_in_its_name_is_refused() {
    let _pty_guard = serial_pty_test();
    let table = Arc::new(PaneTable::new());
    let folder = std::env::temp_dir().join("cf-conpty\0folder");
    let opened = table.open_streamed(
        &folder,
        &[system("cmd.exe")],
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
            .is_some_and(|error| error.contains("folder") && error.contains("NUL")),
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

/// The units of `name=value` in `block`, where it has them.
fn holds(block: &[u16], name: &str, value: &[u16]) -> bool {
    let mut entry: Vec<u16> = name.encode_utf16().collect();
    entry.push(u16::from(b'='));
    entry.extend_from_slice(value);
    entry.push(0);
    block
        .windows(entry.len())
        .any(|window| window == entry.as_slice())
}

/// portable-pty 0.9's serde form, as the environment is read from it: an `OsString` is
/// `{"Windows":[u16…]}`, and a builder's names are its map `envs`, each entry's in
/// `preferred_key`, in the order of the builder's keys. A portable-pty that wrote another
/// form fails here first.
#[test]
fn the_names_are_read_from_portable_ptys_serde_form() {
    assert_eq!(
        serde_json::to_value(OsString::from_wide(&[0xD800, u16::from(b'A')])).expect("form"),
        serde_json::json!({ "Windows": [0xD800, 0x41] })
    );
    let mut command = portable_pty::CommandBuilder::new("program");
    command.env_clear();
    command.env("Zed", "1");
    command.env("Cf_Lone", OsString::from_wide(&[0xD800]));
    command.env("alpha", "2");
    let names = names_of(&command).expect("the builder's form");
    assert_eq!(
        names,
        ["alpha", "Cf_Lone", "Zed"].map(OsString::from),
        "its names, by its keys"
    );
}

/// A variable the builder holds and the host's process does not (as one the registry gained
/// after the user logged in), whose value is not Unicode, is in the block as the builder has
/// it. (dc3012bb found its names in the host's environment and the builder's list of
/// Unicode ones, and left this one out.)
#[test]
fn a_variable_only_the_builder_holds_whose_value_is_not_unicode_is_passed_on() {
    let value = [0xD800, u16::from(b'A')];
    let mut command = portable_pty::CommandBuilder::new("program");
    command.env("CF_ONLY_IN_THE_BUILDER", OsString::from_wide(&value));
    #[allow(clippy::disallowed_methods)] // The host's environment, which has no such name.
    let host_has_it = std::env::var_os("CF_ONLY_IN_THE_BUILDER").is_some();
    assert!(!host_has_it);
    assert!(holds(
        &environment_block(&command),
        "CF_ONLY_IN_THE_BUILDER",
        &value
    ));
}

/// Where the builder's form cannot be read, the variables it lists go, one whose value is
/// not Unicode is left out, and the pane host's log says so once, never with a value.
#[test]
fn a_form_that_cannot_be_read_is_said_once_and_its_unicode_variables_go() {
    let mut command = portable_pty::CommandBuilder::new("program");
    command.env_clear();
    command.env("CF_SAID", "plain");
    command.env("CF_LONE", OsString::from_wide(&[0xD800, 0xD801]));
    let unread = || Err(Unread("a form of another version".into()));
    let first = block_of(&command, unread());
    let second = block_of(&command, unread());
    assert_eq!(first, second);
    assert!(holds(
        &first,
        "CF_SAID",
        &"plain".encode_utf16().collect::<Vec<_>>()
    ));
    assert!(!first.windows(2).any(|units| units == [0xD800, 0xD801]));
    let said: Vec<String> = SAID
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|line| line.contains("a form of another version"))
        .cloned()
        .collect();
    assert_eq!(said.len(), 1, "said once: {said:?}");
}

/// What is not portable-pty's builder has no `envs`, and is said so.
#[test]
fn a_form_with_no_envs_is_not_read() {
    #[derive(serde::Serialize)]
    struct Other {
        args: Vec<String>,
    }
    let unread = names_of(&Other { args: vec![] }).expect_err("no envs");
    assert!(unread.to_string().contains("envs"), "{unread}");
}
