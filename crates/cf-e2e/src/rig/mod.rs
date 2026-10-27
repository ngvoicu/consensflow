//! The rig: a real daemon and the real pane host, connected as the app connects
//! them, in a home of the case's own, with a stand-in `claude` in the windows
//! it opens (and a stand-in `devin`, for a case that asks for one:
//! [`Config::devin`]; an `opencode` 1.x: [`Config::opencode`]; an `opencode`
//! that has updated itself to 2: [`Config::opencode_2`]; or each where it
//! installs itself, off a `PATH` that is the Finder's:
//! [`Config::as_the_finder_starts_it`]). The daemon is `cf ui --json --no-open`;
//! the pane
//! host is `consensflow-bridge`, which gives each window a terminal of its own
//! (a PTY, or ConPTY); they speak the bridge's frames to each other over their
//! standard streams, and the rig stands between them ([`route`]). It only
//! observes and routes the bytes: `pane.open`, the terminals, the arbitration
//! of input and the cleanup are the product's.
//!
//! The rig is also the page. A case asks the daemon what the page asks
//! ([`Rig::page`]: `project.open`, `board.get`, `task.get`…), and the pane host
//! what the desktop app asks ([`Rig::host`]: `pane.input`), and types to a
//! chief as a human does ([`Rig::tell`]). What a window prints, what the
//! stand-in agent wrote, the jobs it left running ([`Rig::jobs`]) and the
//! frames themselves are there to look at: a window's screen as text
//! ([`Rig::screen`]), what the daemon wrote as it went ([`Rig::events`]) and the
//! keys it pressed, and when ([`Rig::presses`]).
//!
//! A process that is none of any window's runs beside them (`stranger`), and a rig
//! that finds it ended by something else when it ends fails: the pane host ends a
//! window's own and nothing of the machine's.

mod install;
mod jobs;
mod launch;
mod left;
mod names;
mod observe;
mod project;
mod report;
mod route;
mod stranger;
mod sweep;
mod wait;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, MutexGuard};
use std::time::Duration;

use serde_json::Value;

use crate::daemon::Daemon;
use crate::daemon_log::StartLine;
use crate::live::members::Identity;
use crate::outcome::{self, Verdict};
use crate::process::{Ran, Run, Sink, Spawned};
use crate::{checkout, files, wire, Error, Result};

pub use jobs::Job;
pub use names::project_name;
pub use project::{write_staff, Project};
pub use route::{Held, Pasted};

pub(crate) use observe::events_of;

use route::Shared;
use stranger::Stranger;

/// How long the daemon or the pane host has to answer a request of the case's.
const REQUEST: Duration = Duration::from_secs(10);

/// How long the daemon and the pane host each have to end once their input has.
const EXIT: Duration = Duration::from_secs(5);

/// How long a process a window started has to be gone once the pane host is.
const GONE: Duration = Duration::from_secs(3);

/// How long the router is given to read what the pane host wrote last, once the
/// host has gone.
const LAST_FRAMES: Duration = Duration::from_secs(10);

/// How long a window's first frame is waited for by default.
pub const OPEN: Duration = Duration::from_secs(10);

/// How long a chief is waited for to be idle, by default, before it is typed to.
const IDLE: Duration = Duration::from_secs(60);

/// What a rig is started with.
#[derive(Debug, Clone)]
pub struct Config {
    stand_in: PathBuf,
    vars: Vec<(String, String)>,
    /// What only the daemon is given, as the app gives its daemon what it took of a
    /// terminal's environment: the pane host and its windows have none of it.
    daemon_vars: Vec<(String, String)>,
    unset: Vec<String>,
    first: Vec<PathBuf>,
    path: Vec<PathBuf>,
    existing_root: Option<PathBuf>,
    daemon: Option<PathBuf>,
    claude_config: Option<Arc<dyn ClaudeConfig>>,
    /// The `cf` the daemon's copy is made of, where it is not the release build.
    cf: Option<PathBuf>,
    codex: Option<PathBuf>,
    devin: Option<PathBuf>,
    /// The stand-in `opencode`: of an OpenCode 1.x, or of one that is 2.
    opencode: Option<PathBuf>,
    finder: bool,
    masker: Option<Masker>,
    reports: Option<PathBuf>,
    homes: Option<PathBuf>,
    case: Option<String>,
    judged: bool,
}

/// What a rig passes what it says of itself through, where its owner gives it one
/// ([`Config::masked_by`]): a run that hides what looks like a key or a token in all
/// it says. Each text is passed whole, before any of it is cut; a window's screen as
/// its rows, which the owner masks as one text. A rig with none says it as it is.
#[derive(Clone)]
pub struct Masker {
    text: Arc<dyn Fn(&str) -> String + Send + Sync>,
    screen: Arc<ScreenMask>,
}

/// What a masker passes the rows of a window's screen through.
type ScreenMask = dyn Fn(&[String]) -> Vec<String> + Send + Sync;

impl Masker {
    /// A masker that passes each text through `text`, and the rows of each window's
    /// screen through `screen`.
    pub fn new(
        text: impl Fn(&str) -> String + Send + Sync + 'static,
        screen: impl Fn(&[String]) -> Vec<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            text: Arc::new(text),
            screen: Arc::new(screen),
        }
    }

    /// `text` as it is said.
    pub fn shown(&self, text: &str) -> String {
        (self.text)(text)
    }

    /// The rows of a window's screen as they are said.
    pub fn screen(&self, rows: &[String]) -> Vec<String> {
        (self.screen)(rows)
    }
}

impl std::fmt::Debug for Masker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Masker")
    }
}

impl Config {
    /// A rig whose windows run `stand_in`, the stand-in for a harness's terminal
    /// program (the `fake-agent` binary of this crate).
    pub fn new(stand_in: impl Into<PathBuf>) -> Self {
        Self {
            stand_in: stand_in.into(),
            vars: Vec::new(),
            daemon_vars: Vec::new(),
            unset: Vec::new(),
            first: Vec::new(),
            path: Vec::new(),
            existing_root: None,
            daemon: None,
            claude_config: None,
            cf: None,
            codex: None,
            devin: None,
            opencode: None,
            finder: false,
            masker: None,
            reports: None,
            homes: None,
            case: None,
            judged: false,
        }
    }

    /// As the Finder starts an app's daemon when the login shell gives it no
    /// `PATH`: on the system's alone, `/usr/bin:/bin:/usr/sbin:/sbin`, with no
    /// stand-in on it. Each is where its harness installs itself: `claude` (and a
    /// Devin's, an OpenCode's) in the home's `.local/bin`, and a Codex's in the
    /// machine's own folder of programs, the one Homebrew's `codex` is in
    /// (`/opt/homebrew/bin`), which is the rig's (`install::tool_folder`). The
    /// owner's machine the night a task to a Codex worker was refused
    /// (2026-10-15).
    #[must_use]
    pub fn as_the_finder_starts_it(mut self) -> Self {
        self.finder = true;
        self
    }

    /// With the report of a rig a case leaves unclosed kept in `folder`, not beside
    /// the rig's home in the machine's temp folder: for a rig whose windows are a
    /// real harness's, whose screens the person's login drew and no masker is sure
    /// to hide (the lead's decision of 2026-10-16), and which the machine's temp
    /// folder would keep for good.
    #[must_use]
    pub fn reports_in(mut self, folder: impl Into<PathBuf>) -> Self {
        self.reports = Some(folder.into());
        self
    }

    /// With the rig's report, and the line that says where it is, naming `case` as the case
    /// that left it, where the rig is not started on a thread named for its case: a run
    /// that is no test. A rig started on the thread of a test is that test's, by the
    /// thread's name.
    #[must_use]
    pub fn case(mut self, case: impl Into<String>) -> Self {
        self.case = Some(case.into());
        self
    }

    /// With the rig's case judged by what it returns ([`Outcome`](crate::Outcome)): a rig
    /// dropped unclosed in a case that has not failed was dropped by a case that passed,
    /// and has no report to keep. A rig not judged, which is every rig whose case does not
    /// return one (a live suite's, a run's), cannot tell and keeps its report for every
    /// drop unclosed.
    #[must_use]
    pub fn judged_by_outcome(mut self) -> Self {
        self.judged = true;
        self
    }

    /// With the rig's home made in `folder`, and not in the machine's temp folder: for a case
    /// that wants its rig's home, and the homes a rig that starts sweeps, in a folder of its
    /// own. A rig that starts takes away the homes in that folder that a killed run left:
    /// those older than a day that no process works in.
    #[must_use]
    pub fn homes_in(mut self, folder: impl Into<PathBuf>) -> Self {
        self.homes = Some(folder.into());
        self
    }

    /// With what the rig says of itself, when a case leaves it unclosed (its windows'
    /// screens, the programs' error output, the daemon's log), passed through
    /// `masker` first: for an owner that hides what looks like a key or a token in all
    /// it says (the long run of the evals). Every other rig says it as it is.
    #[must_use]
    pub fn masked_by(mut self, masker: Masker) -> Self {
        self.masker = Some(masker);
        self
    }

    /// With `stand_in`, the stand-in Codex (the `fake-codex` binary of this crate),
    /// as the `codex` the windows find on their `PATH`: the windows of Codex's
    /// agents run on it, each on a thread of its own, its TUI waiting to be ended,
    /// its record in the rig's Codex home. A script that runs it, so Unix's: there
    /// is no such shim on Windows.
    #[must_use]
    pub fn codex(mut self, stand_in: impl Into<PathBuf>) -> Self {
        self.codex = Some(stand_in.into());
        self
    }

    /// With the windows of a Devin agent running `program`, the stand-in for Devin
    /// (the `fake-devin` binary of this crate), which they find first on their
    /// `PATH` as `devin`, as the windows of a Claude agent find the stand-in
    /// agent as `claude`.
    #[must_use]
    pub fn devin(mut self, program: impl Into<PathBuf>) -> Self {
        self.devin = Some(program.into());
        self
    }

    /// With the `opencode` the windows find first on their `PATH` the stand-in
    /// `program` (the `fake-opencode-2` binary of this crate): the OpenCode of a
    /// machine that has updated itself to 2, which says so to `--version`, answers
    /// nothing else, and writes down what it is asked in `opencode.jsonl` of the
    /// rig's folder ([`Rig::opencode_asked`]).
    #[must_use]
    pub fn opencode_2(mut self, program: impl Into<PathBuf>) -> Self {
        self.opencode = Some(program.into());
        self
    }

    /// With the `opencode` the windows find first on their `PATH` the stand-in
    /// `program` (the `fake-opencode-1` binary of this crate): an OpenCode 1.x the
    /// daemon launches a window on as it launches one on OpenCode, which serves
    /// what the launch asks of OpenCode's server and writes down what it is asked
    /// and the requests it is sent in `opencode.jsonl` of the rig's folder
    /// ([`Rig::opencode_asked`], [`Rig::opencode_requests`]).
    #[must_use]
    pub fn opencode(mut self, program: impl Into<PathBuf>) -> Self {
        self.opencode = Some(program.into());
        self
    }

    /// With the variable `name` set to `value` in the environment of the
    /// daemon, the pane host and so every window, over what the rig sets.
    #[must_use]
    pub fn var(mut self, name: &str, value: impl Into<String>) -> Self {
        self.vars.push((name.to_owned(), value.into()));
        self
    }

    /// With the variable `name` set to `value` in the daemon's environment alone, over
    /// what the rig sets: as the app gives its daemon what it took of the human's
    /// terminal (`TERMINAL_ENV`), which the pane host, started from the app's own
    /// environment, never has.
    #[must_use]
    pub fn daemon_var(mut self, name: &str, value: impl Into<String>) -> Self {
        self.daemon_vars.push((name.to_owned(), value.into()));
        self
    }

    /// Without the variable `name` in that environment, whatever the rig sets
    /// and whatever [`Config::var`] gave it: for a real harness that reads its
    /// config where the rig's own value does not point (Claude Code with
    /// `CLAUDE_CONFIG_DIR` set keeps its config there, finds no login, and
    /// opens on its first-run dialog).
    #[must_use]
    pub fn unset(mut self, name: &str) -> Self {
        self.unset.push(name.to_owned());
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

    /// With the folder `dir` on that `PATH` right after the folder of the stand-in
    /// `claude`, before the folders [`Config::also_on_path`] adds and the system's:
    /// for a program of a case's own that is found before the machine's of its
    /// name (a stand-in `cf`, where the machine has the real one beside the
    /// folders of its harnesses).
    #[must_use]
    pub fn first_on_path(mut self, dir: impl Into<PathBuf>) -> Self {
        self.first.push(dir.into());
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

    /// With the daemon's `cf` copied from `program`, and not from the release build:
    /// for the case that takes the program away once the rig is up, as a build does.
    #[must_use]
    pub fn cf_from(mut self, program: impl Into<PathBuf>) -> Self {
        self.cf = Some(program.into());
        self
    }

    /// With the Claude Code config folder of every window `claude_config`'s, in place
    /// of the folder the rig makes in its home: for a case whose windows run the real
    /// Claude Code on a login of the machine's, copied into a folder of its own, so
    /// that nothing of the run is written in the person's `~/.claude`
    /// ([`ClaudeConfig`]). The rig asks it for the folder when it starts, and for
    /// what the run came to when it ends.
    #[must_use]
    pub fn claude_config(mut self, claude_config: Arc<dyn ClaudeConfig>) -> Self {
        self.claude_config = Some(claude_config);
        self
    }
}

/// A Claude Code config folder that a rig's windows (and its daemon, which reads
/// the records Claude keeps there) run on in place of the one the rig makes, kept
/// by the owner of a live case ([`Config::claude_config`]).
pub trait ClaudeConfig: std::fmt::Debug + Send + Sync {
    /// The folder, made when it is first asked for and the same after: a rig that
    /// starts again on the same home (the app's restart) finds its conversations
    /// where they were. An error is why there is none, said before anything is spent.
    fn folder(&self) -> Result<PathBuf>;

    /// Where the login in the folder is copied from, as a run says it.
    fn described(&self) -> String;

    /// Copies what the run's Claude Code kept in the folder (its conversations and
    /// its sessions, never the login) into `into`, the `.claude` of the home the rig made
    /// (`<root>/home/.claude`, beside `CONSENSFLOW_HOME`), which is where a kept home has them: asked when a rig whose home is kept ends, the
    /// windows gone, before [`ClaudeConfig::finish`]; the folder itself goes with the
    /// run. Copying again over a copy made before is the same copy, grown. A folder
    /// that holds no login of the person's has nothing to take out of it.
    fn keep_records(&self, _into: &Path) -> Result {
        Ok(())
    }

    /// What the run came to in the folder and in the person's own setup, asked
    /// when a rig that used the folder ends, the windows gone: an error says what
    /// the run did that it was not to, and fails the case that ends the rig.
    fn finish(&self) -> Result;
}

/// What a rig that closed leaves: its root, kept, and the last word of the pane
/// host on its windows, read to its end: every exit it told, those since a case last
/// asked among them. (The host tells none of the windows it reaps as it shuts down:
/// its panes' threads send `pane.exit` after its bridge has closed.)
#[derive(Debug, Clone, PartialEq)]
pub struct Last {
    pub root: PathBuf,
    /// Every `pane.exit` the host told, with when the rig saw it ([`Rig::timed_exits`]).
    pub exits: Vec<(Value, i64)>,
    /// Every `pane.kill` the daemon asked ([`Rig::kills`]).
    pub kills: Vec<Value>,
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
    /// The `cf` the daemon runs, which its windows find first on their `PATH`: the rig's own
    /// copy (`crate::bundle`).
    cf: PathBuf,
    /// A process none of the windows' is, which nothing may have ended when they are.
    stranger: Stranger,
    /// The Claude config folder the windows run on, where a case gave one
    /// ([`Config::claude_config`]), and whether it has been asked what the run came
    /// to ([`ClaudeConfig::finish`]).
    claude_config: Option<Arc<dyn ClaudeConfig>>,
    claude_config_finished: bool,
    /// What it passes what it says of itself through ([`Config::masked_by`]).
    masker: Option<Masker>,
    /// Where its report is kept, where it is not beside its home ([`Config::reports_in`]).
    reports: Option<PathBuf>,
    /// The case that started it, as its report names it ([`Config::case`]).
    case: String,
    /// Whether its case is judged by what it returns, and whether it has failed
    /// ([`Config::judged_by_outcome`]); the failures of the thread it started on then.
    judged: Option<Verdict>,
    /// Whether the home is left when the rig ends: it is not, unless a rig is
    /// to resume on it or a case asked to keep it ([`Rig::keep_home`]).
    keep_root: bool,
    /// Whether the rig ended: set when a close succeeded, never by one that failed.
    closed: bool,
    /// Whether a close was tried and failed.
    close_failed: bool,
    /// Whether the case holds its close to fail ([`Rig::expect_the_close_to_fail`]).
    close_expected_to_fail: AtomicBool,
    /// Whether the case ends the rig as a drop does, on purpose ([`Rig::close_as_dropped`]).
    dropped_on_purpose: bool,
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

    /// The `cf` the daemon runs: the one a window of the rig finds first, and the one
    /// whose path the daemon writes in what it gives the windows.
    pub fn cf(&self) -> &Path {
        &self.cf
    }

    /// The daemon's process id.
    pub fn daemon_pid(&self) -> u32 {
        self.daemon.id()
    }

    /// The pane host's process id: the process that holds the windows' terminals and
    /// reads their output.
    pub fn host_pid(&self) -> u32 {
        self.host.id()
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
            seen.daemon,
            seen.host,
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

    /// Which process the pane host opened the program of the window `pane` (`id`,
    /// `generation`) as: the `pid` of its answer to that `pane.open`, read once as the
    /// answer passed and kept ([`route::Root`]). None until it answered, or where it said
    /// none, or the process had gone by the time it was read.
    pub fn window_process(&self, pane: &Value) -> Option<Identity> {
        window_process_in(&self.shared.seen().roots, pane)
    }

    /// Every `pane.exit` the pane host told so far, in order: which window
    /// ended (`id`, `generation`), how (`exitCode`, `signal`) and the last lines
    /// its screen showed (`tail`).
    pub fn exits(&self) -> Vec<Value> {
        self.shared.seen().exits.clone()
    }

    /// Every `pane.exit` the pane host told so far, each with when the rig saw it,
    /// in milliseconds since the epoch: when the window ended, which a case that
    /// looks every few seconds would otherwise put at its next look.
    pub fn timed_exits(&self) -> Vec<(Value, i64)> {
        let seen = self.shared.seen();
        seen.exits
            .iter()
            .cloned()
            .zip(seen.exit_times.iter().copied())
            .collect()
    }

    /// The process each window was opened on, as the pane host answered the
    /// daemon's `pane.open`: `{ id, generation, pid }`, in the order the host
    /// answered (a window it could not open has none).
    pub fn window_pids(&self) -> Vec<Value> {
        self.shared.seen().pids.clone()
    }

    /// Every `pane.kill` the daemon asked of the pane host so far, in order: the
    /// windows it closed itself (`id`, `generation`). A window that exited with
    /// none asked of it ended by itself, or by someone else's hand.
    pub fn kills(&self) -> Vec<Value> {
        self.shared.seen().kills.clone()
    }

    /// How many times the daemon pressed the key `bytes` into the window `id`
    /// (Escape is `[27]`, Control-C `[3]`): the `pane.input` requests it made of
    /// the pane host that are exactly that.
    pub fn keys_pressed(&self, id: &str, bytes: &[u8]) -> usize {
        self.presses(id, bytes).len()
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
        Run::new(&self.cf)
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
    /// did: where it is. A rig that starts a day later takes it away unless a process
    /// works in it.
    pub fn keep_home(&mut self) -> PathBuf {
        self.keep_root = true;
        self.root.clone()
    }

    /// Ends the rig: both programs' input is ended, both are waited for, and
    /// every process a window started is checked to be gone (and, on Unix, the
    /// process that is none of any window's to run still). The home goes with
    /// it.
    pub fn close(mut self) -> Result<()> {
        self.shutdown(EXIT)
    }

    /// Begins the close without waiting for it: the daemon's input and the pane host's
    /// end, as the app's quit ends them. [`Rig::shut_down_within`] then waits for them, its
    /// time counted from there, so a case can wait for an event of the close first.
    pub fn begin_close(&self) {
        self.daemon.end_input();
        self.host.end_input();
    }

    /// [`Rig::close`], but the daemon and the pane host each have `within` to end,
    /// where [`Rig::close`] gives them a few seconds, which suits the stand-in
    /// agents that leave when asked: a real harness's window may take longer than
    /// that, and the host gives a program that does not leave ten seconds before
    /// it kills it.
    pub fn close_within(mut self, within: Duration) -> Result<()> {
        self.shut_down_within(within)
    }

    /// [`Rig::close_within`], but the rig is kept: what the daemon and the pane
    /// host did not end in `within` is still running until the rig is dropped,
    /// which kills them, for a look at what a rig that did not end left.
    pub fn shut_down_within(&mut self, within: Duration) -> Result<()> {
        self.shutdown(within)
    }

    /// [`Rig::close`], but the home is left, for a rig that resumes on it
    /// ([`Config::existing_root`]). Where it is.
    pub fn close_keeping_the_root(self) -> Result<PathBuf> {
        self.close_keeping_the_root_within(EXIT)
    }

    /// [`Rig::close_keeping_the_root`], the daemon and the pane host each given
    /// `within` to end, as [`Rig::close_within`] gives them: for real harnesses.
    pub fn close_keeping_the_root_within(mut self, within: Duration) -> Result<PathBuf> {
        self.keep_root = true;
        self.shutdown(within)?;
        Ok(self.root.clone())
    }

    /// [`Rig::close_keeping_the_root_within`], with what the pane host said last of
    /// its windows, read to the end of its output: every exit it told, which a case
    /// that took the exits before the close may not have seen yet (a process a
    /// window's close ended, named in its `pane.exit` and nowhere before). Where the
    /// router has not read to the end of the host's output a few seconds after the
    /// host ended, [`Error::Unread`], with what it had read.
    pub fn close_with_the_last_exits_within(mut self, within: Duration) -> Result<Last> {
        self.keep_root = true;
        // The host ends, and the router reads what it wrote to its end after it.
        self.shutdown(within)?;
        last_word(&self.shared, self.root.clone(), LAST_FRAMES)
    }

    /// For a case that holds the close to fail (the rig's checks of what the pane host
    /// ended, of what a window left running): a close that fails keeps no report of
    /// the rig, since the failure is what the case is about and it says it. That is
    /// the rig's whole report, so an error the case returns after the expected failure
    /// (not a panic, which keeps one) keeps none either: the case says what it needs to
    /// in the error. A close that fails in any other case does keep one, and so does a
    /// case that never tries it.
    pub fn expect_the_close_to_fail(&self) {
        self.close_expected_to_fail.store(true, Ordering::Relaxed);
    }

    /// What the drop of the rig says of itself. A panic in flight is a failed case's. A rig
    /// closed, whose close the case holds to fail, or that the case ended as a drop on
    /// purpose ([`Rig::close_as_dropped`]), has nothing to say. Any other is
    /// dropped unclosed: a case that is not judged by what it returns cannot say that it
    /// passed, and the rig says it; one that is says it once the case is known to have
    /// failed, whether that is already or comes later (`outcome::leave`).
    fn drop_says(&self) -> DropSays {
        if std::thread::panicking() {
            return DropSays::Now;
        }
        let failure_expected =
            self.close_failed && self.close_expected_to_fail.load(Ordering::Relaxed);
        if self.closed || failure_expected || self.dropped_on_purpose {
            return DropSays::Nothing;
        }
        self.judged
            .map_or(DropSays::Now, DropSays::OnceItsCaseFails)
    }

    /// Ends the rig as a case that failed leaves it, dropped unclosed: the drop's own way
    /// (its home kept or taken away, its Claude config asked what the run came to), for
    /// a case about that way. It keeps no report of the rig: the case holds to the drop,
    /// and passes or fails by what it finds after.
    pub fn close_as_dropped(mut self) {
        self.dropped_on_purpose = true;
    }

    /// Ends the rig, as [`Rig::close`] says. It is closed once that succeeded: a close
    /// that failed leaves the rig to be dropped unclosed, which keeps its report.
    fn shutdown(&mut self, exit: Duration) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        let ended = self.end(exit);
        if ended.is_ok() {
            self.closed = true;
        } else {
            self.close_failed = true;
        }
        ended
    }

    fn end(&mut self, exit: Duration) -> Result<()> {
        self.daemon.end_input();
        self.host.end_input();
        let (daemon, host) = (self.daemon.wait(exit)?, self.host.wait(exit)?);
        if daemon.is_none() || host.is_none() {
            return Err(Error::Timeout(format!(
                "process did not exit{}",
                self.what_they_said()
            )));
        }
        // What the pane host ended, it ended as the windows' own: a process that is
        // none of theirs, started beside it, was ended by nothing (it may have left
        // of itself, which is no end the pane host gave it).
        if let Some(how) = self.stranger.ended_by_another() {
            return Err(Error::Daemon(format!(
                "a process that is none of any window's was ended with them ({how}){}",
                self.what_they_said()
            )));
        }
        // The pane host kills each window's process as it shuts down, and a
        // killed process leaves the process table a moment later (once it is
        // reaped): one still there after a few seconds was left behind.
        if let Some(left) = left::left_behind(&self.root.join("pids.jsonl"), GONE) {
            return Err(Error::Daemon(format!(
                "fake harness process remains after bridge shutdown: {left}"
            )));
        }
        // Every window is gone: nothing writes in the Claude config folder any more.
        self.finish_claude_config()
    }

    /// Asks the Claude config folder the windows ran on what the run came to
    /// ([`ClaudeConfig::finish`]), once: the answer to the case that ends the rig.
    fn finish_claude_config(&mut self) -> Result {
        match &self.claude_config {
            Some(claude_config) if !self.claude_config_finished => {
                self.claude_config_finished = true;
                // The home is kept: what Claude Code kept of the run goes in it, where it
                // has always been (the folder holds a login, and goes with the run).
                let kept = if self.keep_root {
                    claude_config.keep_records(&self.root.join("home").join(".claude"))
                } else {
                    Ok(())
                };
                claude_config.finish().and(kept)
            }
            _ => Ok(()),
        }
    }
}

impl Rig {
    /// What a failed case of the rig leaves of it, as the report the rig keeps says it: the
    /// windows' screens, the daemon's failure events and log, and what the stand-in agents
    /// wrote, all told through the masker the rig was given.
    pub fn failure_report(&self) -> String {
        report::report_of(self)
    }
}

/// What the drop of a rig does with the report of what it showed.
enum DropSays {
    /// Nothing: the case closed it, or holds its close to fail.
    Nothing,
    /// It says it now: a panic is in flight, or the rig's case cannot say that it passed.
    Now,
    /// It says it once the case has failed, as the verdict reads.
    OnceItsCaseFails(Verdict),
}

impl Drop for Rig {
    /// A rig not closed (a case that failed on the way, by a check, by an error it
    /// returned, or by a close that failed) is ended all the same, saying what the
    /// windows showed first; and the home goes unless it is to be kept. A rig whose case
    /// passed says nothing: one dropped unclosed by a case that is judged by what it
    /// returns ([`Config::judged_by_outcome`]) writes what it shows while it is up, and
    /// that is said only if the case fails; the scan of the suites
    /// (`tests/rig/reports.rs`) is what holds the cases to the close.
    fn drop(&mut self) {
        match self.drop_says() {
            DropSays::Nothing => {}
            DropSays::Now => report::left_by(self).say(),
            DropSays::OnceItsCaseFails(verdict) => {
                let left = report::left_by(self);
                outcome::leave(verdict, move || left.say());
            }
        }
        let _ = self.shutdown(EXIT);
        self.daemon.kill();
        self.host.kill();
        // A rig that did not end by being closed (a case that failed) is still asked
        // what the run did in the person's setup, which the folder says loudly.
        let _ = self.finish_claude_config();
        if !self.keep_root {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// The last word of the pane host the routers of `shared` read, once its output has
/// ended, waited for up to `within`; the rig's root is `root`. Where it has not ended
/// by then, [`Error::Unread`] with what was read so far: an exit the host tells after
/// is none of it.
fn last_word(shared: &Shared, root: PathBuf, within: Duration) -> Result<Last> {
    let ended = wait::until(within, || shared.seen().host_ended);
    let seen = shared.seen();
    let last = Last {
        root,
        exits: seen
            .exits
            .iter()
            .cloned()
            .zip(seen.exit_times.iter().copied())
            .collect(),
        kills: seen.kills.clone(),
    };
    if ended {
        return Ok(last);
    }
    Err(Error::Unread {
        said: format!(
            "the pane host's output had not ended {} s after the host did: what it told after is not read (host frames={})",
            within.as_secs(),
            seen.host
        ),
        last: Box::new(last),
    })
}

/// The process the pane host said, among the windows it answered the open of (`roots`),
/// it opened the program of the window `pane` (`id`, `generation`) as: as it was read
/// then, the last where it answered more than once. It is never read again: the number may
/// be another process's by now.
fn window_process_in(roots: &[route::Root], pane: &Value) -> Option<Identity> {
    roots
        .iter()
        .rev()
        .find(|root| root.id == pane["id"] && root.generation == pane["generation"])
        .and_then(|root| root.identity)
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
    fn a_windows_process_is_the_one_read_as_the_host_answered_its_open_and_never_read_again() {
        use serde_json::json;
        let root = |id: &str, generation: u64, identity: Option<Identity>| route::Root {
            id: json!(id),
            generation: json!(generation),
            identity,
        };
        // The window p1-zeus's first process was this test's number, begun long before this
        // test: it has gone, and the number is this test's process now. A look that read the
        // number again took this process for the window's (Astra and Sol on round 6).
        let gone = Identity {
            pid: std::process::id(),
            started: 1,
        };
        let other = Identity {
            pid: 200,
            started: 20,
        };
        let roots = [
            root("p1-zeus", 1, Some(gone)),
            root("p1-zeus", 2, Some(other)),
            root("p1-hera", 1, None),
        ];
        let pane = |id: &str, generation: u64| json!({ "id": id, "generation": generation });
        assert_eq!(window_process_in(&roots, &pane("p1-zeus", 1)), Some(gone));
        assert_eq!(window_process_in(&roots, &pane("p1-zeus", 2)), Some(other));
        // A host that said no process of it, and a window it never answered the open of.
        assert_eq!(window_process_in(&roots, &pane("p1-hera", 1)), None);
        assert_eq!(window_process_in(&roots, &pane("p1-zeus", 3)), None);
    }

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

    #[test]
    fn a_last_word_not_read_to_its_end_in_time_is_an_error_that_holds_what_was_read() {
        let shared = Arc::new(Shared::default());
        let root = PathBuf::from("/nowhere");
        {
            let mut seen = shared.seen();
            seen.exits
                .push(serde_json::json!({ "id": "p1-chief", "generation": 1 }));
            seen.exit_times.push(1_000);
        }
        // The host's last exit comes late: after the wait for its output's end.
        let late = Arc::clone(&shared);
        let telling = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(600));
            let mut seen = late.seen();
            seen.exits
                .push(serde_json::json!({ "id": "p1-worker", "generation": 2 }));
            seen.exit_times.push(2_000);
            seen.host_ended = true;
        });
        let Err(Error::Unread { said, last }) =
            last_word(&shared, root.clone(), Duration::from_millis(100))
        else {
            panic!("the output had not ended, and it was taken for the whole");
        };
        assert!(said.contains("had not ended"), "{said}");
        assert_eq!(last.exits.len(), 1, "what was read: {:?}", last.exits);
        assert_eq!(last.root, root);
        // Waited for long enough, the last word holds the late exit.
        telling.join().unwrap();
        let whole = last_word(&shared, root, Duration::from_secs(5)).unwrap();
        assert_eq!(whole.exits.len(), 2);
        assert_eq!(whole.exits[1].1, 2_000);
    }
}
