//! What the macOS tests of a turn's work share: a pane running a Python program
//! that builds the tree of a harness's window (a window's program that starts
//! servers, and an agent that starts commands in shells), told its turn begins, and
//! what is asked of the tree after.
#![cfg(target_os = "macos")]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cf_proto::panes::Servers;
use portable_pty::PtySize;
use serde_json::Value;

use crate::pty::{process_exists, PaneEnvironment, PaneKey, PaneOutput, PaneTable, StreamedPane};

/// What every program of these tests begins with: it reaps what it started as it
/// ends, so that a process that was ended is no zombie, which a number would still
/// say is there. A disposition of SIGCHLD that ignored it would be kept by the
/// shells it starts, and their `wait` would not wait.
const REAP: &str = r#"
import os, signal

def reap(*_):
    try:
        while os.waitpid(-1, os.WNOHANG)[0]:
            pass
    except ChildProcessError:
        pass

signal.signal(signal.SIGCHLD, reap)
"#;

/// A window whose agent's tool ran a shell with a job it detached (`nohup sleep 31
/// &`), which ended with the command: the job is left to the system's init, in the
/// shell's own group. The program says it started, with the job's number, once the
/// shell is gone.
pub(super) const DETACHED: &str = r#"
import json, subprocess, time

before = subprocess.Popen(["sleep", "33"])
print("ready", flush=True)
input()
shell = subprocess.Popen(["/bin/sh", "-c", "nohup sleep 31 >/dev/null 2>&1 & echo $!"],
                         stdout=subprocess.PIPE, text=True, preexec_fn=os.setpgrp)
job = int(shell.stdout.readline())
shell.stdout.read()
time.sleep(0.3)
print("started " + json.dumps({"program": os.getpid(), "before": before.pid, "job": job}), flush=True)
time.sleep(60)
"#;

/// A file or a folder of the test's own, not there yet.
pub(super) fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "cf-turn-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_file(&path);
    let _ = fs::remove_dir_all(&path);
    path
}

/// A folder, with a space in its name, holding the programs of a harness named
/// `codex`: its binary and, beside it, a helper named for it (`codex-code-mode-host`),
/// as Codex keeps. Both are one small program of C, which runs the command it is
/// given with `system` and waits for it, or sleeps where it is given none: a program
/// the system keeps as it is, which a copy of a system program is not.
#[allow(clippy::disallowed_methods)] // A test's own programs.
pub(super) fn harness_programs() -> PathBuf {
    let dir = scratch("codex bin");
    fs::create_dir_all(&dir).expect("a folder for the harness's programs");
    let source = dir.join("program.c");
    fs::write(
        &source,
        "#include <stdlib.h>\n#include <unistd.h>\n\
         int main(int argc, char **argv) {\n\
         \tif (argc > 1) return system(argv[1]);\n\
         \tsleep(60);\n\
         \treturn 0;\n}\n",
    )
    .expect("the source of the harness's programs");
    let helper = dir.join("codex-code-mode-host");
    let made = std::process::Command::new("cc")
        .arg(&source)
        .arg("-o")
        .arg(&helper)
        .status()
        .expect("compile the harness's programs");
    assert!(made.success(), "the harness's programs were not made");
    fs::copy(&helper, dir.join("codex")).expect("the harness's binary");
    dir
}

/// A pane running the Python program `code`, streamed, with `env` in its
/// environment.
pub(super) fn open_python(
    table: &Arc<PaneTable>,
    code: &str,
    env: &HashMap<String, String>,
) -> StreamedPane {
    table
        .open_streamed(
            Path::new("/tmp"),
            &["/usr/bin/python3".into(), "-c".into(), code.into()],
            PaneEnvironment::new(env, &[]),
            PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
            1024 * 1024,
        )
        .expect("open a Python program in a pane")
}

/// Reads the pane's output until a line that begins `marker` is whole, and
/// says the rest of it.
pub(super) fn until_line(output: &Receiver<PaneOutput>, marker: &str) -> String {
    let mut seen = String::new();
    loop {
        if let Some(at) = seen.find(marker) {
            if let Some(end) = seen[at..].find(['\r', '\n']) {
                return seen[at + marker.len()..at + end].to_owned();
            }
        }
        let chunk = output
            .recv_timeout(Duration::from_secs(20))
            .unwrap_or_else(|_| panic!("the pane showed no {marker}: {seen:?}"));
        seen.push_str(&String::from_utf8_lossy(&chunk.bytes));
    }
}

/// The window of `code`, told its turn begins (the mark, then "go") with `servers`
/// and `harness` as the daemon says them, and run to where it says what it started:
/// the pane and the numbers it names.
pub(super) fn started(
    table: &Arc<PaneTable>,
    code: &str,
    servers: Servers,
    harness: Option<&str>,
    env: &HashMap<String, String>,
) -> (StreamedPane, HashMap<String, i32>) {
    let pane = open_python(table, &format!("{REAP}{code}"), env);
    until_line(&pane.output, "ready");
    table
        .mark(&pane.key, servers, harness)
        .expect("mark the turn");
    table.write(&pane.key, b"go\n").expect("tell it to go");
    let said = until_line(&pane.output, "started ");
    let numbers: Value = serde_json::from_str(&said).expect("the numbers it started");
    let pids = numbers
        .as_object()
        .expect("an object")
        .iter()
        .map(|(name, pid)| (name.clone(), pid.as_i64().expect("a number") as i32))
        .collect();
    (pane, pids)
}

/// What the pane's tree lists of the turn's, by number.
pub(super) fn listed(table: &PaneTable, key: &PaneKey) -> Vec<u32> {
    let mut left: Vec<u32> = table
        .background(key)
        .expect("what it left running")
        .processes
        .iter()
        .map(|process| process.pid)
        .collect();
    left.sort_unstable();
    left
}

/// The numbers `pids` names for `names`, in order.
pub(super) fn numbers(pids: &HashMap<String, i32>, names: &[&str]) -> Vec<u32> {
    let mut numbers: Vec<u32> = names.iter().map(|name| pids[*name] as u32).collect();
    numbers.sort_unstable();
    numbers
}

/// Waits until the process is gone, up to two seconds.
pub(super) fn gone(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while process_exists(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    !process_exists(pid)
}
