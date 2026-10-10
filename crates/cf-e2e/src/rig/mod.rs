//! The rig: a real daemon and the real pane host, connected as the app connects
//! them, in a home of the case's own, with a stand-in `claude` in the windows
//! it opens. The daemon is `cf ui --json --no-open`; the pane host is
//! `consensflow-bridge`, which gives each window a terminal of its own (a PTY,
//! or ConPTY); they speak the bridge's frames to each other over their
//! standard streams, and the rig stands between them ([`route`]). It only
//! observes and routes the bytes: `pane.open`, the terminals, the arbitration
//! of input and the cleanup are the product's.
//!
//! The rig is also the page. A case asks the daemon what the page asks
//! ([`Rig::page`]: `project.open`, `board.get`, `task.get`…), and the pane host
//! what the desktop app asks ([`Rig::host`]: `pane.input`), and types to a
//! chief as a human does ([`Rig::tell`]). What a window prints, what the
//! stand-in agent wrote, and the frames themselves are there to look at.

mod install;
mod launch;
mod project;
mod report;
mod route;
mod wait;

use std::path::{Path, PathBuf};
use std::sync::{Arc, MutexGuard};
use std::time::Duration;

use serde_json::{json, Value};

use crate::daemon::Daemon;
use crate::daemon_log::StartLine;
use crate::process::{self, Ran, Run, Sink, Spawned};
use crate::{cf, checkout, files, wire, Error, Result};

pub use project::{write_staff, Project};

use route::Shared;

/// How long the daemon or the pane host has to answer a request of the case's.
const REQUEST: Duration = Duration::from_secs(10);

/// How long the daemon and the pane host each have to end once their input has.
const EXIT: Duration = Duration::from_secs(5);

/// How long a process a window started has to be gone once the pane host is.
const GONE: Duration = Duration::from_secs(3);

/// How long a window's first frame is waited for by default.
pub const OPEN: Duration = Duration::from_secs(10);

/// How long a chief is waited for to be idle, by default, before it is typed to.
const IDLE: Duration = Duration::from_secs(60);

/// What a rig is started with.
#[derive(Debug, Clone)]
pub struct Config {
    stand_in: PathBuf,
    vars: Vec<(String, String)>,
    path: Vec<PathBuf>,
    existing_root: Option<PathBuf>,
    daemon: Option<PathBuf>,
}

impl Config {
    /// A rig whose windows run `stand_in`, the stand-in for a harness's terminal
    /// program (the `fake-agent` binary of this crate).
    pub fn new(stand_in: impl Into<PathBuf>) -> Self {
        Self {
            stand_in: stand_in.into(),
            vars: Vec::new(),
            path: Vec::new(),
            existing_root: None,
            daemon: None,
        }
    }

    /// With the variable `name` set to `value` in the environment of the
    /// daemon, the pane host and so every window, over what the rig sets.
    #[must_use]
    pub fn var(mut self, name: &str, value: impl Into<String>) -> Self {
        self.vars.push((name.to_owned(), value.into()));
        self
    }

    /// With the folder `dir` on the `PATH` of the daemon and so of every window,
    /// after the folder of the stand-in `claude` and before the system's: for a
    /// case that runs a real program of the machine (its own `codex`) beside the
    /// stand-in. The stand-in is still the first `claude` found.
    #[must_use]
    pub fn also_on_path(mut self, dir: impl Into<PathBuf>) -> Self {
        self.path.push(dir.into());
        self
    }

    /// On the home a rig before it ran on, as the app's restart is: the roster
    /// that rig had is kept, and its projects are the daemon's to resume. The
    /// rig removes that home when it is closed.
    #[must_use]
    pub fn existing_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.existing_root = Some(root.into());
        self
    }

    /// With `program` started where the daemon is: for the case that holds the
    /// rig to refusing a daemon that is not the native one.
    #[must_use]
    pub fn daemon(mut self, program: impl Into<PathBuf>) -> Self {
        self.daemon = Some(program.into());
        self
    }
}

/// A process a window's stand-in agent started, as it wrote it down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub pid: u32,
    pub session: String,
}

/// A rig that is running.
pub struct Rig {
    root: PathBuf,
    workspace: PathBuf,
    env: Vec<(String, String)>,
    start_line: StartLine,
    shared: Arc<Shared>,
    daemon: Daemon,
    host: Spawned,
    /// Whether the home is left when the rig ends: it is not, unless a rig is
    /// to resume on it or a case asked to keep it ([`Rig::keep_home`]).
    keep_root: bool,
    closed: bool,
    _turn: MutexGuard<'static, ()>,
}

impl Rig {
    /// The folder a project's work is in.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Which daemon this is, as its own start line says.
    pub fn start_line(&self) -> &StartLine {
        &self.start_line
    }

    /// The daemon's process id.
    pub fn daemon_pid(&self) -> u32 {
        self.daemon.id()
    }

    /// The value of the variable `name` in the environment of the daemon and
    /// the windows it opens.
    pub fn var(&self, name: &str) -> Option<&str> {
        self.env
            .iter()
            .rev()
            .find(|(given, _)| given == name)
            .map(|(_, value)| value.as_str())
    }

    /// The daemon's home, `CONSENSFLOW_HOME`.
    pub fn home(&self) -> PathBuf {
        PathBuf::from(self.var("CONSENSFLOW_HOME").unwrap_or_default())
    }

    /// The daemon's log.
    pub fn log(&self) -> String {
        files::read_string(&self.home().join("daemon.log")).unwrap_or_default()
    }

    /// What the page asks the daemon: `op` with `body`, answered by the body of
    /// the response. The page's requests are `r-test-N` on the daemon's input.
    pub fn page(&self, op: &str, body: Value) -> Result<Value> {
        let id = self.shared.next_page_id();
        self.ask(&id, op, &body, self.daemon.sink(), "the daemon")
    }

    /// What the desktop app asks the pane host: `op` with `body`, answered by
    /// the body of the response. The app's requests are `n-test-N`.
    pub fn host(&self, op: &str, body: Value) -> Result<Value> {
        let id = self.shared.next_host_id();
        self.ask(&id, op, &body, self.host.sink(), "the pane host")
    }

    fn ask(&self, id: &str, op: &str, body: &Value, to: Option<Sink>, whom: &str) -> Result<Value> {
        let answer = self.shared.pending.expect(id);
        if let Some(to) = to {
            to.send(wire::request(id, op, body));
        }
        answer.recv_timeout(REQUEST).map_err(|_| {
            self.shared.pending.forget(id);
            Error::Timeout(format!(
                "timed out waiting for {whom} to answer {op}{}",
                self.what_they_said()
            ))
        })
    }

    /// The frames each wrote and what each said on its error output, for the
    /// message of a wait that ran out.
    fn what_they_said(&self) -> String {
        let seen = self.shared.seen();
        format!(
            "; daemon frames={} host frames={}; daemon stderr={} host stderr={}",
            seen.daemon.len(),
            seen.host.len(),
            self.daemon.errors().trim(),
            self.host.errors().trim()
        )
    }

    /// Every `pane.open` request the daemon made so far, in order: what each
    /// window was opened with (`id`, `generation`, `argv`, `env`).
    pub fn open_frames(&self) -> Vec<Value> {
        self.shared.seen().opened.clone()
    }

    /// The `pane.open` of the window `id`, waited for up to `within`: an
    /// operation answers before its window opens.
    pub fn open_frame(&self, id: &str, within: Duration) -> Result<Value> {
        let find = || {
            self.shared
                .seen()
                .opened
                .iter()
                .find(|frame| frame["id"] == id)
                .cloned()
        };
        self.wait_for(&format!("the window {id} to be opened"), within, || {
            Ok(find().is_some())
        })?;
        find().ok_or_else(|| Error::Daemon(format!("the window {id} was opened, and is gone")))
    }

    /// Every `pane.exit` the pane host told so far, in order: which window
    /// ended (`id`, `generation`), how (`exitCode`, `signal`) and the last lines
    /// its screen showed (`tail`).
    pub fn exits(&self) -> Vec<Value> {
        self.shared.seen().exits.clone()
    }

    /// The human types into the chief's terminal, the one way work reaches the
    /// chief: a bracketed paste of `text`, then Enter, once the chief is idle (it
    /// is waited for a minute).
    pub fn tell(&self, project: i64, text: &str) -> Result<()> {
        // The first lane of the chief's, as the page draws it.
        let chief = || -> Result<Value> {
            let board = Project::new(self, project).board()?;
            Ok(board["lanes"]
                .as_array()
                .and_then(|lanes| {
                    lanes
                        .iter()
                        .find(|lane| lane["participant"]["handle"] == "chief")
                })
                .cloned()
                .unwrap_or(Value::Null))
        };
        self.wait_for("the chief to be idle", IDLE, || {
            Ok(chief()?["activity"]["state"] == "idle")
        })?;
        let pane = chief()?["pane"].clone();
        let typed = format!("\u{1b}[200~{text}\u{1b}[201~\r");
        let answer = self.host(
            "pane.input",
            json!({
                "id": pane["id"],
                "generation": pane["generation"],
                "bytes": typed.as_bytes(),
            }),
        )?;
        if answer["ok"] == true {
            Ok(())
        } else {
            Err(Error::Daemon(format!("pane.input was refused: {answer}")))
        }
    }

    /// `cf` run as an agent runs it in a window: the rig's environment with the
    /// window's own URL and token (`frame` is its `pane.open`), `words` as its
    /// words.
    pub fn cf_in_window<I, S>(&self, frame: &Value, words: I) -> Result<Ran>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let env = &frame["env"];
        Run::new(cf::binary()?)
            .vars(self.env.iter().map(|(name, value)| (name, value)))
            .var(
                "CONSENSFLOW_URL",
                env["CONSENSFLOW_URL"].as_str().unwrap_or_default(),
            )
            .var(
                "CONSENSFLOW_TOKEN",
                env["CONSENSFLOW_TOKEN"].as_str().unwrap_or_default(),
            )
            .finding_programs()
            .cwd(checkout::root())
            .args(words)
            .run()
    }

    /// What the conversation `session` of the stand-in agent recorded, one
    /// JSON record to a line; nothing if it has recorded none. Only whole
    /// records: the agent writes while the case reads, and the line it is in the
    /// middle of is left for the next read ([`files::read_finished_lines`]).
    pub fn transcript(&self, session: &str) -> String {
        recorded(self.var("CLAUDE_CONFIG_DIR").unwrap_or_default(), session)
    }

    /// The processes the stand-in agent recorded starting, in order. The
    /// chief's comes first.
    pub fn started(&self) -> Vec<Started> {
        files::read_string(&self.root.join("processes.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let (pid, session) = line.split_once('\t')?;
                Some(Started {
                    pid: pid.parse().ok()?,
                    session: session.to_owned(),
                })
            })
            .collect()
    }

    /// Ends the daemon at once, as a crash does, or as the app's quit does
    /// first: the daemon dies, then the pane host.
    pub fn kill_daemon(&mut self) {
        self.daemon.kill();
    }

    /// Whether the daemon has ended.
    pub fn daemon_exited(&mut self) -> bool {
        self.daemon.has_exited()
    }

    /// Waits up to `within` for the daemon to be gone, as it is after
    /// [`Rig::kill_daemon`].
    pub fn wait_for_daemon_exit(&mut self, within: Duration) -> Result<()> {
        if self.daemon.wait(within)?.is_some() {
            Ok(())
        } else {
            Err(Error::Timeout(format!(
                "timed out after {} s waiting for the daemon to be gone{}",
                within.as_secs(),
                self.what_they_said()
            )))
        }
    }

    /// Leaves the home where it is when the rig ends, however it ends (closed,
    /// or dropped by a case that failed), for a person to look at what the case
    /// did: where it is.
    pub fn keep_home(&mut self) -> PathBuf {
        self.keep_root = true;
        self.root.clone()
    }

    /// Ends the rig: both programs' input is ended, both are waited for, and
    /// every process a window started is checked to be gone. The home goes with
    /// it.
    pub fn close(mut self) -> Result<()> {
        self.shutdown()
    }

    /// [`Rig::close`], but the home is left, for a rig that resumes on it
    /// ([`Config::existing_root`]). Where it is.
    pub fn close_keeping_the_root(mut self) -> Result<PathBuf> {
        self.keep_root = true;
        self.shutdown()?;
        Ok(self.root.clone())
    }

    fn shutdown(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.daemon.end_input();
        self.host.end_input();
        let (daemon, host) = (self.daemon.wait(EXIT)?, self.host.wait(EXIT)?);
        if daemon.is_none() || host.is_none() {
            return Err(Error::Timeout(format!(
                "process did not exit{}",
                self.what_they_said()
            )));
        }
        // The pane host kills each window's process as it shuts down, and a
        // killed process leaves the process table a moment later (once it is
        // reaped): one still there after a few seconds was left behind.
        let pids = files::read_string(&self.root.join("pids.jsonl")).unwrap_or_default();
        for pid in pids
            .lines()
            .filter_map(|line| line.trim().parse::<u32>().ok())
        {
            if !wait::until(GONE, || !process::is_alive(pid)) {
                return Err(Error::Daemon(format!(
                    "fake harness process remains after bridge shutdown: {pid}"
                )));
            }
        }
        Ok(())
    }
}

impl Drop for Rig {
    /// A rig not closed (a case that failed on the way, by a check or by an
    /// error it returned: every case that passes ends by closing it) is ended
    /// all the same, saying what the windows showed first; and the home goes
    /// unless it is to be kept.
    fn drop(&mut self) {
        if std::thread::panicking() || !self.closed {
            report::say(self);
        }
        let _ = self.shutdown();
        self.daemon.kill();
        self.host.kill();
        if !self.keep_root {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// The records of the conversation `session` that the stand-in agent wrote
/// under the Claude config folder `config`, whole ones only.
fn recorded(config: &str, session: &str) -> String {
    let file = Path::new(config)
        .join("projects")
        .join("integration")
        .join(format!("{session}.jsonl"));
    files::read_finished_lines(&file).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conversation_is_read_a_whole_record_at_a_time_while_the_agent_is_still_writing_it() {
        let config = tempfile::tempdir().unwrap();
        let file = config
            .path()
            .join("projects")
            .join("integration")
            .join("s-1.jsonl");
        let whole = "{\"type\":\"user\"}\n{\"type\":\"assistant\"}\n";
        // The agent is in the middle of the third: a case that reads now must not
        // meet it, as a case once met `EOF while parsing a string`.
        let torn = "{\"type\":\"assistant\",\"message\":{\"content\":[{\"text\":\"ran cf: Wai";
        files::write(&file, format!("{whole}{torn}")).unwrap();
        let config = config.path().to_string_lossy().into_owned();
        assert_eq!(recorded(&config, "s-1"), whole);
        // A conversation nothing has been written of is nothing.
        assert_eq!(recorded(&config, "s-2"), "");
    }
}
