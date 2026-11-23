//! The daemon's process: started and connected to, ended, and the folder its errors go to.

use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use cf_bridge::{BridgeBuilder, BridgeError};
use serde::Deserialize;
use serde_json::Value;

use super::{DaemonFailure, StartedDaemon, DAEMON_STOP_GRACE, LEDGER_EXIT};

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
        let ended = child.wait();
        // A daemon that ended for a ledger it cannot save to says so by its status: not a
        // start to try again, whatever else it failed to say.
        if ended.is_ok_and(|status| status.code() == Some(LEDGER_EXIT)) {
            return DaemonFailure {
                cause: crate::daemon::ledger_words(),
                retry: false,
                ledger: true,
            };
        }
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
pub(super) fn daemon_stderr() -> Stdio {
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
pub(super) fn daemon_stderr() -> Stdio {
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
