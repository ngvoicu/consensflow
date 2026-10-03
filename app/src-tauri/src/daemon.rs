//! The daemon as the app holds it. It starts on a thread of its own
//! and again after a start that failed, stops with the app, and every change
//! is told to the page as `daemon-status`.

use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bridge::{Bridge, BridgeBuilder, BridgeError};
use crate::pty::PaneTable;

/// How long the daemon gets to stop on its own before it is killed.
pub(crate) const DAEMON_STOP_GRACE: Duration = Duration::from_secs(2);
/// What the page is told of the daemon (see `DaemonStatus`).
pub(crate) const DAEMON_STATUS_EVENT: &str = "daemon-status";
/// Between starts of a daemon that failed to start.
pub(crate) const DAEMON_RESTART: Backoff = Backoff {
    first: Duration::from_secs(1),
    most: Duration::from_secs(30),
};
const DAEMON_STOPPED: &str = "ConsensFlow's daemon stopped while the app was running";
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
/// the most.
#[derive(Clone, Copy)]
pub(crate) struct Backoff {
    first: Duration,
    most: Duration,
}

/// The daemon as the app holds it, and what the page knows of it.
///
/// A start that failed (the ledger still held by a daemon finishing its stop
/// after a force-quit, a migration that throws, a missing runtime) left the
/// app without its daemon for the session, the cause in a detail the page never
/// read; a daemon that died mid-session closed its bridge without a word and
/// the board froze. Now the page is told every change, and a failed start is
/// tried again: it ended before its handle line, so it left no window behind
/// it. A daemon that stops later is not started again, because its windows
/// still run in the pane host.
pub(crate) struct Daemon {
    state: Mutex<DaemonState>,
    /// Woken when the first start has its outcome, or the app stops.
    settled: Condvar,
    report: DaemonReport,
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
            }),
            settled: Condvar::new(),
            report,
        })
    }

    fn lock(&self) -> MutexGuard<'_, DaemonState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Starts the daemon from a thread of its own, so the app's window opens
    /// meanwhile: a daemon may take a while to start (a migration on a big
    /// ledger), and the first start used to run before the window existed.
    pub(crate) fn start(
        self: &Arc<Self>,
        starter: DaemonStarter,
        panes: Arc<PaneTable>,
        backoff: Backoff,
    ) {
        let daemon = Arc::clone(self);
        if let Err(error) = thread::Builder::new()
            .name("consensflow-daemon-start".to_string())
            .spawn(move || daemon.run(&starter, &panes, backoff))
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
        let status = if started.bridge.is_closed() {
            DaemonStatus::down(DAEMON_STOPPED, false)
        } else {
            DaemonStatus::up()
        };
        state.current = start;
        state.daemon = Some(started.daemon);
        state.bridge = Some(started.bridge);
        state.roster = Some(started.roster);
        self.change(&mut state, status);
        Ok(())
    }

    fn fail(&self, failure: &DaemonFailure) {
        eprintln!("consensflow: {}", failure.cause);
        self.tell(DaemonStatus::down(&failure.cause, failure.retry));
    }

    /// A start's bridge closed: if it is the daemon's, the daemon stopped.
    fn closed(&self, start: u64) {
        let mut state = self.lock();
        if state.stopping || state.current != start || !state.status.available {
            return;
        }
        eprintln!("consensflow: {DAEMON_STOPPED}");
        self.change(&mut state, DaemonStatus::down(DAEMON_STOPPED, false));
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

    pub(crate) fn roster(&self) -> Option<RosterHandle> {
        let state = self.lock();
        state
            .status
            .available
            .then(|| state.roster.clone())
            .flatten()
    }

    /// The app is stopping: no start is taken in, nothing more is told, and
    /// nothing waits for a first start. The first call has the daemon and its
    /// bridge handed over to be stopped.
    pub(crate) fn stop(&self) -> Option<(Option<Child>, Option<Bridge>)> {
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

/// Where the daemon's error output goes. On Windows a windowed app has no
/// stderr to hand down (inheriting an invalid handle fails the spawn), so the
/// daemon appends to the app's error log, the file the macOS build redirects
/// the app's own stderr to, kept the same way; elsewhere the daemon inherits
/// the app's.
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
    #[cfg(unix)]
    use std::collections::HashMap;
    #[cfg(unix)]
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[cfg(unix)]
    use portable_pty::PtySize;
    use serde_json::json;
    use tauri::ipc::Channel;
    use tauri::{Emitter, Manager};

    use crate::arbiter::{EnterTiming, InputArbiter};
    use crate::commands::subscribe_output;
    use crate::input_queue::InputQueue;
    use crate::output_hub::OutputHub;
    #[cfg(unix)]
    use crate::pty::process_exists;
    use crate::runtime::AppRuntime;

    /// Starts between failures, short enough for a test.
    const QUICK: Backoff = Backoff {
        first: Duration::from_millis(10),
        most: Duration::from_millis(40),
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
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(format!("{}{then}", if ready { handle } else { "" }));
        let mut builder = BridgeBuilder::new(1024);
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
            let mut command = Command::new("/bin/sh");
            command.arg("-c").arg(format!(
                "echo $$ > '{}'; exec /bin/sleep 60",
                hung.display()
            ));
            let mut builder = BridgeBuilder::new(1024);
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

        release.send(()).expect("let the start go on");
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("up"),
            DaemonStatus::up()
        );
        assert_eq!(answer.recv_timeout(Duration::from_secs(5)), Ok(true));

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
        assert!(answer.recv_timeout(Duration::from_millis(200)).is_err());

        assert!(daemon.stop().is_some());
        assert_eq!(
            answer
                .recv_timeout(Duration::from_secs(1))
                .expect("answered"),
            Some("ConsensFlow's daemon has not started".to_string())
        );
        drop(release);
    }

    /// A daemon that stops while the app runs is not started again (its
    /// windows still run in the pane host), and the page is told.
    #[cfg(unix)]
    #[test]
    fn a_daemon_that_stops_mid_session_is_told_and_not_started_again() {
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: DaemonStarter = Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            stand_in_core(true, "sleep 0.5", closed)
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::new(PaneTable::new()), QUICK);

        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("up"),
            DaemonStatus::up()
        );
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).expect("stopped"),
            DaemonStatus::down(DAEMON_STOPPED, false)
        );
        thread::sleep(Duration::from_millis(200));
        assert_eq!(starts.load(Ordering::SeqCst), 1, "started again");
        assert_eq!(daemon.connection().err().as_deref(), Some(DAEMON_STOPPED));
        assert!(daemon.roster().is_none());
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
        let _pty_guard = crate::pty::serial_pty_test();
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
    /// macOS app keeps it: a log past its limit is moved aside first. It used
    /// to grow without end.
    #[cfg(windows)]
    #[test]
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
