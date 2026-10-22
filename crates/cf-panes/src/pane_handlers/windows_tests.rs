//! The pane handlers on Windows, driven over the bridge as the daemon drives them: a
//! console host that dies under a program that runs on (it let go of its console), which
//! ends the window's output for good. The host ends the program, and the daemon hears the
//! window closed (`pane.exit`) only once it has gone: the daemon forgets a window it is
//! told has closed, and may open another on its conversation at once (both reviews of
//! 2026-10-16). The test ends the console host and calls nothing of the table's: what
//! comes after is the product's own handoff, which it observes from the daemon's side.
//! Whether the program still ran is asked at the moment the host hands `pane.exit` to the
//! bridge (`EXIT_HANDED`), which sends it later: asked when the frame arrived, a host
//! that handed the exit over and then ended the program could pass (Sol, round 3).

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cf_bridge::BridgeBuilder;
use cf_proto::bridge::Role;
use serde_json::{json, Value};
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE, STILL_ACTIVE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_TERMINATE,
};

use super::{register_pane_handlers, EXIT_HANDED};
use crate::arbiter::{EnterTiming, InputArbiter};
use crate::input_queue::InputQueue;
use crate::output_hub::{OutputHub, PaneOutputMessage};
use crate::pty::conpty_test::{powershell, system as conpty_system};
use crate::pty::{serial_pty_test, PaneTable};

/// A program that says its number, lets go of its console (`FreeConsole`), says it has
/// in the file `CF_FREED` names, and runs on: the console host's death does not end it,
/// as it ends one that still uses the console.
const LETS_GO_OF_ITS_CONSOLE: &str = "Add-Type -Namespace CfTest -Name Console \
     -MemberDefinition '[DllImport(\"kernel32.dll\")] public static extern bool FreeConsole();'; \
     Write-Output \"PID=$PID#\"; [CfTest.Console]::FreeConsole() | Out-Null; \
     Set-Content -LiteralPath $env:CF_FREED -Value $PID; Start-Sleep 600";

/// The console hosts this test's process started: a pane's pseudoconsole is one
/// (`conhost.exe`, or the `OpenConsole.exe` a sideloaded ConPTY runs).
fn console_hosts() -> HashSet<u32> {
    let me = std::process::id();
    let mut hosts = HashSet::new();
    // SAFETY: a snapshot of the system's processes, read entry by entry into a structure
    // whose size it is told, and closed once read.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return hosts;
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..PROCESSENTRY32W::default()
        };
        let mut more = Process32FirstW(snapshot, &mut entry);
        while more != 0 {
            let length = entry
                .szExeFile
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
            if entry.th32ParentProcessID == me
                && (name.eq_ignore_ascii_case("conhost.exe")
                    || name.eq_ignore_ascii_case("OpenConsole.exe"))
            {
                hosts.insert(entry.th32ProcessID);
            }
            more = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
    }
    hosts
}

fn running(pid: u32) -> bool {
    // SAFETY: a query-only handle, closed before returning.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut code = 0u32;
        let read = GetExitCodeProcess(process, &mut code);
        CloseHandle(process);
        read != 0 && code == STILL_ACTIVE as u32
    }
}

/// Ends the process `pid`: the console host that dies, or the test's own program where
/// the host left it running.
fn end(pid: u32) {
    // SAFETY: a handle with the right to end the process, closed once used.
    unsafe {
        let process = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !process.is_null() {
            TerminateProcess(process, 1);
            CloseHandle(process);
        }
    }
}

/// The file the program says it let go of its console in, gone when the test is.
struct Freed(PathBuf);

impl Drop for Freed {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The daemon's end of the bridge: what it asks, and the frames it reads.
struct Daemon {
    stream: TcpStream,
    frames: BufReader<TcpStream>,
    /// A frame read in part, which a read that waited too long left.
    line: String,
    asked: u64,
}

impl Daemon {
    /// The next frame, by `until`; none where none came.
    fn next(&mut self, until: Instant) -> Option<Value> {
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            self.stream
                .set_read_timeout(Some(left))
                .expect("a read timeout");
            match self.frames.read_line(&mut self.line) {
                Ok(0) => return None,
                Ok(_) if self.line.ends_with('\n') => {
                    let frame = serde_json::from_str(self.line.trim()).expect("a frame");
                    self.line.clear();
                    return Some(frame);
                }
                Ok(_) => {}
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                // The host's end of the socket closed: Windows may say so as a reset.
                Err(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
                    ) =>
                {
                    return None
                }
                Err(error) => panic!("the bridge: {error}"),
            }
        }
    }

    /// Asks `op` of the host, as the daemon does, and its answer; the events that come
    /// meanwhile are kept in `events`.
    fn ask(&mut self, op: &str, body: Value, events: &mut Vec<Value>) -> Value {
        self.asked += 1;
        let id = format!("n-{}", self.asked);
        let mut frame =
            serde_json::to_vec(&json!({"v":1,"id":id,"kind":"req","op":op,"body":body}))
                .expect("a request");
        frame.push(b'\n');
        self.stream.write_all(&frame).expect("send a request");
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let frame = self
                .next(until)
                .unwrap_or_else(|| panic!("no answer to {op}"));
            if frame["kind"] == "res" && frame["id"] == id.as_str() {
                return frame["body"].clone();
            }
            events.push(frame);
        }
    }
}

/// The pane host as the app has it, its handlers on a bridge over loopback, and the
/// daemon at the bridge's other end.
struct Host {
    panes: Arc<PaneTable>,
    inputs: Arc<InputQueue>,
    bridge: cf_bridge::ConnectedBridge,
    daemon: Daemon,
    /// What the panes print: the id of the pane, and its bytes.
    printed: mpsc::Receiver<(String, Vec<u8>)>,
}

impl Host {
    /// A host whose bridge keeps its output past the end of its input where
    /// `keeps_output` (the headless pane host's).
    fn start(keeps_output: bool) -> Self {
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let output = Arc::new(OutputHub::new());
        let (printed, seen) = mpsc::channel::<(String, Vec<u8>)>();
        output.register_sink(Arc::new(move |message: PaneOutputMessage| {
            let id = serde_json::to_value(&message)
                .ok()
                .and_then(|message| message["id"].as_str().map(str::to_owned))
                .unwrap_or_default();
            printed.send((id, message.bytes)).is_ok()
        }));
        let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
        if keeps_output {
            builder.keep_output_after_eof();
        }
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            arbiter,
            output,
            Arc::clone(&inputs),
        );
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let mut daemon_end =
            TcpStream::connect(listener.local_addr().expect("its address")).expect("connect");
        let (host_end, _) = listener.accept().expect("accept");
        daemon_end
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("the bridge's handle");
        let bridge = builder
            .connect(host_end.try_clone().expect("the host's end"), host_end)
            .expect("connect the bridge");
        let daemon = Daemon {
            frames: BufReader::new(daemon_end.try_clone().expect("the daemon's end")),
            stream: daemon_end,
            line: String::new(),
            asked: 0,
        };
        Self {
            panes,
            inputs,
            bridge,
            daemon,
            printed: seen,
        }
    }
}

#[test]
fn a_program_whose_console_host_died_is_ended_and_gone_before_the_daemon_hears_its_window_closed() {
    let _pty_guard = serial_pty_test();
    let Host {
        inputs,
        bridge: connected,
        mut daemon,
        printed: seen,
        ..
    } = Host::start(false);
    let mut events = Vec::new();

    let freed =
        Freed(std::env::temp_dir().join(format!("cf-console-freed-{}", std::process::id())));
    let _ = std::fs::remove_file(&freed.0);
    let before = console_hosts();
    let opened = daemon.ask(
        "pane.open",
        json!({
            "id": "console-pane", "generation": 1,
            "cwd": std::env::temp_dir(),
            "argv": powershell(LETS_GO_OF_ITS_CONSOLE),
            "env": HashMap::from([("CF_FREED", freed.0.to_string_lossy())]),
            "size": {"rows": 24, "cols": 80},
        }),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let program = u32::try_from(opened["pid"].as_u64().expect("the program's number"))
        .expect("a process number");
    let hosts: Vec<u32> = console_hosts().difference(&before).copied().collect();
    assert_eq!(hosts.len(), 1, "the pane's console host: {hosts:?}");
    // Whether the program runs, asked as the exit of this pane is handed to the bridge.
    let ran_when_handed = Arc::new(Mutex::new(None));
    let asked = Arc::clone(&ran_when_handed);
    *EXIT_HANDED.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(move |key| {
        if key.id == "console-pane" {
            *asked.lock().unwrap_or_else(PoisonError::into_inner) = Some(running(program));
        }
    }));

    // What it prints, the cursor question answered as the page answers it, until it has
    // let go of its console.
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut said = Vec::new();
    while !freed.0.exists() {
        assert!(
            Instant::now() < deadline,
            "the program never let go of its console: {:?}",
            String::from_utf8_lossy(&said)
        );
        if let Ok((_, bytes)) = seen.recv_timeout(Duration::from_millis(100)) {
            if bytes.windows(4).any(|query| query == b"\x1b[6n") {
                let answered = daemon.ask(
                    "pane.reply",
                    json!({"id":"console-pane","generation":1,"bytes":b"\x1b[1;1R".to_vec()}),
                    &mut events,
                );
                assert_eq!(answered["ok"], true, "{answered}");
            }
            said.extend(bytes);
        }
    }

    // The console host dies; the program runs on.
    end(hosts[0]);
    let until = Instant::now() + Duration::from_secs(45);
    let mut exit = events
        .iter()
        .find(|event| event["op"] == "pane.exit")
        .cloned();
    while exit.is_none() {
        let Some(frame) = daemon.next(until) else {
            break;
        };
        if frame["kind"] == "evt" && frame["op"] == "pane.exit" {
            exit = Some(frame["body"].clone());
        }
    }
    *EXIT_HANDED.lock().unwrap_or_else(PoisonError::into_inner) = None;
    let gone = Instant::now() + Duration::from_secs(10);
    while running(program) && Instant::now() < gone {
        std::thread::sleep(Duration::from_millis(50));
    }
    let ran_on = running(program);
    if ran_on {
        end(program);
    }
    let exit = exit.expect("the daemon never heard the window closed");
    assert_eq!(
        *ran_when_handed
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
        Some(false),
        "the exit was handed to the bridge while the program still ran"
    );
    assert!(
        exit.get("survivor").is_none(),
        "the program was proven gone: {exit}"
    );
    assert!(!ran_on, "the program outlived its window");
    let listed = daemon.ask("pane.list", json!({}), &mut events);
    assert_eq!(listed["panes"], json!([]), "{listed}");

    inputs.close_and_drain();
    drop(daemon);
    connected.bridge.wait_closed().expect("the bridge closes");
}

/// Windows still open when the daemon ends the host's input are ended with the host, which
/// tells the exit of each, with the program it left running, before it closes its own
/// output: the daemon's end of the bridge reads to its end what the host said as it stopped
/// (`headless::shut_down`, the headless host's end).
#[test]
fn a_host_shut_down_with_windows_open_tells_the_exit_of_each_before_it_closes_its_output() {
    let _pty_guard = serial_pty_test();
    let Host {
        panes,
        inputs,
        bridge: connected,
        mut daemon,
        printed,
    } = Host::start(true);
    let mut events = Vec::new();
    let shell = conpty_system(r"WindowsPowerShell\v1.0\powershell.exe");

    // Two windows, each of which starts a ping in the turn the host is told of, and says so.
    let windows = [("shut-down-1", "1006"), ("shut-down-2", "1007")];
    for (id, seconds) in windows {
        let opened = daemon.ask(
            "pane.open",
            json!({
                "id": id, "generation": 1,
                "cwd": std::env::temp_dir(),
                "argv": powershell(&format!(
                    "Write-Output 'READY#'; [Console]::In.ReadLine() | Out-Null; \
                     Start-Process -WindowStyle Hidden -FilePath '{shell}' -ArgumentList '-NoProfile','-Command','ping -n {seconds} 127.0.0.1'; \
                     Write-Output 'STARTED#'; Start-Sleep 1000"
                )),
                "size": {"rows": 24, "cols": 80},
            }),
            &mut events,
        );
        assert_eq!(opened["ok"], true, "{opened}");
    }
    let mut said: HashMap<String, Vec<u8>> = HashMap::new();
    let mut hear = |daemon: &mut Daemon, events: &mut Vec<Value>, word: &str| {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let heard = |said: &HashMap<String, Vec<u8>>, id: &str| {
                said.get(id)
                    .is_some_and(|bytes| bytes.windows(word.len()).any(|at| at == word.as_bytes()))
            };
            if windows.iter().all(|(id, _)| heard(&said, id)) {
                return;
            }
            assert!(Instant::now() < deadline, "no {word}: {said:?}");
            if let Ok((id, bytes)) = printed.recv_timeout(Duration::from_millis(100)) {
                // The console asks where the cursor is before a program may print.
                if bytes.windows(4).any(|query| query == b"\x1b[6n") {
                    let answered = daemon.ask(
                        "pane.reply",
                        json!({"id":id,"generation":1,"bytes":b"\x1b[1;1R".to_vec()}),
                        events,
                    );
                    assert_eq!(answered["ok"], true, "{answered}");
                }
                said.entry(id).or_default().extend(bytes);
            }
        }
    };
    hear(&mut daemon, &mut events, "READY#");
    for (id, _) in windows {
        let pane = json!({"id":id,"generation":1});
        let marked = daemon.ask("pane.mark", pane.clone(), &mut events);
        assert_eq!(marked["ok"], true, "{marked}");
        let typed = daemon.ask(
            "pane.input",
            json!({"id":id,"generation":1,"bytes":[13]}),
            &mut events,
        );
        assert_eq!(typed["ok"], true, "{typed}");
    }
    hear(&mut daemon, &mut events, "STARTED#");
    for (id, seconds) in windows {
        let pane = json!({"id":id,"generation":1});
        let until = Instant::now() + Duration::from_secs(60);
        loop {
            let left = daemon.ask("pane.background", pane.clone(), &mut events);
            let named = left["processes"].as_array().is_some_and(|work| {
                work.iter().any(|process| {
                    process["command"].as_str().is_some_and(|command| {
                        command.to_lowercase().contains(&format!("-n {seconds}"))
                    })
                })
            });
            if named {
                break;
            }
            assert!(
                Instant::now() < until,
                "the ping of {id} was never listed: {left}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    // The daemon ends the host's input, and reads what the host says until its output ends.
    daemon
        .stream
        .shutdown(std::net::Shutdown::Write)
        .expect("end the host's input");
    let shutting = {
        let (bridge, panes, inputs) = (
            connected.bridge.clone(),
            Arc::clone(&panes),
            Arc::clone(&inputs),
        );
        std::thread::spawn(move || crate::headless::shut_down(&bridge, &panes, &inputs))
    };
    let until = Instant::now() + Duration::from_secs(60);
    while let Some(frame) = daemon.next(until) {
        events.push(frame);
    }
    assert!(
        Instant::now() < until,
        "the host's output never ended: {events:?}"
    );
    shutting
        .join()
        .expect("the shut down ends")
        .expect("the host shuts down");

    for (id, seconds) in windows {
        let exits: Vec<&Value> = events
            .iter()
            .filter(|event| event["op"] == "pane.exit" && event["body"]["id"] == id)
            .map(|event| &event["body"])
            .collect();
        assert_eq!(exits.len(), 1, "{id} has one exit: {exits:?}");
        let named = exits[0]["background"].as_array().is_some_and(|work| {
            work.iter().any(|process| {
                process["command"].as_str().is_some_and(|command| {
                    command.to_lowercase().contains(&format!("-n {seconds}"))
                })
            })
        });
        assert!(
            named,
            "the ping of {id} is named in its exit: {:?}",
            exits[0]
        );
        assert!(exits[0].get("survivor").is_none(), "{:?}", exits[0]);
    }
}
