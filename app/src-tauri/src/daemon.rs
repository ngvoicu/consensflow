//! The daemon as the app holds it. It starts on a thread of its own
//! and again after a start that failed, stops with the app, and every change
//! is told to the page as `daemon-status`.

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::Child;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use cf_bridge::Bridge;
use cf_panes::headless::{reap_until_clean, Reaped};
use cf_panes::pty::PaneTable;
use serde_json::Value;

use crate::daemon_command::LoginLookup;
use crate::diag;
use crate::runtime::SHUTDOWN_REAP;

/// How long the daemon gets to stop on its own before it is killed.
pub(crate) const DAEMON_STOP_GRACE: Duration = Duration::from_secs(2);
/// What the daemon says, once, as it ends because its ledger cannot save any more
/// (`cf_daemon::broken`): `{"words": …}`, which the person is told as they are. The app
/// closes the windows and does not start that daemon again.
pub(crate) const LEDGER_BROKEN_EVENT: &str = "ledger.broken";
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
/// What the person is told while the daemon that stopped has its windows closed, before
/// anything of them is known to be closed.
const DAEMON_CLOSING_WINDOWS: &str = "ConsensFlow's daemon stopped; closing its windows…";
/// What the person is told once every window of the daemon that stopped is proven gone, while
/// the app starts the daemon again (the page adds that it is starting it).
const DAEMON_WINDOWS_CLOSED: &str = "ConsensFlow's daemon stopped and its windows are closed.";
/// What the person is told, once, when the daemon is up again: the new daemon has paused the
/// projects that were open (its start does it, before it says it is ready).
const DAEMON_STARTED_AGAIN: &str = "ConsensFlow's daemon was started again. Its windows were closed, and the projects that were open are paused until you resume them.";
/// What the person is told when the end of a window of the daemon that stopped could not be
/// confirmed in time: no other daemon is started beside what might still run.
const DAEMON_NOT_CONFIRMED: &str = "ConsensFlow's daemon stopped, and the end of its windows could not be confirmed, so it was not started again while one of them might still run.";
/// What the person is told when the app gave up: the restarts did not hold.
const DAEMON_KEEPS_STOPPING: &str = "ConsensFlow's daemon keeps stopping: it was started again three times, and each time it stopped within a minute";
/// How the daemon's process ends when the ledger cannot be saved to (`cf_base::home`).
pub(crate) const LEDGER_EXIT: i32 = cf_base::home::CANNOT_SAVE_EXIT;
/// How long a daemon has to say it is ready (its handle line) before its
/// start counts as failed. It opens its ledger first and migrates it, and
/// each step of a migration rebuilds a table and checks every reference in
/// the ledger under full sync: a limit too short would cut each start of a
/// big ledger short, undone every time, and the app would never start. A
/// daemon that hangs is still reported, and started again, within two
/// minutes.
pub(crate) const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(120);

mod connect;
mod status;

#[cfg(all(test, windows))]
use connect::daemon_stderr;
pub(crate) use connect::{connect_daemon, stop_daemon, RosterHandle};
pub(crate) use status::DaemonStatus;

/// Why a start of the daemon failed, and whether another start could fare
/// better: a runtime missing from the app does not come back.
pub(crate) struct DaemonFailure {
    pub(crate) cause: String,
    pub(crate) retry: bool,
    /// The cause is the whole message of a ledger that cannot be saved to: the daemon
    /// ended for it, with its status, before it was ready. It is told as it is, and the
    /// start is never tried again.
    pub(crate) ledger: bool,
}

impl DaemonFailure {
    fn transient(cause: String) -> Self {
        Self {
            cause,
            retry: true,
            ledger: false,
        }
    }
}

/// A daemon that started: its process, its bridge, and the address of the
/// agents screens it handed the app.
pub(crate) struct StartedDaemon {
    daemon: Child,
    bridge: Bridge,
    roster: RosterHandle,
    /// The ledger the process was launched with, said when it was launched: the words of a
    /// ledger that cannot save name this file, never one the app's own environment gives.
    ledger: PathBuf,
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
    /// A test's: the thread that closes the windows is not made.
    #[cfg(test)]
    no_cleanup_thread: std::sync::atomic::AtomicBool,
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
    /// What each daemon said as it ended because its ledger cannot save, by the process
    /// that said it (a frame can come before the daemon is in hand): the words are the
    /// person's, and only the daemon in hand gives them. What an older daemon says only adds
    /// a word nobody asks for, and displaces nothing.
    broken: HashMap<u32, String>,
    /// The process in hand, and the ledger it was launched with: only its words count.
    pid: Option<u32>,
    ledger: Option<PathBuf>,
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
                broken: HashMap::new(),
                pid: None,
                ledger: None,
            }),
            settled: Condvar::new(),
            report,
            login: Arc::default(),
            #[cfg(test)]
            no_cleanup_thread: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// A test makes the thread that closes the windows fail to start.
    #[cfg(test)]
    fn fail_the_cleanup_thread(&self) {
        self.no_cleanup_thread
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// What the daemon says as it ends because its ledger cannot save (the bridge's
    /// `ledger.broken` event): its words are kept for the stop that follows.
    pub(crate) fn hears_broken(self: &Arc<Self>) -> Arc<dyn Fn(Value) + Send + Sync> {
        let daemon = Arc::downgrade(self);
        Arc::new(move |body| {
            let Some(daemon) = daemon.upgrade() else {
                return;
            };
            let from = body
                .get("pid")
                .and_then(Value::as_u64)
                .and_then(|pid| u32::try_from(pid).ok());
            if let (Some(from), Some(words)) = (from, body.get("words").and_then(Value::as_str)) {
                // Kept by the process that says it, and asked for by the one in hand only: a
                // late frame of an older daemon changes nothing, whatever it says.
                let mut state = daemon.lock();
                if state.broken.len() >= 8 {
                    state.broken.clear();
                }
                state.broken.insert(from, words.to_owned());
            }
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
                        ledger: false,
                    });
                }
            })
        {
            self.fail(&DaemonFailure {
                cause: format!("ConsensFlow's daemon could not be started: {error}"),
                retry: false,
                ledger: false,
            });
        }
    }

    /// Starts the daemon, and again after a start that failed, waiting longer
    /// each time, while no pane is open and the app is not stopping.
    fn run(self: &Arc<Self>, starter: &DaemonStarter, panes: &PaneTable, backoff: Backoff) {
        let mut wait = backoff.first;
        loop {
            let Err(failure) = self.attempt(starter, false) else {
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
    fn attempt(
        self: &Arc<Self>,
        starter: &DaemonStarter,
        again: bool,
    ) -> Result<(), DaemonFailure> {
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
        state.pid = Some(started.daemon.id());
        state.ledger = Some(started.ledger);
        // Words of any other process are of no use to this one.
        let in_hand = state.pid;
        state.broken.retain(|pid, _| Some(*pid) == in_hand);
        state.bridge = Some(started.bridge);
        state.roster = Some(started.roster);
        state.up_since = Some(Instant::now());
        if ended {
            // The process is waited for, with the lock let go: its own status is read
            // before anything is decided.
            let mut child = started.daemon;
            drop(state);
            let exit = reaped(&mut child);
            let mut state = self.lock();
            state.daemon = Some(child);
            self.stopped(state, exit);
            return Ok(());
        }
        state.daemon = Some(started.daemon);
        // A daemon started again says what became of the windows and the projects once, when
        // it is up: its start has paused the projects that were open by then.
        let status = if again {
            DaemonStatus::started_again()
        } else {
            DaemonStatus::up()
        };
        self.change(&mut state, status);
        Ok(())
    }

    fn fail(&self, failure: &DaemonFailure) {
        diag::say(format!("consensflow: {}", failure.cause));
        let status = if failure.ledger {
            DaemonStatus::ledger(&failure.cause)
        } else {
            DaemonStatus::down(&failure.cause, failure.retry)
        };
        self.tell(status);
    }

    /// A start's bridge closed: if it is the daemon's, the daemon stopped. How its process
    /// ended is read first, before anything is decided: the status is the daemon's own word
    /// for why, and no frame it sent before it ended is needed to know it.
    fn closed(self: &Arc<Self>, start: u64) {
        let mut state = self.lock();
        if state.stopping || state.current != start || !state.status.available {
            return;
        }
        let mut child = state.daemon.take();
        drop(state);
        // Its end is waited for, not guessed: the child is reaped, never killed first and
        // never given a time, and what it ended with is read before anything is decided.
        let exit = child.as_mut().and_then(reaped);
        let mut state = self.lock();
        if state.stopping || state.current != start {
            drop(state);
            if let Some(mut child) = child {
                stop_daemon(&mut child);
            }
            return;
        }
        state.daemon = child;
        self.stopped(state, exit);
    }

    /// The daemon in hand stopped while the app runs: the person is told, once, and the
    /// app starts it again ([`Daemon::restart`]) unless it has stopped again and again
    /// ([`Backoff::restarts`]), when the person is asked to quit.
    fn stopped(self: &Arc<Self>, mut state: MutexGuard<'_, DaemonState>, exit: Option<i32>) {
        diag::say(format!("consensflow: {DAEMON_STOPPED}"));
        // A daemon that ended because its ledger cannot save is not started again: the
        // windows are closed, as for any stop, and the person is told why. Its status says
        // it; the words come from what it said, if it said them and this daemon is the one
        // that did, and from the app otherwise. A daemon that has given its handle ends with
        // that status for a ledger that broke, and for nothing else.
        let from = state.pid;
        let said = from.and_then(|pid| state.broken.remove(&pid));
        state.broken.clear();
        let ledger = state.ledger.clone().unwrap_or_default();
        let broken = (exit == Some(LEDGER_EXIT) || said.is_some())
            .then(|| said.unwrap_or_else(|| cf_base::home::cannot_save_words(&ledger)));
        let Some(restart) = state.restart.clone() else {
            let cause = broken.as_deref().unwrap_or(DAEMON_STOPPED);
            self.change(&mut state, DaemonStatus::down(cause, false));
            return;
        };
        if broken.is_none() {
            let stayed_up = state
                .up_since
                .is_some_and(|since| since.elapsed() >= restart.backoff.stable);
            state.restarts = if stayed_up { 1 } else { state.restarts + 1 };
            if state.restarts > restart.backoff.restarts {
                self.change(&mut state, DaemonStatus::down(DAEMON_KEEPS_STOPPING, false));
                return;
            }
        }
        // Said as it is: the windows are being closed, and nothing is closed yet.
        self.change(&mut state, DaemonStatus::closing_windows());
        let old = state.daemon.take();
        drop(state);
        // What the cleanup needs is kept where this can reach it if its thread is not made:
        // the windows are closed all the same, and what the person is told is the daemon's
        // reason, not this one's.
        let job = Arc::new(Mutex::new(Some((restart, old, broken))));
        let held = Arc::clone(&job);
        let daemon = Arc::clone(self);
        #[cfg(test)]
        let refuse = self
            .no_cleanup_thread
            .load(std::sync::atomic::Ordering::SeqCst);
        #[cfg(not(test))]
        let refuse = false;
        let spawned = if refuse {
            Err(std::io::Error::other("no thread"))
        } else {
            thread::Builder::new()
                .name("consensflow-daemon-restart".to_string())
                .spawn(move || {
                    let taken = held.lock().unwrap_or_else(|e| e.into_inner()).take();
                    if let Some((restart, old, broken)) = taken {
                        daemon.restart(&restart, old, broken);
                    }
                })
        };
        if let Err(error) = spawned {
            let taken = job.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some((restart, old, broken)) = taken {
                self.end_windows(&restart, old, broken, &format!("{error}"));
            }
        }
    }

    /// The thread that closes the windows and starts the daemon again was not made: the
    /// windows are closed here, and nothing is started. A daemon whose ledger cannot save
    /// is told as it is; any other is told it could not be started again.
    fn end_windows(
        self: &Arc<Self>,
        restart: &Restart,
        old: Option<Child>,
        broken: Option<String>,
        error: &str,
    ) {
        if let Some(mut old) = old {
            stop_daemon(&mut old);
        }
        let closed = reap_until_clean(&restart.panes, restart.backoff.cleanup);
        if let Some(words) = broken {
            self.ledger_ended(&words, &closed);
            return;
        }
        let mut cause = format!("ConsensFlow's daemon could not be started again: {error}");
        if !matches!(closed, Reaped::Clean) {
            cause = format!("{cause}. {DAEMON_NOT_CONFIRMED}");
        }
        self.fail(&DaemonFailure {
            cause,
            retry: false,
            ledger: false,
        });
    }

    /// The daemon ended because the ledger cannot save, and its windows were closed: the
    /// person is told why, and the warning that the windows' end could not be confirmed goes
    /// with it where that is so.
    fn ledger_ended(&self, words: &str, reaped: &Reaped) {
        let cause = match reaped {
            Reaped::Clean => words.to_owned(),
            Reaped::TooLate | Reaped::Unconfirmed => {
                diag::say(format!("consensflow: {DAEMON_NOT_CONFIRMED}"));
                format!("{words} {DAEMON_NOT_CONFIRMED}")
            }
        };
        diag::say(format!("consensflow: {cause}"));
        self.tell(DaemonStatus::ledger(&cause));
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
    fn restart(self: &Arc<Self>, restart: &Restart, old: Option<Child>, broken: Option<String>) {
        if let Some(mut old) = old {
            stop_daemon(&mut old);
        }
        let reaped = reap_until_clean(&restart.panes, restart.backoff.cleanup);
        // The ledger cannot save: nothing is started again, and the person is told why,
        // with the warning where the end of the windows is not confirmed.
        if let Some(words) = broken {
            self.ledger_ended(&words, &reaped);
            return;
        }
        match reaped {
            Reaped::Clean => {}
            Reaped::TooLate | Reaped::Unconfirmed => {
                // The end of the windows is not confirmed: that is what is said, and the
                // person is asked to quit.
                diag::say(format!("consensflow: {DAEMON_NOT_CONFIRMED}"));
                self.tell(DaemonStatus::not_confirmed());
                return;
            }
        }
        // Every window is proven gone: now it is true to say they are closed, and that the
        // daemon is being started.
        self.tell(DaemonStatus::windows_closed());
        let mut wait = restart.backoff.first;
        loop {
            if self.lock().stopping {
                return;
            }
            let Err(failure) = self.attempt(&restart.starter, true) else {
                return;
            };
            // A replacement the ledger refused (74) is not tried again and is not made an
            // ordinary failure: the person is told the ledger's words as they are.
            if failure.ledger {
                self.fail(&failure);
                return;
            }
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
                    ledger: false,
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

/// How a process ended, as its code: waited for until it is gone, and reaped. None where it
/// ended by a signal.
fn reaped(child: &mut Child) -> Option<i32> {
    child.wait().ok().and_then(|status| status.code())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    use cf_bridge::BridgeBuilder;
    use cf_proto::bridge::Role;
    use serde_json::Value;
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
        for _ in 0..4 {
            told.push(heard.recv_timeout(Duration::from_secs(25)).expect("told"));
        }
        assert_eq!(
            told,
            [
                DaemonStatus::up(),
                DaemonStatus::closing_windows(),
                DaemonStatus::windows_closed(),
                DaemonStatus::started_again(),
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

    /// A stand-in daemon that ends as one whose ledger cannot save does: it says `frames`
    /// (`{pid}` is its own number), and ends with `then`. What it says is heard by `hears`.
    fn a_ledger_stand_in(
        panes: &Arc<PaneTable>,
        starts: &Arc<AtomicUsize>,
        hears: &Arc<dyn Fn(Value) + Send + Sync>,
        frames: &[String],
        then: Then,
    ) -> DaemonStarter {
        let (table, counted, hears, frames) = (
            Arc::clone(panes),
            Arc::clone(starts),
            Arc::clone(hears),
            frames.to_vec(),
        );
        Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
            let inputs = Arc::new(InputQueue::new(Arc::clone(&table), Arc::clone(&arbiter)));
            let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
            crate::runtime::register_handlers(
                &mut builder,
                Arc::clone(&table),
                arbiter,
                Arc::new(OutputHub::new()),
                inputs,
            );
            let hear = Arc::clone(&hears);
            builder.on_event(LEDGER_BROKEN_EVENT, move |body| hear(body));
            builder.on_close(closed);
            connect_daemon(
                stand_in_command(&frames, then),
                builder,
                Duration::from_secs(30),
            )
        })
    }

    fn the_words_frame(words: &str) -> String {
        format!(
            r#"{{"v":1,"id":"n-1","kind":"evt","op":"ledger.broken","body":{{"words":"{words}","pid":{{pid}}}}}}"#
        )
    }

    const SAID: &str =
        "ConsensFlow cannot save to its ledger, /h/consensflow.db, so it has stopped.";

    fn told_of(heard: &mpsc::Receiver<DaemonStatus>, count: usize) -> Vec<DaemonStatus> {
        (0..count)
            .map(|_| heard.recv_timeout(Duration::from_secs(25)).expect("told"))
            .collect()
    }

    /// A daemon that ends for a ledger it cannot save to has its windows closed as for any
    /// stop, and the person is told its words as they are; it is not started again, however
    /// often it was started before (CLN-65).
    #[test]
    fn a_daemon_whose_ledger_cannot_save_is_not_started_again_and_the_person_is_told_why() {
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
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        let starter = a_ledger_stand_in(
            &panes,
            &starts,
            &daemon.hears_broken(),
            &[the_words_frame(SAID)],
            Then::ExitAfter(300, LEDGER_EXIT),
        );
        daemon.start(starter, Arc::clone(&panes), QUICK);

        assert_eq!(
            told_of(&heard, 3),
            [
                DaemonStatus::up(),
                DaemonStatus::closing_windows(),
                DaemonStatus::ledger(SAID),
            ]
        );
        assert!(
            heard.recv_timeout(Duration::from_millis(800)).is_err(),
            "nothing more is told"
        );
        assert_eq!(starts.load(Ordering::SeqCst), 1, "not started again");
        assert_eq!(daemon.connection().err().as_deref(), Some(SAID));
        assert!(
            panes.list().expect("a list").is_empty(),
            "its windows are closed"
        );
    }

    /// The status of the process is the daemon's word for why: a frame that was lost to the
    /// end of the pipe changes nothing but whose words are shown.
    #[test]
    fn a_ledger_that_cannot_save_is_known_from_the_exit_status_when_its_frame_is_lost() {
        let panes = Arc::new(PaneTable::new());
        let starts = Arc::new(AtomicUsize::new(0));
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        let starter = a_ledger_stand_in(
            &panes,
            &starts,
            &daemon.hears_broken(),
            &[],
            Then::ExitAfter(300, LEDGER_EXIT),
        );
        daemon.start(starter, panes, QUICK);

        let told = told_of(&heard, 3);
        assert_eq!(told[2], DaemonStatus::ledger(&fallback_words()));
        assert!(heard.recv_timeout(Duration::from_millis(800)).is_err());
        assert_eq!(starts.load(Ordering::SeqCst), 1, "not started again");
    }

    /// An event that is heard after the close was decided on changes nothing: the status was
    /// read first.
    #[test]
    fn an_event_that_races_the_close_does_not_decide_it() {
        let panes = Arc::new(PaneTable::new());
        let starts = Arc::new(AtomicUsize::new(0));
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        let hears = daemon.hears_broken();
        let slow: Arc<dyn Fn(Value) + Send + Sync> = Arc::new(move |body| {
            thread::sleep(Duration::from_millis(1200));
            hears(body);
        });
        let starter = a_ledger_stand_in(
            &panes,
            &starts,
            &slow,
            &[the_words_frame(SAID)],
            Then::ExitAfter(100, LEDGER_EXIT),
        );
        daemon.start(starter, panes, QUICK);

        let told = told_of(&heard, 3);
        assert!(told[2].ledger, "{:?}", told[2]);
        assert!(heard.recv_timeout(Duration::from_millis(1800)).is_err());
        assert_eq!(starts.load(Ordering::SeqCst), 1, "not started again");
    }

    /// What an older daemon said names that daemon, not the one in hand: it takes no part in
    /// how this one ended.
    #[test]
    fn a_late_event_from_an_older_daemon_does_not_end_the_one_that_is_up() {
        let panes = Arc::new(PaneTable::new());
        let starts = Arc::new(AtomicUsize::new(0));
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        let hears = daemon.hears_broken();
        let counted = Arc::clone(&starts);
        let table = Arc::clone(&panes);
        let first = a_ledger_stand_in(&panes, &starts, &hears, &[], Then::StopAfter(1000));
        let again = a_ledger_stand_in(&panes, &starts, &hears, &[], Then::StayUp);
        let starter: DaemonStarter = Arc::new(move |closed| {
            let _ = &table;
            if counted.load(Ordering::SeqCst) == 0 {
                first(closed)
            } else {
                again(closed)
            }
        });
        daemon.start(starter, Arc::clone(&panes), QUICK);
        assert_eq!(told_of(&heard, 1), [DaemonStatus::up()]);
        // Heard while this daemon is up: another process's, by its number.
        hears(json!({ "words": "old", "pid": 1u32 }));
        assert_eq!(
            told_of(&heard, 3),
            [
                DaemonStatus::closing_windows(),
                DaemonStatus::windows_closed(),
                DaemonStatus::started_again(),
            ]
        );
        assert_eq!(
            starts.load(Ordering::SeqCst),
            2,
            "started again, as for any stop"
        );
        let (child, bridge) = daemon.stop().expect("the app stops");
        if let Some(bridge) = bridge {
            bridge.close_input();
        }
        if let Some(mut child) = child {
            stop_daemon(&mut child);
        }
    }

    /// The words of the daemon in hand are not displaced by a late frame of an older one.
    #[test]
    fn a_late_frame_of_an_older_daemon_does_not_displace_the_words_of_the_one_in_hand() {
        let panes = Arc::new(PaneTable::new());
        let starts = Arc::new(AtomicUsize::new(0));
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        let hears = daemon.hears_broken();
        let starter = a_ledger_stand_in(
            &panes,
            &starts,
            &hears,
            &[the_words_frame(SAID)],
            Then::ExitAfter(1500, LEDGER_EXIT),
        );
        daemon.start(starter, Arc::clone(&panes), QUICK);
        assert_eq!(told_of(&heard, 1), [DaemonStatus::up()]);
        thread::sleep(Duration::from_millis(500));
        // Its own words were heard by now; another process's come late, by its number.
        hears(json!({ "words": "an older daemon's", "pid": 1u32 }));
        let told = told_of(&heard, 2);
        assert_eq!(told[1], DaemonStatus::ledger(SAID), "{told:?}");
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    /// The thread that closes the windows is not made: they are closed all the same, and the
    /// person is told the daemon's reason.
    #[test]
    fn a_cleanup_thread_that_is_not_made_still_closes_the_windows_and_keeps_the_ledgers_reason() {
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
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.fail_the_cleanup_thread();
        let starter = a_ledger_stand_in(
            &panes,
            &starts,
            &daemon.hears_broken(),
            &[the_words_frame(SAID)],
            Then::ExitAfter(300, LEDGER_EXIT),
        );
        daemon.start(starter, Arc::clone(&panes), QUICK);

        assert_eq!(
            told_of(&heard, 3),
            [
                DaemonStatus::up(),
                DaemonStatus::closing_windows(),
                DaemonStatus::ledger(SAID),
            ]
        );
        assert!(
            panes.list().expect("a list").is_empty(),
            "the windows are closed"
        );
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    /// What the app says of a daemon that gave its handle and then ended for its ledger, and
    /// said nothing itself: for the ledger of the process's launch (no home of the command's
    /// own: the app's).
    fn fallback_words() -> String {
        cf_base::home::cannot_save_words(&crate::daemon_command::launch_ledger(
            &crate::testing::command("x"),
        ))
    }

    /// Whether `status` is the app's word for a daemon that ended for its ledger before its
    /// handle and said nothing.
    fn unsaid(status: &DaemonStatus) -> bool {
        status.ledger
            && status
                .cause
                .as_deref()
                .is_some_and(|said| said.contains("couldn't say why"))
    }

    /// A stand-in whose launch names `home`, as a login shell's home does.
    fn a_ledger_stand_in_at(
        starts: &Arc<AtomicUsize>,
        hears: &Arc<dyn Fn(Value) + Send + Sync>,
        frames: &[String],
        then: Then,
        home: &std::path::Path,
    ) -> DaemonStarter {
        let (counted, hears, frames, home) = (
            Arc::clone(starts),
            Arc::clone(hears),
            frames.to_vec(),
            home.to_path_buf(),
        );
        Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
            let hear = Arc::clone(&hears);
            builder.on_event(LEDGER_BROKEN_EVENT, move |body| hear(body));
            builder.on_close(closed);
            let mut command = stand_in_command(&frames, then);
            command.env("CONSENSFLOW_HOME", &home);
            connect_daemon(command, builder, Duration::from_secs(30))
        })
    }

    /// The stream ends before the process does, and the process ends for the ledger well after
    /// the time the app used to give it: it is read from its own end, and not started again,
    /// whether the end of the stream came before the daemon was in hand (a short wait) or after
    /// (a long one).
    #[test]
    fn a_stream_that_ends_before_the_process_does_is_read_from_the_process_s_own_end() {
        // (the stream ends after, the process ends after): a stream that ends before the daemon
        // is in hand, and one that ends after; a process that ends within the time the app
        // used to give it, and one that ends well after.
        for (closes, wait) in [(0, 700), (0, 1900), (400, 700), (400, 1900), (400, 3200)] {
            let panes = Arc::new(PaneTable::new());
            let starts = Arc::new(AtomicUsize::new(0));
            let (report, heard) = listener();
            let daemon = Daemon::new(report);
            let starter = a_ledger_stand_in(
                &panes,
                &starts,
                &daemon.hears_broken(),
                &[],
                Then::CloseThenExit(closes, wait, LEDGER_EXIT),
            );
            daemon.start(starter, panes, QUICK);
            // Told up first where the stream ended after the daemon was in hand, and not
            // where it had ended already: what matters is the last, and that none is a start.
            let mut told = Vec::new();
            while told
                .last()
                .is_none_or(|status: &DaemonStatus| !status.ledger)
            {
                told.push(heard.recv_timeout(Duration::from_secs(25)).expect("told"));
            }
            assert_eq!(
                told.last(),
                Some(&DaemonStatus::ledger(&fallback_words())),
                "{closes}/{wait}"
            );
            assert!(
                told.iter()
                    .all(|status| *status != DaemonStatus::started_again()),
                "{closes}/{wait}: {told:?}"
            );
            assert!(heard.recv_timeout(Duration::from_millis(500)).is_err());
            assert_eq!(
                starts.load(Ordering::SeqCst),
                1,
                "{closes}/{wait}: not started again"
            );
        }
    }

    /// A daemon that closes its stream before it is ready and ends for its ledger after: its
    /// status is waited for, and it is killed by nothing.
    #[test]
    fn a_daemon_whose_stream_ends_before_it_is_ready_is_waited_for_not_killed() {
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: DaemonStarter = Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            builder.on_close(closed);
            #[cfg(unix)]
            let command = {
                let mut command = crate::testing::command("/bin/sh");
                command
                    .arg("-c")
                    .arg(format!("exec 1>&-; sleep 0.9; exit {LEDGER_EXIT}"));
                command
            };
            #[cfg(windows)]
            let command = {
                let mut command = crate::testing::command("powershell.exe");
                command
                    .args(["-NoProfile", "-NonInteractive", "-Command"])
                    .arg(format!(
                        "{CLOSE_STDOUT}; Start-Sleep -Milliseconds 900; exit {LEDGER_EXIT}"
                    ));
                command
            };
            connect_daemon(command, builder, Duration::from_secs(30))
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::new(PaneTable::new()), QUICK);
        let told = told_of(&heard, 1);
        assert!(unsaid(&told[0]), "{told:?}");
        assert!(heard.recv_timeout(Duration::from_millis(1500)).is_err());
        assert_eq!(starts.load(Ordering::SeqCst), 1, "no retry");
    }

    /// Where the daemon's words were lost, the file named is the one its process was launched
    /// with, whatever the app's own environment says.
    #[test]
    fn the_words_name_the_ledger_the_child_was_launched_with() {
        let launched = tempfile::tempdir().expect("a folder");
        let starts = Arc::new(AtomicUsize::new(0));
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        let starter = a_ledger_stand_in_at(
            &starts,
            &daemon.hears_broken(),
            &[],
            Then::ExitAfter(200, LEDGER_EXIT),
            launched.path(),
        );
        daemon.start(starter, Arc::new(PaneTable::new()), QUICK);
        let told = told_of(&heard, 3);
        let ledger = launched.path().join("consensflow.db");
        assert!(told[2].ledger, "{:?}", told[2]);
        let said = told[2].cause.clone().unwrap_or_default();
        assert!(said.contains(&ledger.display().to_string()), "{said}");
        assert_eq!(said, cf_base::home::cannot_save_words(&ledger));
        assert_ne!(said, fallback_words(), "not the app's own");
    }

    /// A daemon that stopped, and whose replacement the ledger refuses (74), keeps the ledger's
    /// flag and its words, and is not started a third time.
    #[test]
    fn a_replacement_refused_for_the_ledger_keeps_the_ledgers_words_and_flag() {
        let panes = Arc::new(PaneTable::new());
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let first = a_ledger_stand_in(
            &panes,
            &Arc::new(AtomicUsize::new(0)),
            &Daemon::new(Arc::new(|_: &DaemonStatus| {})).hears_broken(),
            &[],
            Then::StopAfter(300),
        );
        let starter: DaemonStarter = Arc::new(move |closed| {
            if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                return first(closed);
            }
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            builder.on_close(closed);
            connect_daemon(an_exit_with(LEDGER_EXIT), builder, Duration::from_secs(30))
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::clone(&panes), QUICK);
        let told = told_of(&heard, 4);
        assert_eq!(told[0], DaemonStatus::up());
        assert!(unsaid(&told[3]), "{told:?}");
        assert!(heard.recv_timeout(Duration::from_millis(1200)).is_err());
        assert_eq!(starts.load(Ordering::SeqCst), 2, "no third start");
    }

    /// No thread can be made, for the cleanup or for the closing of a window: the window is
    /// closed on the thread that asks, and answers nothing after the stop is declared.
    #[test]
    fn with_no_thread_to_make_the_windows_are_closed_on_the_current_one() {
        let _pty_guard = cf_panes::pty::serial_pty_test();
        struct Allowed;
        impl Drop for Allowed {
            fn drop(&mut self) {
                cf_panes::headless::refuse_threads(false);
                cf_bridge::refuse_threads(false);
            }
        }
        let _allowed = Allowed;
        let panes = Arc::new(PaneTable::new());
        let (argv, folder) = a_program_that_runs_on();
        for _ in 0..2 {
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
        }
        let starts = Arc::new(AtomicUsize::new(0));
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.fail_the_cleanup_thread();
        cf_panes::headless::refuse_threads(true);
        // The bridge's own threads for what it delivers (the daemon's last word among them).
        cf_bridge::refuse_threads(true);
        let starter = a_ledger_stand_in(
            &panes,
            &starts,
            &daemon.hears_broken(),
            &[the_words_frame(SAID)],
            Then::ExitAfter(300, LEDGER_EXIT),
        );
        daemon.start(starter, Arc::clone(&panes), QUICK);
        assert_eq!(
            told_of(&heard, 3),
            [
                DaemonStatus::up(),
                DaemonStatus::closing_windows(),
                DaemonStatus::ledger(SAID),
            ]
        );
        assert!(
            panes.list().expect("a list").is_empty(),
            "no window is left"
        );
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    /// The executable a `cargo build --message-format=json` says it made of the bin `cf`: where
    /// cargo put it, whatever `--target`, `--target-dir` or `CARGO_TARGET_DIR` the run uses.
    fn built_cf(output: &str) -> Option<PathBuf> {
        output.lines().rev().find_map(|line| {
            let message: serde_json::Value = serde_json::from_str(line).ok()?;
            let artifact = message["reason"] == "compiler-artifact"
                && message["target"]["name"] == "cf"
                && message["target"]["kind"]
                    .as_array()?
                    .iter()
                    .any(|kind| kind == "bin");
            artifact
                .then(|| message["executable"].as_str().map(PathBuf::from))
                .flatten()
        })
    }

    /// The `cf` of this checkout, built here: the app's tests link no binary of the daemon's.
    fn real_cf() -> PathBuf {
        let mut build = crate::testing::command(env!("CARGO"));
        build
            .args([
                "build",
                "--offline",
                "--locked",
                "--package",
                "cf",
                "--bin",
                "cf",
                "--message-format=json-render-diagnostics",
            ])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdout(Stdio::piped());
        if !cfg!(debug_assertions) {
            build.arg("--release");
        }
        let built = build.output().expect("cargo runs");
        assert!(built.status.success(), "cf did not build");
        built_cf(&String::from_utf8_lossy(&built.stdout)).expect("cargo says where it put cf")
    }

    /// Where cargo says it put `cf` is what is run, wherever that is: not a place derived from
    /// this test's own binary, which a `--target` or a `--target-dir` puts somewhere else.
    #[test]
    fn the_real_cf_is_where_cargo_says_it_put_it() {
        let said = [
            r#"{"reason":"compiler-artifact","target":{"name":"cf","kind":["bin"]},"executable":"/elsewhere/aarch64-apple-darwin/debug/cf"}"#,
            r#"{"reason":"compiler-artifact","target":{"name":"cf","kind":["lib"]},"executable":null}"#,
            r#"{"reason":"build-finished","success":true}"#,
        ]
        .join("\n");
        assert_eq!(
            built_cf(&said),
            Some(PathBuf::from("/elsewhere/aarch64-apple-darwin/debug/cf"))
        );
        assert_eq!(built_cf(r#"{"reason":"build-finished"}"#), None);
        let folder = std::env::current_exe().expect("this test's binary");
        let beside = folder.with_file_name(if cfg!(windows) { "cf.exe" } else { "cf" });
        assert!(
            real_cf().is_file(),
            "the cf cargo built is there (beside this binary: {})",
            beside.display()
        );
    }

    /// The real `cf ui --json --no-open` on the scratch home `home`, under the supervisor: what
    /// the person is told, and how many times a daemon was started.
    fn under_the_supervisor(home: &std::path::Path) -> (Vec<DaemonStatus>, usize) {
        let cf = real_cf();
        let starts = Arc::new(AtomicUsize::new(0));
        let (counted, at) = (Arc::clone(&starts), home.to_path_buf());
        let starter: DaemonStarter = Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
            builder.on_close(closed);
            let mut command = crate::testing::command(&cf);
            for (name, _) in cf_base::env::Env::from_process().iter() {
                if name.to_string_lossy().starts_with("CONSENSFLOW_") {
                    command.env_remove(name);
                }
            }
            command
                .args(["ui", "--json", "--no-open"])
                .env("CONSENSFLOW_HOME", &at)
                .env("HOME", &at)
                .env("USERPROFILE", &at);
            connect_daemon(command, builder, Duration::from_secs(60))
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::new(PaneTable::new()), QUICK);
        let told = told_of(&heard, 1);
        assert!(heard.recv_timeout(Duration::from_millis(1200)).is_err());
        (told, starts.load(Ordering::SeqCst))
    }

    /// A scratch home that has had a board, and its ledger's path.
    fn a_marked_home() -> (tempfile::TempDir, PathBuf) {
        let home = tempfile::tempdir().expect("a home");
        std::fs::write(home.path().join("consensflow.board"), "{\"board\":true}\n")
            .expect("a marker");
        let ledger = home.path().join("consensflow.db");
        (home, ledger)
    }

    /// Damage behind a valid SQLite header: the child is the one judge of the ledger, and the
    /// app shows its words.
    #[test]
    fn a_ledger_damaged_behind_its_header_is_told_in_the_childs_own_words() {
        let (home, ledger) = a_marked_home();
        let mut damaged = b"SQLite format 3\0".to_vec();
        damaged.resize(4096, 0xff);
        std::fs::write(&ledger, damaged).expect("a file");
        let (told, starts) = under_the_supervisor(home.path());
        assert_eq!(
            told,
            [DaemonStatus::ledger(&cf_base::home::lost_ledger_words(
                &ledger
            ))]
        );
        assert_eq!(starts, 1);
    }

    /// A FIFO at the ledger's name: the child refuses it at once, and the app, which opens no
    /// ledger, is not held by it.
    #[cfg(unix)]
    #[test]
    fn a_fifo_at_the_ledgers_name_is_refused_by_the_child_and_never_opened_by_the_app() {
        let (home, ledger) = a_marked_home();
        let made = crate::testing::command("/usr/bin/mkfifo")
            .arg(&ledger)
            .status()
            .expect("mkfifo runs");
        assert!(made.success());
        let (told, starts) = under_the_supervisor(home.path());
        assert_eq!(
            told,
            [DaemonStatus::ledger(&cf_base::home::lost_ledger_words(
                &ledger
            ))]
        );
        assert_eq!(starts, 1);
    }

    /// A stand-in that says `line` where the handle line goes (`{pid}` its own number) and ends
    /// with `code`.
    fn a_refusing_stand_in(line: &str, code: i32) -> Command {
        a_refusing_stand_in_that_lingers(line, code, 0)
    }

    /// [`a_refusing_stand_in`] that stays `linger_ms` more after it has said its line, as a
    /// daemon does that is still ending: a start that ended it first would read another status.
    fn a_refusing_stand_in_that_lingers(line: &str, code: i32, linger_ms: u64) -> Command {
        #[cfg(unix)]
        {
            let line = line.replace("{pid}", "'\"$$\"'");
            let mut command = crate::testing::command("/bin/sh");
            command.arg("-c").arg(format!(
                "printf '%s\\n' '{line}'; sleep {}; exit {code}",
                linger_ms as f64 / 1000.0
            ));
            command
        }
        #[cfg(windows)]
        {
            let line = line.replace('\'', "''").replace("{pid}", "' + $PID + '");
            let mut command = crate::testing::command("powershell.exe");
            command
                .args(["-NoProfile", "-NonInteractive", "-Command"])
                .arg(format!(
                    "[Console]::Out.WriteLine('{line}'); Start-Sleep -Milliseconds {linger_ms}; exit {code}"
                ));
            command
        }
    }

    /// The refusal line is used where its pid is the child's: another's is not, and the app says
    /// what it knows itself.
    #[test]
    fn a_refusal_line_is_the_childs_words_only_where_its_pid_is_the_childs() {
        for (pid, words) in [("{pid}", Some("said by the child")), ("999999999", None)] {
            let line =
                format!(r#"{{"refused":{{"exit":74,"pid":{pid},"words":"said by the child"}}}}"#);
            let starts = Arc::new(AtomicUsize::new(0));
            let counted = Arc::clone(&starts);
            let starter: DaemonStarter = Arc::new(move |closed| {
                counted.fetch_add(1, Ordering::SeqCst);
                let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
                builder.on_close(closed);
                connect_daemon(
                    a_refusing_stand_in(&line, LEDGER_EXIT),
                    builder,
                    Duration::from_secs(30),
                )
            });
            let (report, heard) = listener();
            let daemon = Daemon::new(report);
            daemon.start(starter, Arc::new(PaneTable::new()), QUICK);
            let told = told_of(&heard, 1);
            assert!(told[0].ledger, "{pid}: {told:?}");
            let said = told[0].cause.clone().unwrap_or_default();
            match words {
                Some(words) => assert_eq!(said, words, "{pid}"),
                None => assert!(said.contains("couldn't say why"), "{pid}: {said}"),
            }
            assert!(heard.recv_timeout(Duration::from_millis(1200)).is_err());
            assert_eq!(starts.load(Ordering::SeqCst), 1, "{pid}");
        }
    }

    /// A start that is refused is waited for, never ended first: its status is the answer. A
    /// child that says its refusal and goes on for a moment ends with 74 and is told as a
    /// ledger that cannot be saved to, and is started no more.
    #[test]
    fn a_refused_child_is_read_for_its_status_before_it_is_ended() {
        let line = r#"{"refused":{"exit":74,"pid":{pid},"words":"said by the child"}}"#;
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: DaemonStarter = Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
            builder.on_close(closed);
            connect_daemon(
                a_refusing_stand_in_that_lingers(line, LEDGER_EXIT, 1500),
                builder,
                Duration::from_secs(30),
            )
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::new(PaneTable::new()), QUICK);
        let told = told_of(&heard, 1);
        assert!(told[0].ledger, "{told:?}");
        assert_eq!(told[0].cause.as_deref(), Some("said by the child"));
        assert!(heard.recv_timeout(Duration::from_millis(1200)).is_err());
        assert_eq!(starts.load(Ordering::SeqCst), 1, "no second start");
    }

    /// The statements of `source`'s product code (its comments and its tests left out) that name a
    /// ledger and look at a file: a statement is what ends at a `;` or a brace, so a call split
    /// over lines is one.
    fn ledger_looks(source: &str) -> Vec<String> {
        const LOOKS: [&str; 10] = [
            "File::open",
            "OpenOptions",
            "metadata(",
            "fs::read",
            "read_to_string",
            "try_exists",
            ".exists()",
            ".is_file()",
            "read_exact",
            "unsaved_words",
        ];
        let product = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or_default();
        let code: String = product
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join(" ");
        code.split([';', '{', '}'])
            .map(|statement| statement.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|statement| {
                statement.contains("ledger") && LOOKS.iter().any(|look| statement.contains(look))
            })
            .collect()
    }

    /// The guard finds a look at a ledger however the call is laid out.
    #[test]
    fn the_guard_that_the_app_looks_at_no_ledger_finds_a_call_split_over_lines() {
        let split = "fn a() {\n    let found = std::fs::read(\n        ledger_path,\n    );\n}\n";
        assert_eq!(ledger_looks(split).len(), 1, "{:?}", ledger_looks(split));
        let apart = "fn a() {\n    let ledger = home.join(\"x\");\n    let found = std::fs::read(path);\n}\n";
        assert!(ledger_looks(apart).is_empty());
        let said = "// a ledger is read with std::fs::read(ledger)\nfn a() {}\n";
        assert!(ledger_looks(said).is_empty(), "a comment looks at nothing");
        let tests = "fn a() {}\n#[cfg(test)]\nmod tests {\n fn t() { std::fs::read(ledger); }\n}\n";
        assert!(
            ledger_looks(tests).is_empty(),
            "tests look at what they like"
        );
    }

    /// The app opens and measures no ledger (findings 14 and 15): the child is its one judge.
    /// The guard reads the product's sources of the daemon's supervision.
    #[test]
    fn the_app_opens_and_measures_no_ledger() {
        let sources = [
            ("daemon.rs", include_str!("daemon.rs")),
            ("daemon/connect.rs", include_str!("daemon/connect.rs")),
            ("daemon_command.rs", include_str!("daemon_command.rs")),
        ];
        let found: Vec<String> = sources
            .iter()
            .flat_map(|(name, source)| {
                ledger_looks(source)
                    .into_iter()
                    .map(move |statement| format!("{name}: {statement}"))
            })
            .collect();
        assert!(found.is_empty(), "the app looks at the ledger: {found:#?}");
    }

    /// A daemon that ends for its ledger before it is ready is not a start to try again.
    #[test]
    fn a_daemon_that_ends_for_its_ledger_before_it_is_ready_is_never_started_again() {
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let starter: DaemonStarter = Arc::new(move |closed| {
            counted.fetch_add(1, Ordering::SeqCst);
            let mut builder = BridgeBuilder::new(Role::Host, 1024);
            builder.on_close(closed);
            connect_daemon(an_exit_with(LEDGER_EXIT), builder, Duration::from_secs(30))
        });
        let (report, heard) = listener();
        let daemon = Daemon::new(report);
        daemon.start(starter, Arc::new(PaneTable::new()), QUICK);

        let told = told_of(&heard, 1);
        assert!(unsaid(&told[0]), "{told:?}");
        assert!(
            heard.recv_timeout(Duration::from_millis(1500)).is_err(),
            "no retry"
        );
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    /// A program that ends at once with `code`, on either system.
    fn an_exit_with(code: i32) -> Command {
        #[cfg(unix)]
        {
            let mut command = crate::testing::command("/bin/sh");
            command.arg("-c").arg(format!("exit {code}"));
            command
        }
        #[cfg(windows)]
        {
            let mut command = crate::testing::command("powershell.exe");
            command
                .args(["-NoProfile", "-NonInteractive", "-Command"])
                .arg(format!("exit {code}"));
            command
        }
    }

    /// PowerShell that closes the process's own standard output handle, which `[Console]::Out.Close()`
    /// does not (it leaves the native handle open until the process ends, so the reader sees no
    /// end of stream).
    #[cfg(windows)]
    const CLOSE_STDOUT: &str = "Add-Type -Name K -Namespace W -MemberDefinition '[DllImport(\"kernel32.dll\")] public static extern System.IntPtr GetStdHandle(int n); [DllImport(\"kernel32.dll\")] public static extern bool CloseHandle(System.IntPtr h);'; $null = [W.K]::CloseHandle([W.K]::GetStdHandle(-11))";

    /// What a stand-in daemon does once it has said its handle line and its frames.
    #[derive(Clone, Copy)]
    enum Then {
        /// Stops after this many milliseconds (its input is closed with it).
        StopAfter(u64),
        /// Reads its input to the end, and stops with it.
        StayUp,
        /// Ends after this many milliseconds with this status.
        ExitAfter(u64, i32),
        /// Closes its output after the first number of milliseconds (the app reads the end of
        /// the stream), and ends only after the second, with this status: the stream ends
        /// before the process does.
        CloseThenExit(u64, u64, i32),
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
                // `{pid}` is the number of the process that says it.
                let frame = frame.replace("{pid}", "'\"$$\"'");
                script.push_str(&format!("printf '%s\\n' '{frame}'; "));
            }
            script.push_str(&match then {
                Then::StopAfter(milliseconds) => format!("sleep {}", milliseconds as f64 / 1000.0),
                Then::StayUp => "while read -r line; do :; done".to_string(),
                Then::ExitAfter(milliseconds, code) => {
                    format!("sleep {}; exit {code}", milliseconds as f64 / 1000.0)
                }
                Then::CloseThenExit(closes, milliseconds, code) => {
                    format!(
                        "sleep {}; exec 1>&-; sleep {}; exit {code}",
                        closes as f64 / 1000.0,
                        milliseconds as f64 / 1000.0
                    )
                }
            });
            let mut command = crate::testing::command("/bin/sh");
            command.arg("-c").arg(script);
            command
        }
        #[cfg(windows)]
        {
            let said = |text: &str| {
                format!(
                    "$o.WriteLine('{}'); ",
                    text.replace('\'', "''").replace("{pid}", "' + $PID + '")
                )
            };
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
                Then::ExitAfter(milliseconds, code) => {
                    format!("Start-Sleep -Milliseconds {milliseconds}; exit {code}")
                }
                Then::CloseThenExit(closes, milliseconds, code) => format!(
                    "Start-Sleep -Milliseconds {closes}; {CLOSE_STDOUT}; Start-Sleep -Milliseconds {milliseconds}; exit {code}"
                ),
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
            DaemonStatus::closing_windows()
        );
        // The new daemon comes up while the launch is still held, and the launch is let go.
        assert_eq!(
            heard
                .recv_timeout(Duration::from_secs(30))
                .expect("windows closed"),
            DaemonStatus::windows_closed()
        );
        assert_eq!(
            heard
                .recv_timeout(Duration::from_secs(30))
                .expect("up again"),
            DaemonStatus::started_again()
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
        for _ in 0..4 {
            told.push(heard.recv_timeout(Duration::from_secs(25)).expect("told"));
        }
        assert_eq!(
            told,
            [
                DaemonStatus::up(),
                DaemonStatus::closing_windows(),
                DaemonStatus::windows_closed(),
                DaemonStatus::started_again(),
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
                DaemonStatus::closing_windows(),
                DaemonStatus::not_confirmed(),
            ]
        );
        thread::sleep(Duration::from_millis(300));
        assert_eq!(starts.load(Ordering::SeqCst), 1, "started again");
        assert_eq!(
            daemon.connection().err().as_deref(),
            Some(DAEMON_NOT_CONFIRMED)
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
        for _ in 0..11 {
            told.push(heard.recv_timeout(Duration::from_secs(20)).expect("told"));
        }
        // The first start, then three times: closing, closed, up again; then the quit.
        let mut every = vec![DaemonStatus::up()];
        for _ in 0..3 {
            every.extend([
                DaemonStatus::closing_windows(),
                DaemonStatus::windows_closed(),
                DaemonStatus::started_again(),
            ]);
        }
        every.push(DaemonStatus::down(DAEMON_KEEPS_STOPPING, false));
        assert_eq!(told, every);
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
        for _ in 0..10 {
            let told = heard.recv_timeout(Duration::from_secs(20)).expect("told");
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
                ledger: false,
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

    /// The page draws a restart from what it is told, in these words and these fields: the
    /// steps are said as they are true (T-1363), and the page adds that the app is starting the
    /// daemon only to a status that does not say `closing`.
    #[test]
    fn the_page_is_told_each_step_of_a_restart_in_the_words_it_draws() {
        let said = |status: DaemonStatus| serde_json::to_value(status).expect("JSON");
        assert_eq!(
            said(DaemonStatus::closing_windows()),
            json!({
                "available": false,
                "cause": "ConsensFlow's daemon stopped; closing its windows…",
                "retrying": true,
                "closing": true
            })
        );
        assert_eq!(
            said(DaemonStatus::windows_closed()),
            json!({
                "available": false,
                "cause": "ConsensFlow's daemon stopped and its windows are closed.",
                "retrying": true
            })
        );
        assert_eq!(
            said(DaemonStatus::started_again()),
            json!({
                "available": true,
                "cause": null,
                "retrying": false,
                "notice": "ConsensFlow's daemon was started again. Its windows were closed, and the projects that were open are paused until you resume them."
            })
        );
        assert_eq!(
            said(DaemonStatus::not_confirmed()),
            json!({
                "available": false,
                "cause": "ConsensFlow's daemon stopped, and the end of its windows could not be confirmed, so it was not started again while one of them might still run.",
                "retrying": false
            })
        );
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

        let prepared = crate::commands::pane_page_prepare(
            app.handle().clone(),
            "test-page".into(),
            "nonce".into(),
        );
        assert_eq!(prepared["ok"], true);
        assert_eq!(
            crate::commands::pane_page_activate(
                app.handle().clone(),
                "test-page".into(),
                "nonce".into(),
                prepared["epoch"].as_u64().expect("an epoch")
            )["ok"],
            true
        );
        let subscribed = tauri::async_runtime::block_on(subscribe_output(
            app.handle().clone(),
            "test-page".to_string(),
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
