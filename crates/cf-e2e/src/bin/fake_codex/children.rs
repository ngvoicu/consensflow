//! A server that keeps starting programs once a thread is open, as Codex's does while a
//! turn runs its shell commands. Each program's process is written to the log
//! (`{"run":"child","child":<pid>}`) as it starts, so that a case can ask the system, by
//! pid, whether it is still there once the window has ended the server; and, once the
//! last has started, `{"run":"children","all":<count>}`: a case that finds no such line
//! knows the server was starting programs when it was ended.
//!
//! The server can instead lose a program's parent ([`lose_a_parent`]): a launcher it starts
//! starts a program and ends at once, as a shell command that leaves a background program
//! does, and the program has no parent left to be found by.
//!
//! Bounded, because a loop that is not has flooded a machine (zeewin, T-322: 15,800
//! pings): [`STARTS`] programs at most, each running [`RUNS_FOR`] seconds unless it is
//! ended. They are not waited for, nor ended when the server is: the case asks what the
//! window's end left.

use std::process::ExitCode;
use std::sync::Once;
use std::thread;
use std::time::Duration;

use cf_e2e::process::Run;
use serde_json::json;

use super::{fail, record};

/// What the launcher is started with, first.
pub const LAUNCHER: &str = "--launcher";

/// How many programs the server starts, at most.
const STARTS: usize = 80;

/// How long between one start and the next: short, so that the window ends the server
/// while it is starting one.
const PACE: Duration = Duration::from_millis(10);

/// How long each program runs unless it is ended: past what a case waits for the end of
/// them, and few enough seconds that a case that fails leaves nothing for long.
const RUNS_FOR: u32 = 20;

/// Starts the programs on a thread of its own, once however many threads are started.
pub fn keep_starting() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        thread::spawn(start_them);
    });
}

fn start_them() {
    let mut started = Vec::new();
    for count in 0..STARTS {
        // Each is marked by an address of its own (127.0.54.x), so that a ping a case
        // left is told from every other.
        #[cfg(windows)]
        let run = Run::new(r"C:\Windows\System32\PING.EXE")
            .args(["-n", &(RUNS_FOR + 1).to_string()])
            .arg(format!("127.0.54.{}", count % 250 + 1));
        #[cfg(not(windows))]
        let run = {
            let _ = count;
            Run::new("/bin/sleep").arg(RUNS_FOR.to_string())
        };
        let Ok(child) = run.closed_input().spawn() else {
            return;
        };
        record(json!({ "run": "child", "child": child.id() }));
        // Kept: a `Spawned` that is dropped ends its program, which only the window's end is to do.
        started.push(child);
        thread::sleep(PACE);
    }
    record(json!({ "run": "children", "all": started.len() }));
    loop {
        thread::park();
    }
}

/// Starts a launcher beside the server, on a thread of its own, once: when it has ended, the
/// program it started has no parent. Its process is in the log as the others are.
pub fn lose_a_parent() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        thread::spawn(|| {
            let Ok(program) = std::env::current_exe() else {
                return;
            };
            let _ = Run::new(program)
                .arg(LAUNCHER)
                .inheriting_env()
                .closed_input()
                .run();
        });
    });
}

/// The launcher: starts one program, writes its process to the log, and ends.
#[allow(clippy::disallowed_methods)] // The one program is left to run on its own, streams of its own.
pub fn launch() -> ExitCode {
    use std::process::{Command, Stdio};
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new(r"C:\Windows\System32\PING.EXE");
        command
            .args(["-n", &(RUNS_FOR + 1).to_string()])
            .arg("127.0.54.254");
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("/bin/sleep");
        command.arg(RUNS_FOR.to_string());
        command
    };
    match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            record(json!({ "run": "child", "child": child.id() }));
            ExitCode::SUCCESS
        }
        Err(failed) => fail(&failed.to_string()),
    }
}
