//! The daemon as the app holds it. It starts on a thread of its own
//! and again after a start that failed, stops with the app, and every change
//! is told to the page as `daemon-status`.

use std::panic::{self, AssertUnwindSafe};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use cf_bridge::{Bridge, BridgeBuilder, BridgeError};
use cf_panes::headless::{reap_until_clean, Reaped};
use cf_panes::pty::PaneTable;

use crate::daemon_command::LoginLookup;
use crate::runtime::SHUTDOWN_REAP;

/// How long the daemon gets to stop on its own before it is killed.
pub(crate) const DAEMON_STOP_GRACE: Duration = Duration::from_secs(2);
/// What the page is told of the daemon (see `DaemonStatus`).
pub(crate) const DAEMON_STATUS_EVENT: &str = "daemon-status";
/// Between starts of a daemon that failed to start.
pub(crate) const DAEMON_RESTART: Backoff = Backoff {
    first: Duration::from_secs(1),
    most: Duration::from_secs(30),
    restarts: 3,
    stable: Duration::from_secs(60),
    cleanup: SHUTDOWN_REAP,
};
const DAEMON_STOPPED: &str = "ConsensFlow's daemon stopped while the app was running";
/// What the person is told once, plainly, while the app starts the daemon again: why the
/// windows closed and what is paused, before anything else (the page adds that it is
/// starting it again).
const DAEMON_STOPPED_RESTARTING: &str = "ConsensFlow's daemon stopped while the app was running, so its windows were closed and your open projects are paused until you resume them";
/// What the person is told when a window of the daemon that stopped is not proven gone in
/// time: no other daemon is started beside what may still run.
const DAEMON_NOT_CLEANED: &str = "ConsensFlow's daemon stopped while the app was running, and a window it started has not ended yet, so it is not started again while that window might still run";
/// What the person is told when the app gave up: the restarts did not hold.
const DAEMON_KEEPS_STOPPING: &str = "ConsensFlow's daemon keeps stopping: it was started again three times, and each time it stopped within a minute";
/// How long a daemon has to say it is ready (its handle line) before its
/// start counts as failed. It opens its ledger first and migrates it, and
/// each step of a migration rebuilds a table and checks every reference in
/// the ledger under full sync: a limit too short would cut each start of a
/// big ledger short, undone every time, and the app would never start. A
/// daemon that hangs is still reported, and started again, within two
/// minutes.
pub(crate) const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(120);

/// What the page is told about the daemon (its `daemon-status`): up, or down,
/// why, and whether the app is starting it again.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct DaemonStatus {
    available: bool,
    cause: Option<String>,
    retrying: bool,
}

impl DaemonStatus {
    fn up() -> Self {
        Self {
            available: true,
            cause: None,
            retrying: false,
        }
    }

    fn down(cause: &str, retrying: bool) -> Self {
        Self {
            available: false,
            cause: Some(cause.to_string()),
            retrying,
        }
    }
}

/// Why a start of the daemon failed, and whether another start could fare
/// better: a runtime missing from the app does not come back.
pub(crate) struct DaemonFailure {
    pub(crate) cause: String,
    pub(crate) retry: bool,
}

impl DaemonFailure {
    fn transient(cause: String) -> Self {
        Self { cause, retry: true }
    }
}

/// A daemon that started: its process, its bridge, and the address of the
/// agents screens it handed the app.
pub(crate) struct StartedDaemon {
    daemon: Child,
    bridge: Bridge,
    roster: RosterHandle,
}

impl StartedDaemon {
    fn stop(mut self) {
        self.bridge.close_input();
        stop_daemon(&mut self.daemon);
    }
}

/// Starts the daemon once. What it is given is told when that daemon's bridge
/// closes.
pub(crate) type DaemonStarter =
    Arc<dyn Fn(Box<dyn Fn() + Send + Sync>) -> Result<StartedDaemon, DaemonFailure> + Send + Sync>;
pub(crate) type DaemonReport = Arc<dyn Fn(&DaemonStatus) + Send + Sync>;

/// The waits between starts: the first, then twice as long each time, up to
/// the most. And how often a daemon that stopped while the app ran is started again:
/// `restarts` times in a row, each a start that failed or a daemon that stopped within
/// `stable` of coming up. A daemon that stayed up longer than that begins the count again.
#[derive(Clone, Copy)]
pub(crate) struct Backoff {
    first: Duration,
    most: Duration,
    restarts: u32,
    stable: Duration,
    /// How long a start again waits for the windows of the daemon that stopped to be
    /// proven gone ([`Daemon::restart`]).
    cleanup: Duration,
}

/// The daemon as the app holds it, and what the page knows of it.
///
/// A start that failed (the ledger still held by a daemon finishing its stop
/// after a force-quit, a migration that throws, a missing runtime) left the
/// app without its daemon for the session, the cause in a detail the page never
/// read; a daemon that died mid-session closed its bridge without a word and
/// the board froze. Now the page is told every change, and a failed start is
/// tried again: it ended before its handle line, so it left no window behind
/// it.
///
/// A daemon that stops later is started again too (T-1344: a Mac at a load of 170 left
/// a daemon's bridge full, the bridge closed, the daemon ended, and the person was told to
/// quit and open the app, whose quit ends every window that is working). Its windows are
/// closed first, as a quit closes them: each was launched with the dead daemon's token,
/// which no other knows, and told what the board was, which went with it. The new daemon
/// takes the board from the ledger as at any start, with each open project suspended for the
/// person to resume. The person is told once, plainly, and asked to quit only when the
/// restarts do not hold ([`Backoff::restarts`]).
pub(crate) struct Daemon {
    state: Mutex<DaemonState>,
    /// Woken when the first start has its outcome, or the app stops.
    settled: Condvar,
    report: DaemonReport,
    /// The login shell a start is asking for the daemon's PATH, which the
    /// app's stop ends.
    login: Arc<LoginLookup>,
}

struct DaemonState {
    daemon: Option<Child>,
    bridge: Option<Bridge>,
    roster: Option<RosterHandle>,
    status: DaemonStatus,
    /// The first start has its outcome. Until then the page is told nothing,
    /// since a daemon that is starting is not down, and what it asks waits.
    settled: bool,
    /// Starts so far, and the one whose bridge is in hand: only that bridge
    /// closing is the daemon stopping.
    starts: u64,
    current: u64,
    stopping: bool,
    /// What a start again needs, once the first start was asked for.
    restart: Option<Restart>,
    /// When the daemon in hand came up: a stop that comes sooner than
    /// [`Backoff::stable`] after it counts against the restarts in a row.
    up_since: Option<Instant>,
    /// The restarts in a row ([`Backoff::restarts`]).
    restarts: u32,
}

/// How a daemon that stopped is started again.
#[derive(Clone)]
struct Restart {
    starter: DaemonStarter,
    panes: Arc<PaneTable>,
    backoff: Backoff,
}

impl Daemon {
    pub(crate) fn new(report: DaemonReport) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(DaemonState {
                daemon: None,
                bridge: None,
                roster: None,
                status: DaemonStatus::down("ConsensFlow's daemon has not started", false),
                settled: false,
                starts: 0,
                current: 0,
                stopping: false,
                restart: None,
                up_since: None,
                restarts: 0,
            }),
            settled: Condvar::new(),
            report,
            login: Arc::default(),
        })
    }

    /// The login shell its starts ask for the daemon's PATH, while one asks.
    pub(crate) fn login(&self) -> Arc<LoginLookup> {
        Arc::clone(&self.login)
    }

    fn lock(&self) -> MutexGuard<'_, DaemonState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Starts the daemon from a thread of its own, so the app's window opens
    /// meanwhile: a daemon may take a while to start (a migration on a big
    /// ledger), and the first start used to run before the window existed.
    /// A start that panics ends as one that failed, saying the panic's words:
    /// unsettled, whatever the page asked meanwhile, a request or an agents
    /// screen, would wait until the app quits (T-978).
    pub(crate) fn start(
        self: &Arc<Self>,
        starter: DaemonStarter,
        panes: Arc<PaneTable>,
        backoff: Backoff,
    ) {
        let daemon = Arc::clone(self);
        self.lock().restart = Some(Restart {
            starter: Arc::clone(&starter),
            panes: Arc::clone(&panes),
            backoff,
        });
        if let Err(error) = thread::Builder::new()
            .name("consensflow-daemon-start".to_string())
            .spawn(move || {
                let run = || daemon.run(&starter, &panes, backoff);
                if let Err(panic) = panic::catch_unwind(AssertUnwindSafe(run)) {
                    let said = panic
                        .downcast_ref::<&str>()
                        .map(|said| (*said).to_owned())
                        .or_else(|| panic.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "a panic that says nothing".to_owned());
                    daemon.fail(&DaemonFailure {
                        cause: format!(
                            "ConsensFlow's daemon could not be started: its start stopped on an error ({said})"
                        ),
                        retry: false,
                    });
                }
            })
        {
            self.fail(&DaemonFailure {
                cause: format!("ConsensFlow's daemon could not be started: {error}"),
                retry: false,
            });
        }
    }

    /// Starts the daemon, and again after a start that failed, waiting longer
    /// each time, while no pane is open and the app is not stopping.
    fn run(self: &Arc<Self>, starter: &DaemonStarter, panes: &PaneTable, backoff: Backoff) {
        let mut wait = backoff.first;
        loop {
            let Err(failure) = self.attempt(starter) else {
                return;
            };
            self.fail(&failure);
            if !failure.retry {
                return;
            }
            thread::sleep(wait);
            wait = (wait * 2).min(backoff.most);
            let status = {
                let state = self.lock();
                if state.stopping {
                    return;
                }
                state.status.clone()
            };
            if !matches!(panes.list(), Ok(open) if open.is_empty()) {
                self.tell(DaemonStatus {
                    retrying: false,
                    ..status
                });
                return;
            }
        }
    }

    /// One start. What it starts is the daemon from then on, unless the app
    /// began to stop meanwhile, which stops it again.
    fn attempt(self: &Arc<Self>, starter: &DaemonStarter) -> Result<(), DaemonFailure> {
        let start = {
            let mut state = self.lock();
            state.starts += 1;
            state.starts
        };
        let daemon = Arc::downgrade(self);
        let started = starter(Box::new(move || {
            if let Some(daemon) = daemon.upgrade() {
                daemon.closed(start);
            }
        }))?;
        let mut state = self.lock();
        if state.stopping {
            drop(state);
            started.stop();
            return Ok(());
        }
        // A process that ended before it was in hand was not yet the one
        // held when its bridge closed, so its end is read here.
        let ended = started.bridge.is_closed();
        state.current = start;
        state.daemon = Some(started.daemon);
        state.bridge = Some(started.bridge);
        state.roster = Some(started.roster);
        state.up_since = Some(Instant::now());
        if ended {
            self.stopped(state);
            return Ok(());
        }
        self.change(&mut state, DaemonStatus::up());
        Ok(())
    }

    fn fail(&self, failure: &DaemonFailure) {
        eprintln!("consensflow: {}", failure.cause);
        self.tell(DaemonStatus::down(&failure.cause, failure.retry));
    }

    /// A start's bridge closed: if it is the daemon's, the daemon stopped.
    fn closed(self: &Arc<Self>, start: u64) {
        let state = self.lock();
        if state.stopping || state.current != start || !state.status.available {
            return;
        }
        self.stopped(state);
    }

    /// The daemon in hand stopped while the app runs: the person is told, once, and the
    /// app starts it again ([`Daemon::restart`]) unless it has stopped again and again
    /// ([`Backoff::restarts`]), when the person is asked to quit.
    fn stopped(self: &Arc<Self>, mut state: MutexGuard<'_, DaemonState>) {
        eprintln!("consensflow: {DAEMON_STOPPED}");
        let Some(restart) = state.restart.clone() else {
            self.change(&mut state, DaemonStatus::down(DAEMON_STOPPED, false));
            return;
        };
        let stayed_up = state
            .up_since
            .is_some_and(|since| since.elapsed() >= restart.backoff.stable);
        state.restarts = if stayed_up { 1 } else { state.restarts + 1 };
        if state.restarts > restart.backoff.restarts {
            self.change(&mut state, DaemonStatus::down(DAEMON_KEEPS_STOPPING, false));
            return;
        }
        self.change(
            &mut state,
            DaemonStatus::down(DAEMON_STOPPED_RESTARTING, true),
        );
        let old = state.daemon.take();
        drop(state);
        let daemon = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("consensflow-daemon-restart".to_string())
            .spawn(move || daemon.restart(&restart, old));
        if let Err(error) = spawned {
            self.fail(&DaemonFailure {
                cause: format!("ConsensFlow's daemon could not be started again: {error}"),
                retry: false,
            });
        }
    }

    /// Starts the daemon again after it stopped: what is left of it ends, the windows close
    /// as a quit closes them (no other daemon knows what they were told, or the token each
    /// holds), and a start follows, waiting longer after each that fails, until the
    /// restarts in a row are spent.
    ///
    /// **No daemon starts while a window of the old one can still run.** The windows are
    /// closed and every close is waited for ([`reap_until_clean`]), those that had already
    /// taken a pane out of the table (a `pane.kill` using its grace) and what came into it
    /// meanwhile; a launch of the old daemon that reaches the table late is refused there
    /// (`register_fenced_pane_handlers`), so nothing is left to wait for. The wait is
    /// `Backoff::cleanup`, the time the quit gives the same work: a program is given its
    /// grace and then killed. A window not proven gone by then (a program that cannot be
    /// ended, a close that does not return) leaves the daemon down, and the person is told
    /// to quit, which ends the app's processes.
    fn restart(self: &Arc<Self>, restart: &Restart, old: Option<Child>) {
        if let Some(mut old) = old {
            stop_daemon(&mut old);
        }
        match reap_until_clean(&restart.panes, restart.backoff.cleanup) {
            Reaped::Clean => {}
            Reaped::TooLate | Reaped::Unconfirmed => {
                self.fail(&DaemonFailure {
                    cause: DAEMON_NOT_CLEANED.to_string(),
                    retry: false,
                });
                return;
            }
        }
        let mut wait = restart.backoff.first;
        loop {
            if self.lock().stopping {
                return;
            }
            let Err(failure) = self.attempt(&restart.starter) else {
                return;
            };
            let spent = {
                let mut state = self.lock();
                state.restarts += 1;
                state.restarts > restart.backoff.restarts || !failure.retry
            };
            if spent {
                self.fail(&DaemonFailure {
                    cause: format!(
                        "ConsensFlow's daemon stopped and could not be started again: {}",
                        failure.cause
                    ),
                    retry: false,
                });
                return;
            }
            self.fail(&failure);
            thread::sleep(wait);
            wait = (wait * 2).min(restart.backoff.most);
        }
    }

    /// A change, told to the page, unless the app is stopping.
    fn tell(&self, status: DaemonStatus) {
        let mut state = self.lock();
        if state.stopping {
            return;
        }
        self.change(&mut state, status);
    }

    /// Every change is told to the page. The first is the first start's
    /// outcome, which what the page asked meanwhile is waiting for.
    fn change(&self, state: &mut DaemonState, status: DaemonStatus) {
        state.status = status;
        state.settled = true;
        (self.report)(&state.status);
        self.settled.notify_all();
    }

    /// Where the daemon stands, told again: a page that has just loaded missed
    /// what was told before it listened. Nothing while the first start is
    /// under way, since a daemon that is starting is not down. Under the same
    /// lock as every change, so the last the page hears is the current one.
    pub(crate) fn tell_again(&self) {
        let state = self.lock();
        if state.settled {
            (self.report)(&state.status);
        }
    }

    /// The bridge to ask while the daemon is up; why not otherwise. Asked
    /// while the first start is under way, it waits for the outcome: a page
    /// loads meanwhile and reads the board, and a read that failed is not
    /// made again when the daemon comes up.
    pub(crate) fn connection(&self) -> Result<Bridge, String> {
        let state = self
            .settled
            .wait_while(self.lock(), |state| !state.settled && !state.stopping)
            .unwrap_or_else(|error| error.into_inner());
        match &state.bridge {
            Some(bridge) if state.status.available => Ok(bridge.clone()),
            _ => Err(state.status.cause.clone().unwrap_or_default()),
        }
    }

    /// The daemon's handle, its address and the UI token, while it is up. Asked while the
    /// first start is under way, it waits for the outcome, as a request does
    /// ([`Self::connection`]): an agents screen opened as soon as the board shows is opened
    /// once the daemon is up, not refused as down (T-951).
    pub(crate) fn roster(&self) -> Option<RosterHandle> {
        let state = self
            .settled
            .wait_while(self.lock(), |state| !state.settled && !state.stopping)
            .unwrap_or_else(|error| error.into_inner());
        state
            .status
            .available
            .then(|| state.roster.clone())
            .flatten()
    }

    /// The app is stopping: no start is taken in, nothing more is told, and
    /// nothing waits for a first start; a login shell a start is asking ends
    /// now, where the app's own end would leave it running. The first call has
    /// the daemon and its bridge handed over to be stopped.
    pub(crate) fn stop(&self) -> Option<(Option<Child>, Option<Bridge>)> {
        self.login.end();
        let mut state = self.lock();
        if state.stopping {
            return None;
        }
        state.stopping = true;
        self.settled.notify_all();
        Some((state.daemon.take(), state.bridge.clone()))
    }

    pub(crate) fn bridge(&self) -> Option<Bridge> {
        self.lock().bridge.clone()
    }
}

/// Starts a daemon and connects to it: its output carries the handle line,
/// then the bridge's frames; its input carries the app's. A daemon that has
/// not said it is ready `within` its time is ended, and its start failed.
pub(crate) fn connect_daemon(
    mut command: Command,
    builder: BridgeBuilder,
    within: Duration,
) -> Result<StartedDaemon, DaemonFailure> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(daemon_stderr())
        .spawn()
        .map_err(|error| {
            DaemonFailure::transient(format!(
                "the bundled ConsensFlow could not be started: {error}"
            ))
        })?;
    let input = child.stdout.take().ok_or_else(|| {
        DaemonFailure::transient("the daemon process gave no output to read".to_string())
    })?;
    let writer = child.stdin.take().ok_or_else(|| {
        DaemonFailure::transient("the daemon process gave no input pipe".to_string())
    })?;
    let failed = |child: &mut Child, cause: String| {
        let _ = child.kill();
        let _ = child.wait();
        DaemonFailure::transient(cause)
    };
    // The handle line is read on a thread of its own, since the read has no
    // limit: one behind a daemon that hung held the start for good.
    let (handed, handle) = mpsc::channel();
    if let Err(error) = thread::Builder::new()
        .name("consensflow-daemon-handle".to_string())
        .spawn(move || {
            let _ = handed.send(builder.connect(input, writer));
        })
    {
        return Err(failed(
            &mut child,
            format!("could not read the bundled ConsensFlow's handle: {error}"),
        ));
    }
    let connected = match handle.recv_timeout(within) {
        Ok(Ok(connected)) => connected,
        Ok(Err(BridgeError::Eof)) => {
            return Err(failed(
                &mut child,
                "ConsensFlow's daemon stopped before it was ready".to_string(),
            ));
        }
        Ok(Err(error)) => {
            return Err(failed(
                &mut child,
                format!("could not connect to the bundled ConsensFlow: {error}"),
            ));
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(failed(
                &mut child,
                format!("ConsensFlow's daemon did not say it was ready within {within:?}"),
            ));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(failed(
                &mut child,
                "could not read the bundled ConsensFlow's handle".to_string(),
            ));
        }
    };
    match RosterHandle::from_value(connected.handle) {
        Ok(roster) => Ok(StartedDaemon {
            daemon: child,
            bridge: connected.bridge,
            roster,
        }),
        Err(cause) => Err(failed(&mut child, cause)),
    }
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct RosterHandle {
    pub(crate) url: String,
    pub(crate) token: String,
}

impl RosterHandle {
    pub(crate) fn from_value(value: Value) -> Result<Self, String> {
        let mut handle: Self = serde_json::from_value(value)
            .map_err(|error| format!("the daemon returned an invalid handle: {error}"))?;
        if !handle.url.starts_with("http://127.0.0.1:")
            && !handle.url.starts_with("http://localhost:")
        {
            return Err("the daemon's handle is not a loopback HTTP address".to_string());
        }
        if handle.token.is_empty() {
            return Err("the daemon's handle omitted its UI token".to_string());
        }
        handle.url = handle.url.replace("http://127.0.0.1:", "http://localhost:");
        Ok(handle)
    }
}

/// Where the daemon's error output goes: the app's error log. On Windows the
/// daemon opens it itself, kept the same way, since a windowed app whose own
/// stderr could not be pointed at the log has none to hand down (inheriting
/// an invalid handle fails the spawn); elsewhere it inherits the app's.
#[cfg(windows)]
fn daemon_stderr() -> Stdio {
    crate::error_log()
        .and_then(|log| {
            std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(log)
                .ok()
        })
        .map_or_else(Stdio::null, Stdio::from)
}

#[cfg(not(windows))]
fn daemon_stderr() -> Stdio {
    Stdio::inherit()
}

/// Gives the daemon a moment to stop on its own once its input has ended:
/// it writes down why it stopped and closes its ledger. The same on every
/// platform, since an input ending is the one stop Windows can deliver too.
/// Only what has not gone by then is killed, so a start with no stop after it
/// in the daemon's log means it was killed from outside, never by the app.
pub(crate) fn stop_daemon(daemon: &mut Child) {
    // The bridge holds the daemon's input and has let it go; a child whose
    // input is still ours (a stand-in in a test) gets its EOF here.
    drop(daemon.stdin.take());
    let deadline = Instant::now() + DAEMON_STOP_GRACE;
    while Instant::now() < deadline {
        if matches!(daemon.try_wait(), Ok(Some(_))) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let _ = daemon.kill();
    let _ = daemon.wait();
}

#[cfg(all(test, unix))]
impl Daemon {
    /// A daemon past its first start, holding what a test stood up: up with a
    /// bridge, still down without one.
    pub(crate) fn settled(daemon: Option<Child>, bridge: Option<Bridge>) -> Arc<Self> {
        let held = Self::new(Arc::new(|_: &DaemonStatus| {}));
        {
            let mut state = held.lock();
            if bridge.is_some() {
                state.status = DaemonStatus::up();
            }
            state.settled = true;
            state.daemon = daemon;
            state.bridge = bridge;
        }
        held
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_proto::bridge::Role;
    use std::collections::HashMap;
    #[cfg(unix)]
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use portable_pty::PtySize;
    use serde_json::json;
    use tauri::ipc::Channel;
    use tauri::{Emitter, Manager};

    use crate::commands::subscribe_output;
    use crate::runtime::AppRuntime;
    use cf_panes::arbiter::{EnterTiming, InputArbiter};
    use cf_panes::input_queue::InputQueue;
    use cf_panes::output_hub::OutputHub;
    #[cfg(unix)]
    use cf_panes::pty::process_exists;

    /// Starts between failures, short enough for a test.
    const QUICK: Backoff = Backoff {
        first: Duration::from_millis(10),
        most: Duration::from_millis(40),
        restarts: 3,
        stable: Duration::from_secs(30),
        cleanup: Duration::from_secs(10),
    };

    /// What the page hears of the daemon, as a test hears it.
    fn listener() -> (DaemonReport, mpsc::Receiver<DaemonStatus>) {
        let (told, heard) = mpsc::channel();
        (
            Arc::new(move |status: &DaemonStatus| {
                let _ = told.send(status.clone());
            }),
            heard,
        )
    }

    /// A stand-in for `cf ui --json`, started and connected the way the app
    /// starts the daemon; it prints its handle line when `ready`.
    #[cfg(unix)]
    fn stand_in_core(
        ready: bool,
        then: &str,
        closed: Box<dyn Fn() + Send + Sync>,
    ) -> Result<StartedDaemon, DaemonFailure> {
        let handle = r#"printf '%s\n' '{"url":"http://localhost:1/","token":"t"}'; "#;
        let mut command = crate::testing::command("/bin/sh");
        command
            .arg("-c")
            .arg(format!("{}{then}", if ready { handle } else { "" }));
        let mut builder = BridgeBuilder::new(Role::Host, 1024);
        builder.on_close(closed);
        connect_daemon(command, builder, Duration::from_secs(5))
    }

    /// A daemon that stops before it is ready (its ledger still held by one
    /// finishing its stop) is started again, waiting longer each time, and
    /// the page hears each failure and the start that worked.
    #[cfg(unix)]
    #[test]
    fn a_failed_daemon_start_is_tried_again_and_the_page_is_told() {
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: DaemonStarter = Arc::new(move |closed| {
            if counted.fetch_add(1, Ordering::SeqCst) < 2 {
                stand_in_core(false, "exit 1", closed)
            } else {
                stand_in_core(true, "while read -r line; do :; done", closed)
            }
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        let panes = Arc::new(PaneTable::new());
        daemon.start(starter, Arc::clone(&panes), QUICK);

        let not_ready =
            DaemonStatus::down("ConsensFlow's daemon stopped before it was ready", true);
        for told in [not_ready.clone(), not_ready, DaemonStatus::up()] {
            assert_eq!(
                heard.recv_timeout(Duration::from_secs(5)).expect("told"),
                told
            );
        }
        assert_eq!(starts.load(Ordering::SeqCst), 3);
        assert!(daemon.connection().is_ok());
        assert!(daemon.roster().is_some());

        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let runtime = AppRuntime {
            inputs: Arc::new(InputQueue::new(Arc::clone(&panes), arbiter)),
            panes,
            daemon,
            output: Arc::new(OutputHub::new()),
        };
        runtime.shutdown();
        assert!(
            heard.recv_timeout(Duration::from_millis(200)).is_err(),
            "an app that is quitting tells the page nothing"
        );
    }

    /// A daemon that never says it is ready (a migration that hangs) is
    /// killed at the limit, and its start counts as failed: the page is told
    /// and it is started again. The read of its handle line had no limit, and
    /// held the start for good.
    #[cfg(unix)]
    #[test]
    fn a_daemon_that_never_says_it_is_ready_is_killed_and_started_again() {
        let home = tempfile::tempdir().expect("home");
        let pid_file = home.path().join("hung.pid");
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let hung = pid_file.clone();
        let starter: DaemonStarter = Arc::new(move |closed| {
            if counted.fetch_add(1, Ordering::SeqCst) > 0 {
                return stand_in_core(true, "while read -r line; do :; done", closed);
            }
            let mut command = crate::testing::command("/bin/sh");
            command.arg("-c").arg(format!(
                "echo $$ > '{}'; exec /bin/sleep 60",
                hung.display()
            ));
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            builder.on_close(closed);
            connect_daemon(command, builder, Duration::from_millis(300))
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        let panes = Arc::new(PaneTable::new());
        daemon.start(starter, Arc::clone(&panes), QUICK);

        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("told"),
            DaemonStatus::down(
                "ConsensFlow's daemon did not say it was ready within 300ms",
                true
            )
        );
        let hung_pid = std::fs::read_to_string(&pid_file)
            .expect("the hung daemon's pid")
            .trim()
            .parse::<i32>()
            .expect("a pid");
        assert!(
            !process_exists(hung_pid),
            "the hung daemon was left running"
        );
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("up"),
            DaemonStatus::up()
        );
        assert_eq!(starts.load(Ordering::SeqCst), 2);

        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        AppRuntime {
            inputs: Arc::new(InputQueue::new(Arc::clone(&panes), arbiter)),
            panes,
            daemon,
            output: Arc::new(OutputHub::new()),
        }
        .shutdown();
    }

    /// The first start runs apart, so the app's window opens while the daemon
    /// starts: a page that loads meanwhile is told nothing, since a daemon that
    /// is starting is not down, and what it asks waits for the outcome. The
    /// first start used to run before the window existed, for as long as the
    /// daemon took.
    #[cfg(unix)]
    #[test]
    fn the_first_start_runs_apart_and_what_a_page_asks_waits_for_it() {
        let (release, released) = mpsc::channel::<()>();
        let released = Mutex::new(released);
        let starter: DaemonStarter = Arc::new(move |closed| {
            let _ = released.lock().expect("the release").recv();
            stand_in_core(true, "while read -r line; do :; done", closed)
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        let panes = Arc::new(PaneTable::new());
        let (returned, start_returned) = mpsc::channel();
        let starting = Arc::clone(&daemon);
        let starting_panes = Arc::clone(&panes);
        thread::spawn(move || {
            starting.start(starter, starting_panes, QUICK);
            let _ = returned.send(());
        });
        assert!(
            start_returned.recv_timeout(Duration::from_secs(2)).is_ok(),
            "the start held up the app until the daemon was ready"
        );

        daemon.tell_again();
        assert!(
            heard.recv_timeout(Duration::from_millis(100)).is_err(),
            "a daemon that is starting was told as down"
        );
        let asking = Arc::clone(&daemon);
        let (answered, answer) = mpsc::channel();
        thread::spawn(move || {
            let _ = answered.send(asking.connection().is_ok());
        });
        assert!(
            answer.recv_timeout(Duration::from_millis(200)).is_err(),
            "a request did not wait for the start"
        );
        // An agents screen opened meanwhile (Settings, then Agents, as soon as the board is
        // shown) waits for the daemon too, and opens at its address: it was refused as "the
        // daemon is not up" while the first start went on, for as long as a portable app's
        // first start unpacks its runtime on Windows (T-930, T-951).
        let screening = Arc::clone(&daemon);
        let (screened, screen) = mpsc::channel();
        thread::spawn(move || {
            let _ = screened.send(screening.roster().is_some());
        });
        assert!(
            screen.recv_timeout(Duration::from_millis(200)).is_err(),
            "an agents screen did not wait for the start"
        );

        release.send(()).expect("let the start go on");
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("up"),
            DaemonStatus::up()
        );
        assert_eq!(answer.recv_timeout(Duration::from_secs(5)), Ok(true));
        assert_eq!(
            screen.recv_timeout(Duration::from_secs(5)),
            Ok(true),
            "the agents screen opened at the daemon's address once it was up"
        );

        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        AppRuntime {
            inputs: Arc::new(InputQueue::new(Arc::clone(&panes), arbiter)),
            panes,
            daemon,
            output: Arc::new(OutputHub::new()),
        }
        .shutdown();
    }

    /// An app that stops while its first start is under way answers what a
    /// page asked meanwhile at once.
    #[test]
    fn stopping_ends_a_wait_for_the_first_start() {
        let (release, released) = mpsc::channel::<()>();
        let released = Mutex::new(released);
        let starter: DaemonStarter = Arc::new(move |_closed| {
            let _ = released.lock().expect("the release").recv();
            Err(DaemonFailure::transient("the app stopped".to_string()))
        });
        let (report, _heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::new(PaneTable::new()), QUICK);
        let asking = Arc::clone(&daemon);
        let (answered, answer) = mpsc::channel();
        thread::spawn(move || {
            let _ = answered.send(asking.connection().err());
        });
        // An agents screen asked meanwhile waits too, and is answered none (T-951, T-978).
        let screen = Arc::clone(&daemon);
        let (screened, screen_answer) = mpsc::channel();
        thread::spawn(move || {
            let _ = screened.send(screen.roster().is_some());
        });
        assert!(answer.recv_timeout(Duration::from_millis(200)).is_err());
        assert!(
            screen_answer.try_recv().is_err(),
            "an agents screen did not wait"
        );

        assert!(daemon.stop().is_some());
        assert_eq!(
            answer
                .recv_timeout(Duration::from_secs(1))
                .expect("answered"),
            Some("ConsensFlow's daemon has not started".to_string())
        );
        assert!(!screen_answer
            .recv_timeout(Duration::from_secs(1))
            .expect("an agents screen answered"));
        drop(release);
    }

    /// A start that panics settles as one that failed, with the panic's words: what the page
    /// asked meanwhile, a request or an agents screen, is answered, not left waiting until the
    /// app quits (T-978).
    #[test]
    fn a_start_that_panics_answers_what_waits_for_it_with_its_cause() {
        let (release, released) = mpsc::channel::<()>();
        let released = Mutex::new(released);
        let starter: DaemonStarter = Arc::new(
            move |_closed: Box<dyn Fn() + Send + Sync>| -> Result<StartedDaemon, DaemonFailure> {
                let _ = released.lock().expect("the release").recv();
                panic!("the start broke");
            },
        );
        let (report, _heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::new(PaneTable::new()), QUICK);
        let asking = Arc::clone(&daemon);
        let (answered, answer) = mpsc::channel();
        thread::spawn(move || {
            let _ = answered.send((asking.connection().err(), asking.roster().is_some()));
        });
        assert!(answer.recv_timeout(Duration::from_millis(200)).is_err());

        drop(release);
        let (cause, screen) = answer
            .recv_timeout(Duration::from_secs(2))
            .expect("answered once the start panicked");
        assert_eq!(
            cause.as_deref(),
            Some(
                "ConsensFlow's daemon could not be started: its start stopped on an error \
                 (the start broke)"
            )
        );
        assert!(!screen);
        daemon.stop();
    }

    /// A daemon that stops while the app runs is started again by the app, without a word
    /// from the person: its windows cannot go on with another daemon (the token each was
    /// launched with is the old one's, and what it was told the board was is gone with it), so
    /// they are closed as a quit closes them first, the new daemon takes the board from the
    /// ledger as at any start, and the page is told, once, in plain words.
    #[test]
    fn a_daemon_that_stops_mid_session_is_started_again_after_its_windows_are_closed() {
        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let (argv, folder) = a_program_that_runs_on();
        panes
            .open(
                folder.path(),
                &argv,
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open a window");
        let starts = Arc::new(AtomicUsize::new(0));
        let windows_at_start = Arc::new(Mutex::new(Vec::new()));
        let (counted, seen, table) = (
            Arc::clone(&starts),
            Arc::clone(&windows_at_start),
            Arc::clone(&panes),
        );
        let starter: DaemonStarter = Arc::new(move |closed| {
            let start = counted.fetch_add(1, Ordering::SeqCst);
            seen.lock()
                .expect("seen")
                .push(table.list().map_or(usize::MAX, |open| open.len()));
            if start == 0 {
                stand_in_over(&table, &[], Then::StopAfter(300), closed)
            } else {
                stand_in_over(&table, &[], Then::StayUp, closed)
            }
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::clone(&panes), QUICK);

        let mut told = Vec::new();
        for _ in 0..3 {
            told.push(heard.recv_timeout(Duration::from_secs(60)).expect("told"));
        }
        assert_eq!(
            told,
            [
                DaemonStatus::up(),
                DaemonStatus::down(DAEMON_STOPPED_RESTARTING, true),
                DaemonStatus::up(),
            ]
        );
        assert_eq!(starts.load(Ordering::SeqCst), 2, "started once more");
        assert_eq!(
            *windows_at_start.lock().expect("seen"),
            [1, 0],
            "the windows were closed before the new daemon started"
        );
        assert!(daemon.connection().is_ok());
        assert!(daemon.roster().is_some());
        assert!(
            heard.recv_timeout(Duration::from_millis(300)).is_err(),
            "the person was told more than once"
        );
        let (child, bridge) = daemon.stop().expect("the app stops");
        if let Some(bridge) = bridge {
            bridge.close_input();
        }
        if let Some(mut child) = child {
            stop_daemon(&mut child);
        }
    }

    /// What a stand-in daemon does once it has said its handle line and its frames.
    #[derive(Clone, Copy)]
    enum Then {
        /// Stops after this many milliseconds (its input is closed with it).
        StopAfter(u64),
        /// Reads its input to the end, and stops with it.
        StayUp,
    }

    /// The command of a stand-in for `cf ui --json` on either system: a shell on a Mac, a
    /// PowerShell on Windows. It prints its handle line, then each of `frames` (what the
    /// daemon asks of the pane host), then does `then`.
    fn stand_in_command(frames: &[String], then: Then) -> Command {
        let handle = r#"{"url":"http://localhost:1/","token":"t"}"#;
        #[cfg(unix)]
        {
            let mut script = format!("printf '%s\\n' '{handle}'; ");
            for frame in frames {
                script.push_str(&format!("printf '%s\\n' '{frame}'; "));
            }
            script.push_str(&match then {
                Then::StopAfter(milliseconds) => format!("sleep {}", milliseconds as f64 / 1000.0),
                Then::StayUp => "while read -r line; do :; done".to_string(),
            });
            let mut command = crate::testing::command("/bin/sh");
            command.arg("-c").arg(script);
            command
        }
        #[cfg(windows)]
        {
            let said = |text: &str| format!("$o.WriteLine('{}'); ", text.replace('\'', "''"));
            let mut script = format!("$o=[Console]::Out; {}", said(handle));
            for frame in frames {
                script.push_str(&said(frame));
            }
            script.push_str("$o.Flush(); ");
            script.push_str(&match then {
                Then::StopAfter(milliseconds) => {
                    format!("Start-Sleep -Milliseconds {milliseconds}")
                }
                Then::StayUp => "while ($null -ne [Console]::In.ReadLine()) {}".to_string(),
            });
            let mut command = crate::testing::command("powershell.exe");
            command
                .args(["-NoProfile", "-NonInteractive", "-Command"])
                .arg(script);
            command
        }
    }

    /// A stand-in for `cf ui --json` whose bridge answers with the app's own pane handlers over
    /// `panes` (what the app registers), saying `frames` and then doing `then`.
    fn stand_in_over(
        panes: &Arc<PaneTable>,
        frames: &[String],
        then: Then,
        closed: Box<dyn Fn() + Send + Sync>,
    ) -> Result<StartedDaemon, DaemonFailure> {
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(panes), Arc::clone(&arbiter)));
        let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
        crate::runtime::register_handlers(
            &mut builder,
            Arc::clone(panes),
            arbiter,
            Arc::new(OutputHub::new()),
            inputs,
        );
        builder.on_close(closed);
        connect_daemon(
            stand_in_command(frames, then),
            builder,
            Duration::from_secs(30),
        )
    }

    /// A program that runs for a while on either system, and a folder to run it in.
    fn a_program_that_runs_on() -> (Vec<String>, tempfile::TempDir) {
        #[cfg(unix)]
        let argv = ["/bin/sh", "-c", "sleep 30"];
        #[cfg(windows)]
        let argv = [
            r"C:\Windows\System32\cmd.exe",
            "/c",
            "ping -n 30 127.0.0.1 >nul",
        ];
        (
            argv.iter().map(|word| (*word).to_string()).collect(),
            tempfile::tempdir().expect("a folder"),
        )
    }

    /// A pane whose program ignores the request to end and says its number in `pid_file`,
    /// and what the daemon says to have it closed: the program is given its grace, which is
    /// the table's.
    #[cfg(unix)]
    fn a_pane_that_ignores_the_request_to_end(
        panes: &Arc<PaneTable>,
        pid_file: &Path,
    ) -> (String, i32) {
        let opened = panes
            .open(
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    cf_base::fixture::deaf(
                        "TERM HUP",
                        &format!("echo $$ > '{}'", pid_file.display()),
                    ),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open a window");
        let until = Instant::now() + Duration::from_secs(5);
        let pid = loop {
            if let Some(pid) = std::fs::read_to_string(pid_file)
                .ok()
                .and_then(|text| text.trim().parse::<i32>().ok())
            {
                break pid;
            }
            assert!(Instant::now() < until, "the program never said its number");
            thread::sleep(Duration::from_millis(20));
        };
        let kill = format!(
            r#"{{"v":1,"id":"n-kill","kind":"req","op":"pane.kill","body":{{"id":"{}","generation":{}}}}}"#,
            opened.key.id, opened.key.generation
        );
        (kill, pid)
    }

    /// Waits until `pid` is gone, so that a test leaves no window behind.
    #[cfg(unix)]
    fn wait_until_gone(pid: i32) {
        let until = Instant::now() + Duration::from_secs(20);
        while process_exists(pid) && Instant::now() < until {
            thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !process_exists(pid),
            "a window of the test is still running"
        );
    }

    /// A launch the old daemon asked for and the pane host was slow to begin (a loaded Mac) can
    /// never open a window once its daemon has stopped: the start again does not wait for it,
    /// and it is refused where it would insert its pane (T-1349, the reviewer's first probe: the
    /// new daemon was up at 5 s and the old handler then made a live pane on the closed bridge).
    #[test]
    fn a_launch_the_old_daemon_asked_for_opens_no_window_after_it_stopped() {
        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let held = panes.hold_opens();
        let (argv, folder) = a_program_that_runs_on();
        let open = json!({"v":1,"id":"n-open","kind":"req","op":"pane.open","body":{
            "id":"late-pane","generation":1,"cwd":folder.path(),"argv":argv}})
        .to_string();
        let starts = Arc::new(AtomicUsize::new(0));
        let (counted, table) = (Arc::clone(&starts), Arc::clone(&panes));
        let starter: DaemonStarter = Arc::new(move |closed| {
            if counted.fetch_add(1, Ordering::SeqCst) > 0 {
                return stand_in_over(&table, &[], Then::StayUp, closed);
            }
            stand_in_over(
                &table,
                std::slice::from_ref(&open),
                Then::StopAfter(300),
                closed,
            )
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::clone(&panes), QUICK);

        assert_eq!(
            heard.recv_timeout(Duration::from_secs(30)).expect("up"),
            DaemonStatus::up()
        );
        assert!(
            held.wait_for_opens(1, Duration::from_secs(30)),
            "the launch never reached the table"
        );
        assert_eq!(
            heard
                .recv_timeout(Duration::from_secs(30))
                .expect("stopped"),
            DaemonStatus::down(DAEMON_STOPPED_RESTARTING, true)
        );
        // The new daemon comes up while the launch is still held, and the launch is let go.
        assert_eq!(
            heard
                .recv_timeout(Duration::from_secs(30))
                .expect("up again"),
            DaemonStatus::up()
        );
        held.release();
        thread::sleep(Duration::from_millis(500));
        let left = panes.list().map(|open| open.len()).ok();
        let (child, bridge) = daemon.stop().expect("the app stops");
        if let Some(bridge) = bridge {
            bridge.close_input();
        }
        if let Some(mut child) = child {
            stop_daemon(&mut child);
        }
        let _ = cf_panes::headless::reap_until_clean(&panes, Duration::from_secs(20));
        assert_eq!(
            left,
            Some(0),
            "a window of the old daemon runs beside the new one"
        );
    }

    /// A window the old daemon had asked to close, whose program is using its grace to end,
    /// is out of the table and not yet gone: the new daemon does not start until it is
    /// (T-1349, the reviewer's second probe: it was up with no pane listed and the old process
    /// alive, its cleanup pending).
    #[cfg(unix)]
    #[test]
    fn a_window_in_its_shutdown_grace_is_gone_before_the_new_daemon_starts() {
        let _pty_guard = cf_panes::pty::serial_pty_test();
        let home = tempfile::tempdir().expect("home");
        let panes = Arc::new(PaneTable::closing_within(Duration::from_millis(1500)));
        let (kill, pid) = a_pane_that_ignores_the_request_to_end(&panes, &home.path().join("pid"));
        let starts = Arc::new(AtomicUsize::new(0));
        let alive_at_start = Arc::new(Mutex::new(Vec::new()));
        let (counted, table, seen) = (
            Arc::clone(&starts),
            Arc::clone(&panes),
            Arc::clone(&alive_at_start),
        );
        let starter: DaemonStarter = Arc::new(move |closed| {
            let start = counted.fetch_add(1, Ordering::SeqCst);
            seen.lock().expect("seen").push(process_exists(pid));
            if start > 0 {
                return stand_in_core(true, "while read -r line; do :; done", closed);
            }
            stand_in_over(
                &table,
                std::slice::from_ref(&kill),
                Then::StopAfter(300),
                closed,
            )
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::clone(&panes), QUICK);

        let mut told = Vec::new();
        for _ in 0..3 {
            told.push(heard.recv_timeout(Duration::from_secs(30)).expect("told"));
        }
        assert_eq!(
            told,
            [
                DaemonStatus::up(),
                DaemonStatus::down(DAEMON_STOPPED_RESTARTING, true),
                DaemonStatus::up(),
            ]
        );
        assert_eq!(
            *alive_at_start.lock().expect("seen"),
            [true, false],
            "the new daemon started beside a window of the old one that still ran"
        );
        let (child, bridge) = daemon.stop().expect("the app stops");
        if let Some(bridge) = bridge {
            bridge.close_input();
        }
        if let Some(mut child) = child {
            stop_daemon(&mut child);
        }
        wait_until_gone(pid);
    }

    /// When a window of the old daemon is not proven gone in the time a close takes, the daemon
    /// is not started again: the person is told so and asked to quit, which ends the app's
    /// processes. What they see: the banner for the restart, then, when the time is out, the
    /// daemon's stop with the quit it asks for.
    #[cfg(unix)]
    #[test]
    fn a_daemon_is_not_started_again_beside_a_window_that_is_not_proven_gone() {
        let _pty_guard = cf_panes::pty::serial_pty_test();
        let home = tempfile::tempdir().expect("home");
        // The program is given 4 s to end; the restart waits 300 ms for it.
        let panes = Arc::new(PaneTable::closing_within(Duration::from_secs(4)));
        let (kill, pid) = a_pane_that_ignores_the_request_to_end(&panes, &home.path().join("pid"));
        let starts = Arc::new(AtomicUsize::new(0));
        let (counted, table) = (Arc::clone(&starts), Arc::clone(&panes));
        let starter: DaemonStarter = Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            stand_in_over(
                &table,
                std::slice::from_ref(&kill),
                Then::StopAfter(300),
                closed,
            )
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        let backoff = Backoff {
            cleanup: Duration::from_millis(300),
            ..QUICK
        };
        daemon.start(starter, Arc::clone(&panes), backoff);

        let mut told = Vec::new();
        for _ in 0..3 {
            told.push(heard.recv_timeout(Duration::from_secs(30)).expect("told"));
        }
        assert_eq!(
            told,
            [
                DaemonStatus::up(),
                DaemonStatus::down(DAEMON_STOPPED_RESTARTING, true),
                DaemonStatus::down(DAEMON_NOT_CLEANED, false),
            ]
        );
        thread::sleep(Duration::from_millis(300));
        assert_eq!(starts.load(Ordering::SeqCst), 1, "started again");
        assert_eq!(
            daemon.connection().err().as_deref(),
            Some(DAEMON_NOT_CLEANED)
        );
        wait_until_gone(pid);
    }

    /// A daemon that stops again and again is given up on: the person is asked to quit only
    /// after `restarts` starts in a row ended within `stable` of their beginning, and the
    /// words say so.
    #[cfg(unix)]
    #[test]
    fn a_daemon_that_keeps_stopping_is_given_up_on_and_the_person_asked_to_quit() {
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: DaemonStarter = Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            stand_in_core(true, "sleep 0.1", closed)
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::new(PaneTable::new()), QUICK);

        let mut told = Vec::new();
        for _ in 0..8 {
            told.push(heard.recv_timeout(Duration::from_secs(10)).expect("told"));
        }
        let (up, restarting) = (
            DaemonStatus::up(),
            DaemonStatus::down(DAEMON_STOPPED_RESTARTING, true),
        );
        assert_eq!(
            told,
            [
                up.clone(),
                restarting.clone(),
                up.clone(),
                restarting.clone(),
                up.clone(),
                restarting,
                up,
                DaemonStatus::down(DAEMON_KEEPS_STOPPING, false),
            ]
        );
        thread::sleep(Duration::from_millis(300));
        assert_eq!(starts.load(Ordering::SeqCst), 4, "the first and three more");
        assert_eq!(
            daemon.connection().err().as_deref(),
            Some(DAEMON_KEEPS_STOPPING)
        );
    }

    /// A daemon that stayed up for `stable` has its stops begin the count again: a board that
    /// runs for days and has its daemon stop now and then is started again every time.
    #[cfg(unix)]
    #[test]
    fn a_daemon_that_stayed_up_is_not_counted_against_the_next_restart() {
        let backoff = Backoff {
            restarts: 1,
            stable: Duration::from_millis(100),
            ..QUICK
        };
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: DaemonStarter = Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            stand_in_core(true, "sleep 0.4", closed)
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::new(PaneTable::new()), backoff);

        // Three stops, each after more than `stable`: one restart allowed, and each is given.
        for _ in 0..6 {
            let told = heard.recv_timeout(Duration::from_secs(10)).expect("told");
            assert_ne!(told, DaemonStatus::down(DAEMON_KEEPS_STOPPING, false));
        }
        assert!(starts.load(Ordering::SeqCst) >= 3);
        let (child, bridge) = daemon.stop().expect("the app stops");
        if let Some(bridge) = bridge {
            bridge.close_input();
        }
        if let Some(mut child) = child {
            stop_daemon(&mut child);
        }
    }

    /// A runtime missing from the app does not come back: the page is told,
    /// and nothing starts again.
    #[test]
    fn a_missing_runtime_is_told_and_not_tried_again() {
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: DaemonStarter = Arc::new(move |_closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            Err(DaemonFailure {
                cause: "the bundled runtime is missing from this app".to_string(),
                retry: false,
            })
        });
        let (report, heard) = listener();
        Daemon::new(report).start(starter, Arc::new(PaneTable::new()), QUICK);

        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("told"),
            DaemonStatus::down("the bundled runtime is missing from this app", false)
        );
        thread::sleep(Duration::from_millis(100));
        assert_eq!(starts.load(Ordering::SeqCst), 1, "tried again");
    }

    /// A start is tried again only while no pane is open: one that is open
    /// may be a window of a daemon that got past its handle line.
    #[cfg(unix)]
    #[test]
    fn no_start_is_tried_again_while_a_pane_is_open() {
        let _pty_guard = cf_panes::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let opened = panes
            .open(
                Path::new("/tmp"),
                &[
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "sleep 30".to_string(),
                ],
                &HashMap::new(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            )
            .expect("open a pane");
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: DaemonStarter = Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            stand_in_core(false, "exit 1", closed)
        });
        let (report, heard) = listener();
        Daemon::new(report).start(starter, Arc::clone(&panes), QUICK);

        let cause = "ConsensFlow's daemon stopped before it was ready";
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("failed"),
            DaemonStatus::down(cause, true)
        );
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("gave up"),
            DaemonStatus::down(cause, false)
        );
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        panes.kill(&opened.key).expect("kill the pane");
    }

    /// A page that loads after the daemon failed hears it when it subscribes,
    /// in the shape the page reads.
    #[test]
    fn a_new_page_hears_where_the_daemon_stands() {
        use tauri::Listener;

        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("build mock app");
        let reporter = app.handle().clone();
        let daemon = Daemon::new(Arc::new(move |status: &DaemonStatus| {
            reporter
                .emit(DAEMON_STATUS_EVENT, status)
                .expect("emit the daemon's status");
        }));
        daemon.tell(DaemonStatus::down("the daemon is held up", true));
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        app.manage(AppRuntime {
            inputs: Arc::new(InputQueue::new(Arc::clone(&panes), arbiter)),
            panes,
            daemon,
            output: Arc::new(OutputHub::new()),
        });
        let (told, heard) = mpsc::channel();
        app.listen(DAEMON_STATUS_EVENT, move |event| {
            let _ = told.send(event.payload().to_string());
        });

        let subscribed = tauri::async_runtime::block_on(subscribe_output(
            app.handle().clone(),
            Channel::new(|_| Ok(())),
        ));
        assert_eq!(subscribed["ok"], true);
        let payload = heard
            .recv_timeout(Duration::from_secs(1))
            .expect("the page hears where the daemon stands");
        assert_eq!(
            serde_json::from_str::<Value>(&payload).expect("JSON"),
            json!({"available":false,"cause":"the daemon is held up","retrying":true})
        );
        drop(app);
    }

    #[test]
    fn roster_handle_accepts_only_loopback_http_and_normalizes_localhost() {
        let handle = RosterHandle::from_value(json!({
            "url":"http://127.0.0.1:43123/",
            "token":"secret",
        }))
        .unwrap();
        assert_eq!(handle.url, "http://localhost:43123/");
        assert!(RosterHandle::from_value(json!({
            "url":"https://example.com/",
            "token":"secret",
        }))
        .is_err());
    }

    /// On Windows the daemon's errors go to the app's error log, kept as the
    /// app keeps its own: a log past its limit is moved aside first. It used
    /// to grow without end.
    #[cfg(windows)]
    #[test]
    #[allow(clippy::disallowed_methods)] // The test points CONSENSFLOW_HOME at a folder of its own, and puts it back.
    fn the_daemons_errors_go_to_an_app_log_kept_like_the_others() {
        let home = tempfile::tempdir().expect("home");
        let log = home.path().join("app").join("app.log");
        std::fs::create_dir_all(home.path().join("app")).expect("the log's folder");
        std::fs::write(&log, vec![b'x'; 10 * 1024 * 1024 + 1]).expect("a full log");
        let configured = std::env::var_os("CONSENSFLOW_HOME");
        std::env::set_var("CONSENSFLOW_HOME", home.path());
        let stderr = daemon_stderr();
        match configured {
            Some(configured) => std::env::set_var("CONSENSFLOW_HOME", configured),
            None => std::env::remove_var("CONSENSFLOW_HOME"),
        }
        drop(stderr);
        assert!(
            home.path().join("app").join("app.log.1").exists(),
            "the full log was moved aside"
        );
        assert_eq!(
            std::fs::metadata(&log).expect("a fresh log").len(),
            0,
            "the daemon writes to a fresh log"
        );
    }
}
