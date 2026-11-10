//! The supervisor: starts Codex's app-server and, once the broker is up
//! between them, the TUI; ends both when the window closes; and leaves
//! nothing behind.
//!
//! The app-server is started first, on the endpoint the window owns, and given
//! the arguments the TUI may not override; it is up once its endpoint says so
//! (15 seconds at most). The broker connects to it, and the TUI to the broker
//! with the launch's token in its environment, never on its command line. The
//! window ends when the TUI does, and the exit code is the TUI's.
//!
//! The window's role is read before anything starts, from the file its launch's
//! environment names (`CF_CODEX_ROLE_FILE`), and handed to the broker, which gives
//! it to the TUI's main starts: it is on none of the three command lines. A file
//! that is named and cannot be read, or holds nothing, opens no window, and nor
//! does the variable set to nothing: a window without its role would work as no one
//! asked it to. Neither of Codex's programs is told the file's name. ConsensFlow's
//! own variables (the board's token among them) are on no command line either: the
//! broker gives them to each thread in its `config`.
//!
//! What the window's sub-agents start with in place of its role is a second file of
//! the launch's (`CF_CODEX_SUBAGENT_FILE`, the harness's `codex::role`), which the
//! app-server alone is told of, as the configuration of each of Codex's own agent
//! types (`-c agents.<type>.config_file=…`) that the launch names
//! (`CF_CODEX_SUBAGENT_TYPES`, all of them where it names none; the owner's own
//! definition of one stays theirs), with Codex's own words for it: its path is on that
//! command line, and nothing of the role is in it. A variable set to nothing, a file
//! named that is not there or holds nothing, or a type Codex does not have opens no
//! window, as a role's does: a window without it would start its sub-agents as itself.
//!
//! Each program is started through cf-process's `start_owned`, which on Windows runs it in a job of
//! its own: ending it ends everything it started, what it starts as it is ended included (its id
//! alone, ended with `taskkill /T`, leaves those; zeewin, T-322). A program that ended of itself has
//! its job released, and what it left is left.

use std::cell::RefCell;
use std::ffi::OsString;
use std::future::pending;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_base::startup::{harness_exit, StartKind, StartRefusal, SOCKET_FOLDER_LEFT};
use cf_board::door::{board_of, DOOR_WAIT};
use cf_process::{not_spawned, runnable, start_owned, Ending, Owned};
use cf_proto::codex::agent_types::{agent_type, AgentType, AGENT_TYPES};
use cf_proto::codex::{
    Bridge, InvalidBridge, BRIDGE_VARIABLE, ROLE_VARIABLE, SUBAGENT_TYPES_VARIABLE,
    SUBAGENT_VARIABLE,
};
use tokio::io::AsyncReadExt;
use tokio::process::{ChildStderr, Command};
use tokio::task::JoinHandle;

use crate::arguments::{self, consensflow_variables, MissingValue, Split, BYPASS};
use crate::broker::{Broker, Config, StartError};
use crate::endpoint::{Endpoint, EndpointError, Upstream};
use crate::tail::Tail;

/// How long Codex's server has to come up, and how often that is looked at.
const STARTUP: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(50);
/// How long the app-server has to end when asked, before it is made to.
const GRACE: Duration = Duration::from_millis(1500);
/// How long a failed server's last words are waited for.
const LAST_WORDS: Duration = Duration::from_millis(200);

/// How Codex's app-server went when it did not come up. What it wrote is on the window's screen,
/// and so is how it ended: a window's exit has one byte on a Mac, which carries the kind of failure
/// and no more (`cf_base::startup::StartKind::exit_status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServerEnd {
    /// It ended, in the way [`Exit`] says.
    Ended(Exit),
    /// It was not ready in the time it had.
    Silent,
}

impl ServerEnd {
    /// How the server went, in a few words after "could not start": the number it ended with or the
    /// signal that ended it, and no path.
    fn words(self) -> String {
        match self {
            Self::Ended(Exit::Status(code)) => format!(" (it ended with status {code})"),
            Self::Ended(Exit::Signal(signal)) => format!(" (it was ended by signal {signal})"),
            Self::Ended(Exit::Unknown) | Self::Silent => String::new(),
        }
    }
}

/// How a program ended: the status it returned, or the signal that ended it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exit {
    Status(u32),
    #[cfg_attr(not(unix), allow(dead_code, reason = "only Unix has signals"))]
    Signal(i32),
    /// The system said neither.
    Unknown,
}

impl Exit {
    fn of(status: ExitStatus) -> Self {
        if let Some(code) = status.code() {
            // A Windows status is a `u32` that `i32` carries as it is (`0xC0000005`).
            return Self::Status(u32::from_ne_bytes(code.to_ne_bytes()));
        }
        #[cfg(unix)]
        if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
            return Self::Signal(signal);
        }
        Self::Unknown
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionError {
    #[error("no Codex program was named")]
    NoProgram,
    #[error(transparent)]
    Bridge(#[from] InvalidBridge),
    #[error(transparent)]
    Arguments(#[from] MissingValue),
    #[error("could not read its role from {}: {cause}", path.display())]
    Role { path: PathBuf, cause: io::Error },
    #[error("its role file {} holds nothing", path.display())]
    NoRole { path: PathBuf },
    #[error("{ROLE_VARIABLE} names no file")]
    NoRoleFile,
    #[error("could not read what its sub-agents start with from {}: {cause}", path.display())]
    Children { path: PathBuf, cause: io::Error },
    #[error("the file of what its sub-agents start with, {}, holds nothing", path.display())]
    NoChildren { path: PathBuf },
    #[error("{SUBAGENT_VARIABLE} names no file")]
    NoChildrenFile,
    #[error("{SUBAGENT_TYPES_VARIABLE} names {0}, which is none of Codex's own agent types")]
    NoAgentType(String),
    #[error(transparent)]
    Endpoint(#[from] EndpointError),
    /// The app-server ended, or was not ready in time: what it wrote to its error stream, and how it
    /// went (the number it ended with, which the window's terminal shows and the note does not).
    #[error("Codex server could not start{}: {said}", .how.words())]
    Server { said: String, how: ServerEnd },
    /// The system would not start Codex's server.
    #[error("Codex server could not start: {program}: {cause}")]
    ServerNotStarted { program: String, cause: io::Error },
    /// Codex is an npm shim and no Node is to be found to run it on.
    #[error("{0}")]
    Unrunnable(StartRefusal),
    #[error(transparent)]
    Broker(#[from] StartError),
    #[error("{program}: {cause}")]
    Tui { program: String, cause: io::Error },
    #[error("could not watch for the signals that close a window: {0}")]
    Signals(io::Error),
}

impl SessionError {
    /// The kind of failure, where this is a window that did not get Codex ready: the window's
    /// program then ends with the status of that kind (`cf_base::startup::StartKind::exit_status`),
    /// so that the engine reads from how the window ended, and not from its screen, that Codex never
    /// ran, and a note says the kind and what to do, and no word of the program's, the server's or
    /// a path (T-1328, the second and third readings of T-1322). A window's exit is one byte on a
    /// Mac: the kind is all it carries, and the cause is on the window's own screen, of which
    /// ConsensFlow's log keeps what the engine saw.
    ///
    /// Every failure before Codex's TUI ran is one (each variant is decided here, none by a
    /// wildcard). The failure to remove the window's socket folder is none and no `SessionError`
    /// (T-1337): it comes after the window did its work, so it can win over no failure (`finish`).
    pub(crate) fn startup(&self) -> StartKind {
        match self {
            Self::Unrunnable(refusal) => refusal.failure().kind(),
            Self::ServerNotStarted { program, cause } | Self::Tui { program, cause } => {
                not_spawned(Path::new(program), cause).failure().kind()
            }
            Self::Server {
                how: ServerEnd::Ended(_),
                ..
            } => StartKind::ServerStopped,
            Self::Server {
                how: ServerEnd::Silent,
                ..
            } => StartKind::ServerSilent,
            Self::NoProgram
            | Self::Bridge(_)
            | Self::Arguments(_)
            | Self::Role { .. }
            | Self::NoRole { .. }
            | Self::NoRoleFile
            | Self::Children { .. }
            | Self::NoChildren { .. }
            | Self::NoChildrenFile
            | Self::NoAgentType(_)
            | Self::Endpoint(_)
            | Self::Broker(_)
            | Self::Signals(_) => StartKind::NotSetUp,
        }
    }
}

/// What one window is run with.
struct Plan<'a> {
    env: &'a Env,
    executable: &'a Path,
    bridge: Bridge,
    /// The developer instructions the window's main threads start with.
    role: Option<String>,
    /// The thread the window was launched to resume, if it was.
    resumed: Option<String>,
    /// The configuration its sub-agents start with, which the app-server is told of.
    children: Option<Children>,
    split: Split,
    endpoint: Endpoint,
    /// Whether the window opens in full-permission mode.
    bypass: bool,
    /// How long Codex's server has to come up.
    startup: Duration,
    /// Why the system gave a program of the window no job, if it gave none.
    refusal: fn(&Owned) -> Option<String>,
}

/// How a window's server is set up: how long it has to come up, and where it
/// listens (this platform's endpoint, but for a test); what tells it the system gave a
/// program no job, and where it says so (what [`Owned::refusal`] says and its standard
/// error, but for a test).
struct Setup {
    startup: Duration,
    endpoint: fn(&Env) -> Result<Endpoint, EndpointError>,
    refusal: fn(&Owned) -> Option<String>,
    say: fn(&str),
}

impl Setup {
    /// A window as it runs.
    fn standard() -> Self {
        Self {
            startup: STARTUP,
            endpoint: Endpoint::open,
            refusal: |program| program.refusal().map(str::to_owned),
            say: |line| {
                let _ = writeln!(io::stderr(), "{line}");
            },
        }
    }
}

/// Runs the window: `args` are the Codex program and its arguments. The
/// result is the window's exit code, the TUI's (`cf_base::startup::harness_exit`: one of the
/// statuses this program ends with for a start that failed is never a status of Codex's);
/// everything is ended and removed by then, and a socket folder that could not be removed is said
/// on the terminal without its path and written to ConsensFlow's log, and changes nothing of the
/// result.
pub(crate) async fn supervise(env: &Env, args: &[OsString]) -> Result<i32, SessionError> {
    supervise_with(env, args, Setup::standard()).await
}

/// [`supervise`], the server set up as `setup` says.
async fn supervise_with(env: &Env, args: &[OsString], setup: Setup) -> Result<i32, SessionError> {
    let (executable, codex_args) = args.split_first().ok_or(SessionError::NoProgram)?;
    let bridge = Bridge::parse(env.text(BRIDGE_VARIABLE).unwrap_or_default())?;
    let role = role_of(env)?;
    let children = children_of(env)?;
    let split = arguments::split(codex_args)?;
    let endpoint = (setup.endpoint)(env)?;
    let plan = Plan {
        env,
        executable: Path::new(executable),
        bridge,
        role,
        resumed: arguments::resumed(codex_args),
        children,
        split,
        endpoint,
        bypass: codex_args.iter().any(|arg| arg == BYPASS),
        startup: setup.startup,
        refusal: setup.refusal,
    };
    let mut session = Session::default();
    let outcome = session.run(&plan).await;
    // What is left is ended and removed whatever came of the run, and what comes of that never
    // replaces how the run went: the first failure wins (T-1337).
    session.finish(&plan).await;
    session.say_what_ran_in_no_job(setup.say);
    outcome
}

/// The window's role: the text of the file `env` names in [`ROLE_VARIABLE`], none
/// where the variable is not set. A variable set to nothing is refused, as an empty
/// file is: a launch that meant to give a role and gave none.
fn role_of(env: &Env) -> Result<Option<String>, SessionError> {
    if env.os(ROLE_VARIABLE).is_none() {
        return Ok(None);
    }
    let Some(file) = env.path(ROLE_VARIABLE) else {
        return Err(SessionError::NoRoleFile);
    };
    let path = file.to_path_buf();
    match std::fs::read_to_string(file) {
        Ok(text) if text.is_empty() => Err(SessionError::NoRole { path }),
        Ok(text) => Ok(Some(text)),
        Err(cause) => Err(SessionError::Role { path, cause }),
    }
}

/// What the window's sub-agents start with in place of its role.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Children {
    /// The file of their configuration.
    file: PathBuf,
    /// The agent types whose sub-agents start from it.
    kinds: Vec<&'static AgentType>,
}

/// What the window's sub-agents start with, which `env` names: the file in
/// [`SUBAGENT_VARIABLE`], none where the variable is not set, and the types in
/// [`SUBAGENT_TYPES_VARIABLE`], each of Codex's own where it is not set and none where it
/// is set to nothing (the owner defines every one; a launch then names no file at all). The
/// file's variable set to nothing is refused, as a file that cannot be read or holds
/// nothing is, and a type that is none of Codex's own.
fn children_of(env: &Env) -> Result<Option<Children>, SessionError> {
    if env.os(SUBAGENT_VARIABLE).is_none() {
        return Ok(None);
    }
    let Some(file) = env.path(SUBAGENT_VARIABLE) else {
        return Err(SessionError::NoChildrenFile);
    };
    let path = file.to_path_buf();
    let file = match std::fs::read_to_string(file) {
        Ok(text) if text.is_empty() => Err(SessionError::NoChildren { path }),
        Ok(_) => Ok(path),
        Err(cause) => Err(SessionError::Children { path, cause }),
    }?;
    // Set to nothing, the variable names no type: the owner defines every one.
    let kinds = match env.os(SUBAGENT_TYPES_VARIABLE) {
        None => AGENT_TYPES.iter().collect(),
        Some(named) => named
            .to_string_lossy()
            .split(',')
            .filter(|name| !name.is_empty())
            .map(|name| agent_type(name).ok_or_else(|| SessionError::NoAgentType(name.to_owned())))
            .collect::<Result<_, _>>()?,
    };
    Ok(Some(Children { file, kinds }))
}

/// The processes of one window, and its broker.
#[derive(Default)]
struct Session {
    backend: Option<Owned>,
    tui: Option<Owned>,
    broker: Option<Broker>,
    /// What the window is to say once it has ended, on its standard error: which of its programs
    /// the system gave no job (see [`without_a_job`]), and a status of Codex's that was reported as 1.
    notes: Vec<String>,
}

impl Session {
    async fn run(&mut self, plan: &Plan<'_>) -> Result<i32, SessionError> {
        // Before anything starts, so no signal is missed.
        let mut signals = Signals::new().map_err(SessionError::Signals)?;
        let tail = Rc::new(RefCell::new(Tail::default()));
        let drain = self.start_backend(plan, &tail)?;
        let upstream = self.wait_until_up(plan, &tail, drain, &mut signals).await?;

        let starting = Broker::start(Config {
            bridge: plan.bridge.clone(),
            upstream,
            fresh_bypass: plan.bypass,
            role: plan.role.clone(),
            resumed: plan.resumed.clone(),
            shell_env: consensflow_variables(plan.env),
            board: board_of(plan.env).map(Arc::new),
            question_wait: DOOR_WAIT,
        });
        let broker = self.until_started(starting, &mut signals).await?;
        let port = broker.port();
        self.broker = Some(broker);
        self.start_tui(plan, port)?;

        // The window ends with its TUI, and its TUI with the server.
        let mut backend_ended = false;
        let status = loop {
            tokio::select! {
                status = exit_of(&mut self.tui) => break status,
                _ = exit_of(&mut self.backend), if !backend_ended => {
                    backend_ended = true;
                    end(self.tui.as_ref(), Ending::Asked);
                }
                () = signals.recv() => self.stop(),
            }
        };
        // Ended by a signal, there is no code: the window was closed, not failed.
        let code = status.ok().and_then(|status| status.code()).unwrap_or(0);
        // The statuses this program ends with for a start that failed are its own: a Codex that
        // ended with one is reported as a failure, 1, and the window says so (T-1328).
        let reported = harness_exit(code);
        if reported != code {
            let (first, last) = StartKind::exit_statuses();
            self.notes.push(format!(
                "ConsensFlow: Codex ended with status {code}; this window ends with {reported}, as {first} to {last} are ConsensFlow's own, for a start that failed"
            ));
        }
        Ok(reported)
    }

    /// Starts the app-server with the arguments the TUI may not override, its
    /// standard error read for as long as it runs. The returned task ends when
    /// that stream does.
    fn start_backend(
        &mut self,
        plan: &Plan<'_>,
        tail: &Rc<RefCell<Tail>>,
    ) -> Result<JoinHandle<()>, SessionError> {
        let mut args = plan.split.backend.clone();
        if let Some(children) = &plan.children {
            args.extend(arguments::children(&children.file, &children.kinds));
        }
        args.push("app-server".into());
        args.extend(plan.endpoint.listen().iter().cloned());
        let (mut command, program) = command(plan, &args).map_err(SessionError::Unrunnable)?;
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut backend = start_owned(&mut command)
            .map_err(|cause| SessionError::ServerNotStarted { program, cause })?;
        let stderr = backend.take_stderr();
        self.note_if_in_no_job("Codex's server", (plan.refusal)(&backend));
        self.backend = Some(backend);
        let tail = Rc::clone(tail);
        Ok(tokio::task::spawn_local(async move {
            if let Some(stderr) = stderr {
                read_all(stderr, &tail).await;
            }
        }))
    }

    /// Waits until the app-server's endpoint says it is up: where, for the
    /// broker. It fails with what the server wrote to its standard error when
    /// it ends first or does not come up in time.
    async fn wait_until_up(
        &mut self,
        plan: &Plan<'_>,
        tail: &Rc<RefCell<Tail>>,
        drain: JoinHandle<()>,
        signals: &mut Signals,
    ) -> Result<Upstream, SessionError> {
        let deadline = Instant::now() + plan.startup;
        loop {
            if let Some(upstream) = plan.endpoint.upstream(tail.borrow().text()) {
                return Ok(upstream);
            }
            let ended = self
                .backend
                .as_mut()
                .and_then(|backend| backend.try_wait().ok().flatten());
            if ended.is_some() || Instant::now() > deadline {
                // What a program that ended wrote last may still be in the pipe.
                let _ = tokio::time::timeout(LAST_WORDS, drain).await;
                return Err(SessionError::Server {
                    said: tail.borrow().text().to_string(),
                    how: ended.map_or(ServerEnd::Silent, |status| {
                        ServerEnd::Ended(Exit::of(status))
                    }),
                });
            }
            tokio::select! {
                () = tokio::time::sleep(POLL) => {}
                () = signals.recv() => self.stop(),
            }
        }
    }

    /// Waits for the broker to start; a signal meanwhile ends Codex's
    /// processes, which the start then fails for.
    async fn until_started(
        &mut self,
        starting: impl std::future::Future<Output = Result<Broker, StartError>>,
        signals: &mut Signals,
    ) -> Result<Broker, SessionError> {
        let mut starting = std::pin::pin!(starting);
        loop {
            tokio::select! {
                started = &mut starting => return Ok(started?),
                () = signals.recv() => self.stop(),
            }
        }
    }

    /// Starts the TUI, attached to the broker on `port`, on this terminal.
    fn start_tui(&mut self, plan: &Plan<'_>, port: u16) -> Result<(), SessionError> {
        let mut args: Vec<OsString> = vec![
            "--remote".into(),
            format!("ws://127.0.0.1:{port}").into(),
            "--remote-auth-token-env".into(),
            "CF_CODEX_TUI_TOKEN".into(),
        ];
        args.extend(plan.split.tui.iter().cloned());
        let (mut command, program) = command(plan, &args).map_err(SessionError::Unrunnable)?;
        command
            .env("CF_CODEX_TUI_TOKEN", &plan.bridge.token)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let tui =
            start_owned(&mut command).map_err(|cause| SessionError::Tui { program, cause })?;
        self.note_if_in_no_job("Codex's TUI", (plan.refusal)(&tui));
        self.tui = Some(tui);
        Ok(())
    }

    /// Keeps a line to say, if the system gave the program `name` no job (`refusal` says why):
    /// it is then ended with `taskkill`.
    fn note_if_in_no_job(&mut self, name: &str, refusal: Option<String>) {
        if let Some(why) = refusal {
            self.notes.push(without_a_job(name, &why));
        }
    }

    /// Says what was noted, once the window has ended, on the terminal it ran in: where its
    /// programs have gone, the line is there to be read (and not drawn over, as one said while
    /// the TUI ran would be).
    fn say_what_ran_in_no_job(&self, say: fn(&str)) {
        for note in &self.notes {
            say(note);
        }
    }

    /// Asks both to end.
    fn stop(&self) {
        end(self.tui.as_ref(), Ending::Asked);
        end(self.backend.as_ref(), Ending::Asked);
    }

    /// Ends what is left: both processes asked to end, the broker closed, the
    /// app-server made to end if it has not within a moment, the socket's folder removed. A folder
    /// that cannot be removed is no failure of the window (it did its work, or failed before), and
    /// its path can hold a credential: the window says on its terminal that it is left, without the
    /// path or the cause, and ConsensFlow's log has both (T-1337).
    async fn finish(&mut self, plan: &Plan<'_>) {
        self.stop();
        if let Some(broker) = self.broker.take() {
            broker.close().await;
        }
        if let Some(backend) = &mut self.backend {
            if matches!(backend.try_wait(), Ok(None))
                && tokio::time::timeout(GRACE, backend.wait()).await.is_err()
            {
                end(Some(&*backend), Ending::Forced);
                let _ = backend.wait().await;
            }
        }
        if let Some(directory) = plan.endpoint.directory() {
            if let Err(cause) = remove_folder(directory) {
                crate::log::warn(
                    plan.env,
                    &format!(
                        "cf codex-session could not remove the Codex socket folder {}",
                        directory.display()
                    ),
                    &cause.to_string(),
                );
                self.notes.push(SOCKET_FOLDER_LEFT.to_owned());
            }
        }
    }
}

/// What the window says of a program of its own the system gave no job: that it ended with
/// `taskkill`, which loses what the program starts as it is ended until the window closes.
fn without_a_job(name: &str, why: &str) -> String {
    format!("ConsensFlow: {name} ran in no job of its own, the system gave it none ({why}); it was ended with taskkill, which can leave what it started as it was ended running until the window closes")
}

/// Removes `folder` with what is in it; one already gone is no failure, as
/// `rm` with `force` had it.
fn remove_folder(folder: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(folder) {
        Err(cause) if cause.kind() != io::ErrorKind::NotFound => Err(cause),
        _ => Ok(()),
    }
}

/// `plan`'s program as it starts here, with `args`, and the program that
/// starts (what a failure to start names); or why it cannot start (a Codex
/// that is an npm shim, with no Node to run it on). An OpenAI API key in this
/// environment never reaches Codex, which has its own login, nor the name of the
/// role's file, which is the broker's to read, nor the variable that names its
/// sub-agents' (the app-server is told the file itself); and a program
/// still running when its handle is dropped is killed, not orphaned (and everything it started
/// with it, on Windows).
fn command(plan: &Plan<'_>, args: &[OsString]) -> Result<(Command, String), StartRefusal> {
    let run = runnable(plan.executable, args, plan.env)?;
    let program = run.program.display().to_string();
    let mut command = Command::from(run.command());
    // Both run for as long as the window does.
    cf_process::hold_only_the_streams(command.as_std_mut());
    command
        .env_remove("OPENAI_API_KEY")
        .env_remove(ROLE_VARIABLE)
        .env_remove(SUBAGENT_VARIABLE)
        .env_remove(SUBAGENT_TYPES_VARIABLE)
        .kill_on_drop(true);
    Ok((command, program))
}

/// Asks `child` to end, if it has not been waited for.
fn end(child: Option<&Owned>, how: Ending) {
    if let Some(child) = child {
        child.end(how);
    }
}

/// How `child` ended; never, when there is none.
async fn exit_of(child: &mut Option<Owned>) -> io::Result<ExitStatus> {
    match child {
        Some(child) => child.wait().await,
        None => pending().await,
    }
}

/// Reads `stderr` to its end into `tail`.
async fn read_all(mut stderr: ChildStderr, tail: &RefCell<Tail>) {
    let mut buffer = [0_u8; 4096];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => tail.borrow_mut().push(&buffer[..read]),
        }
    }
}

/// The signals that close a window: SIGTERM, SIGHUP and SIGINT, and Ctrl-C on Windows.
/// The pane host closes one with SIGTERM and then SIGHUP at once, as a terminal hangs
/// up: a SIGHUP left to its default ended the supervisor before it had ended Codex's
/// server, which then took as long as its own shutdown did to go (10 s and more on
/// Codex 0.164 closed soon after it started), and the Close waited for it (T-542).
struct Signals {
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(unix)]
    hangup: tokio::signal::unix::Signal,
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(windows)]
    ctrl_c: tokio::signal::windows::CtrlC,
}

impl Signals {
    fn new() -> io::Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            Ok(Self {
                terminate: signal(SignalKind::terminate())?,
                hangup: signal(SignalKind::hangup())?,
                interrupt: signal(SignalKind::interrupt())?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                ctrl_c: tokio::signal::windows::ctrl_c()?,
            })
        }
    }

    /// Waits for the next one.
    async fn recv(&mut self) {
        #[cfg(unix)]
        tokio::select! {
            _ = self.terminate.recv() => {}
            _ = self.hangup.recv() => {}
            _ = self.interrupt.recv() => {}
        }
        #[cfg(windows)]
        {
            self.ctrl_c.recv().await;
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests;

#[cfg(test)]
mod everywhere;
