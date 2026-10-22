//! The servers a Codex opens with its window (its plugins': computer use, a REPL),
//! each in a group of its own under the process that is Codex's, which stays in the
//! window's. They are there when the window's first turn begins, and one of them
//! starts processes of its own when a tool is called, as a REPL starts the workers
//! of a call: found on 3.0.1, where the task of a Codex designer was held for them.
//!
//! A server is this program started again with [`SERVER`] first: `fake-codex
//! --server <file> <workers>`. It writes `<file>.pid`, and starts that many workers
//! (`<folder>/worker-<its process>-<n>`) when a line arrives on its standard
//! input, which is Codex calling a tool. It ends with its input, and so with the
//! window. A worker is this program started with [`WORKER`]: `fake-codex --worker
//! <seconds> <file>`, which writes `<file>.pid` and runs that many seconds.
//! `cf_e2e::rig::Rig::jobs` reads what they wrote: the servers are `plugin`, the
//! workers `worker`. The process is in `<file>.pid` whole from the moment the file is
//! there ([`say_its_process`]), for a case that waits for the file reads a number.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use cf_e2e::process::{own_var, Run, Spawned};

/// What a server is started with, first.
pub const SERVER: &str = "--server";

/// What a worker is started with, first.
pub const WORKER: &str = "--worker";

/// How long a worker runs: past any case.
const WORKER_SECONDS: u64 = 600;

/// The servers of one window, which its Codex speaks to by their input.
pub struct Plugins {
    servers: Vec<Spawned>,
}

impl Plugins {
    /// `count` servers, started where Codex starts them: with the window. The
    /// first starts `workers` of its own when a tool is called, and the others
    /// none. Their files are in `jobs/` beside the daemon's home, where the rig
    /// looks. Nothing is started where the rig's home is not told.
    pub fn open(count: usize, workers: usize) -> Result<Self, String> {
        let Some(home) = own_var("CONSENSFLOW_HOME") else {
            return Ok(Self {
                servers: Vec::new(),
            });
        };
        let folder = jobs_folder(Path::new(&home));
        fs::create_dir_all(&folder).map_err(|failed| format!("{}: {failed}", folder.display()))?;
        let program = std::env::current_exe().map_err(|failed| failed.to_string())?;
        let window = std::process::id();
        let servers = (1..=count)
            .map(|number| {
                let starts = if number == 1 { workers } else { 0 };
                Run::new(&program)
                    .arg(SERVER)
                    .arg(folder.join(format!("plugin-{window}-{number}")))
                    .arg(starts.to_string())
                    .inheriting_env()
                    .spawn()
                    .map_err(|failed| failed.to_string())
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { servers })
    }

    /// Whether Codex opened no server.
    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    /// Codex calls a tool: the first server starts what it was told to.
    pub fn call_a_tool(&self) {
        if let Some(first) = self.servers.first() {
            first.send("call\n");
        }
    }
}

/// Where the jobs' files are: `jobs/` beside the daemon's home.
fn jobs_folder(home: &Path) -> PathBuf {
    home.parent().unwrap_or(Path::new(".")).join("jobs")
}

/// A server: in a session of its own, ready when its process is written, and a
/// starter of workers on a call, until its input ends.
pub fn serve(args: &[String]) -> ExitCode {
    let (Some(file), Some(workers)) = (
        args.first(),
        args.get(1).and_then(|n| n.parse::<usize>().ok()),
    ) else {
        return refuse(&format!("{SERVER} <file> <workers>"));
    };
    become_a_group_of_its_own();
    if let Err(failed) = say_its_process(file) {
        return refuse(&failed.to_string());
    }
    let folder = Path::new(file).parent().unwrap_or(Path::new("."));
    let mut started: Vec<Spawned> = Vec::new();
    for _ in io::stdin().lock().lines().map_while(Result::ok) {
        // Called again, it starts no more.
        if !started.is_empty() {
            continue;
        }
        let Ok(program) = std::env::current_exe() else {
            continue;
        };
        for number in 1..=workers {
            let worker = folder.join(format!("worker-{}-{number}", std::process::id()));
            let spawned = Run::new(&program)
                .arg(WORKER)
                .arg(WORKER_SECONDS.to_string())
                .arg(worker)
                .inheriting_env()
                .closed_input()
                .spawn();
            match spawned {
                Ok(worker) => started.push(worker),
                Err(failed) => return refuse(&failed.to_string()),
            }
        }
    }
    // Its input ended with the window: what it started goes with it.
    ExitCode::SUCCESS
}

/// A worker: runs its seconds, its process written for whoever asks.
pub fn work(args: &[String]) -> ExitCode {
    let (Some(seconds), Some(file)) = (
        args.first().and_then(|n| n.parse::<u64>().ok()),
        args.get(1),
    ) else {
        return refuse(&format!("{WORKER} <seconds> <file>"));
    };
    if let Err(failed) = say_its_process(file) {
        return refuse(&failed.to_string());
    }
    thread::sleep(Duration::from_secs(seconds));
    ExitCode::SUCCESS
}

/// Writes the process in `<file>.pid`, so that the file is never there empty: it is
/// written under another name and renamed, for `fs::write` makes the file before it
/// writes into it, and a case that waits for the file reads what is there at once.
fn say_its_process(file: &str) -> io::Result<()> {
    let writing = format!("{file}.pid.writing");
    fs::write(&writing, std::process::id().to_string())?;
    fs::rename(writing, format!("{file}.pid"))
}

/// In a session of its own, as Codex's plugins are in groups of their own: a
/// program that is no leader of a group can lead one. Windows has no groups.
fn become_a_group_of_its_own() {
    #[cfg(unix)]
    let _ = nix::unistd::setsid();
}

/// Says why it cannot go on.
fn refuse(why: &str) -> ExitCode {
    let _ = writeln!(io::stderr(), "fake-codex: {why}");
    ExitCode::FAILURE
}
