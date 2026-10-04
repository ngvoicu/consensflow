//! The supervisor: starts Codex's app-server and, once the broker is up
//! between them, the TUI; ends both when the window closes; and leaves
//! nothing behind.
//!
//! The app-server is started first, on the endpoint the window owns, and given
//! the arguments the TUI may not override; it is up once its endpoint says so
//! (15 seconds at most). The broker connects to it, and the TUI to the broker
//! with the launch's token in its environment, never on its command line. The
//! window ends when the TUI does, and the exit code is the TUI's.

use std::cell::RefCell;
use std::ffi::OsString;
use std::future::pending;
use std::io;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_board::door::DOOR_WAIT;
use cf_process::{runnable, terminate, Ending};
use cf_proto::codex::{Bridge, InvalidBridge, BRIDGE_VARIABLE};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, Command};
use tokio::task::JoinHandle;

use crate::arguments::{self, consensflow_shell_environment, MissingValue, Split, BYPASS};
use crate::broker::{Broker, Config, StartError};
use crate::endpoint::{Endpoint, EndpointError, Upstream};
use crate::questions::board_of;
use crate::tail::Tail;

/// How long Codex's server has to come up, and how often that is looked at.
const STARTUP: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(50);
/// How long the app-server has to end when asked, before it is made to.
const GRACE: Duration = Duration::from_millis(1500);
/// How long a failed server's last words are waited for.
const LAST_WORDS: Duration = Duration::from_millis(200);

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionError {
    #[error("no Codex program was named")]
    NoProgram,
    #[error(transparent)]
    Bridge(#[from] InvalidBridge),
    #[error(transparent)]
    Arguments(#[from] MissingValue),
    #[error(transparent)]
    Endpoint(#[from] EndpointError),
    #[error("Codex server could not start: {0}")]
    Server(String),
    #[error(transparent)]
    Broker(#[from] StartError),
    #[error("{program}: {cause}")]
    Tui { program: String, cause: io::Error },
    #[error("could not watch for the signals that close a window: {0}")]
    Signals(io::Error),
}

/// What one window is run with.
struct Plan<'a> {
    env: &'a Env,
    executable: &'a Path,
    bridge: Bridge,
    split: Split,
    endpoint: Endpoint,
    /// Whether the window opens in full-permission mode.
    bypass: bool,
    /// How long Codex's server has to come up.
    startup: Duration,
}

/// How a window's server is set up: how long it has to come up, and where it
/// listens (this platform's endpoint, but for a test).
struct Setup {
    startup: Duration,
    endpoint: fn(&Env) -> Result<Endpoint, EndpointError>,
}

/// Runs the window: `args` are the Codex program and its arguments. The
/// result is the exit code, the TUI's; everything is ended and removed by then.
pub(crate) async fn supervise(env: &Env, args: &[OsString]) -> Result<i32, SessionError> {
    let setup = Setup {
        startup: STARTUP,
        endpoint: Endpoint::open,
    };
    supervise_with(env, args, setup).await
}

/// [`supervise`], the server set up as `setup` says.
async fn supervise_with(env: &Env, args: &[OsString], setup: Setup) -> Result<i32, SessionError> {
    let (executable, codex_args) = args.split_first().ok_or(SessionError::NoProgram)?;
    let bridge = Bridge::parse(env.text(BRIDGE_VARIABLE).unwrap_or_default())?;
    let split = arguments::split(codex_args)?;
    let endpoint = (setup.endpoint)(env)?;
    let plan = Plan {
        env,
        executable: Path::new(executable),
        bridge,
        split,
        endpoint,
        bypass: codex_args.iter().any(|arg| arg == BYPASS),
        startup: setup.startup,
    };
    let mut session = Session::default();
    let outcome = session.run(&plan).await;
    session.finish(&plan).await;
    outcome
}

/// The processes of one window, and its broker.
#[derive(Default)]
struct Session {
    backend: Option<Child>,
    tui: Option<Child>,
    broker: Option<Broker>,
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
            board: board_of(plan.env),
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
        Ok(status.ok().and_then(|status| status.code()).unwrap_or(0))
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
        args.extend(consensflow_shell_environment(plan.env));
        args.push("app-server".into());
        args.extend(plan.endpoint.listen().iter().cloned());
        let (mut command, program) = command(plan, &args);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut backend = command
            .spawn()
            .map_err(|cause| SessionError::Server(format!("{program}: {cause}")))?;
        let stderr = backend.stderr.take();
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
                .is_some_and(|backend| matches!(backend.try_wait(), Ok(Some(_))));
            if ended || Instant::now() > deadline {
                // What a program that ended wrote last may still be in the pipe.
                let _ = tokio::time::timeout(LAST_WORDS, drain).await;
                return Err(SessionError::Server(tail.borrow().text().to_string()));
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
        let (mut command, program) = command(plan, &args);
        command
            .env("CF_CODEX_TUI_TOKEN", &plan.bridge.token)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let tui = command
            .spawn()
            .map_err(|cause| SessionError::Tui { program, cause })?;
        self.tui = Some(tui);
        Ok(())
    }

    /// Asks both to end.
    fn stop(&self) {
        end(self.tui.as_ref(), Ending::Asked);
        end(self.backend.as_ref(), Ending::Asked);
    }

    /// Ends what is left: both processes asked to end, the broker closed, the
    /// app-server made to end if it has not within a moment, the socket's folder removed.
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
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

/// `plan`'s program as it starts here, with `args`, and the program that
/// starts (what a failure to start names). An OpenAI API key in this
/// environment never reaches Codex, which has its own login; and a program
/// still running when its handle is dropped is killed, not orphaned.
fn command(plan: &Plan<'_>, args: &[OsString]) -> (Command, String) {
    let run = runnable(plan.executable, args, plan.env);
    let program = run.program.display().to_string();
    let mut command = Command::from(run.command());
    command.env_remove("OPENAI_API_KEY").kill_on_drop(true);
    (command, program)
}

/// Asks `child` to end, if it has not been waited for.
fn end(child: Option<&Child>, how: Ending) {
    if let Some(pid) = child.and_then(Child::id) {
        terminate(pid, how);
    }
}

/// How `child` ended; never, when there is none.
async fn exit_of(child: &mut Option<Child>) -> io::Result<ExitStatus> {
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

/// The signals that close a window: SIGTERM and SIGINT, and Ctrl-C on Windows.
struct Signals {
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
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
