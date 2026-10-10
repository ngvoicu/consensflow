//! The daemon as the app runs it, on a home of its own: its handle line, then
//! the bridge's JSON lines both ways. The test is the app's side of the bridge:
//! it asks what the page asks, and as the pane host it opens every window it
//! is asked to, though no window's program ever runs.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::daemon::{Daemon, Home};
use cf_e2e::daemon_log::{assert_started, lines_of};
use cf_e2e::wire::{self, Pending};
use cf_e2e::{files, Error, Result};
use serde_json::{json, Value};

/// How long the daemon has to say it is ready, and to answer a request.
const WITHIN: Duration = Duration::from_secs(10);

/// How often what is waited for is looked at again.
const EVERY: Duration = Duration::from_millis(20);

/// How many of the last lines of the daemon's log a wait that gave up shows.
const LOG_TAIL: usize = 12;

/// The stand-in agent, a file that is there for the shim of the Claude the daemon
/// finds on Windows to name: it is never run, as the test is the pane host.
const AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The daemon over its bridge.
pub struct OverBridge {
    // The daemon stops before its home goes: it writes there until it exits.
    pub daemon: Daemon,
    pub home: Home,
    /// The handle line.
    pub handle: Value,
    frames: Arc<Mutex<Vec<Value>>>,
    pending: Pending,
    asked: AtomicU64,
    /// The id of the request whose answer is the last line the test reads.
    leave_after: Arc<Mutex<Option<String>>>,
    /// Told once the daemon's output is no longer read.
    output_closed: Receiver<()>,
}

/// The pane host's answers: a window opens at once, and goes when asked.
fn host(frame: &Value) -> Value {
    match frame["op"].as_str().unwrap_or_default() {
        "pane.open" => {
            json!({ "ok": true, "id": frame["body"]["id"], "generation": frame["body"]["generation"] })
        }
        "pane.kill" => json!({ "ok": true }),
        op => json!({ "ok": false, "error": format!("{op} is not part of this test") }),
    }
}

impl OverBridge {
    /// Starts a daemon on a home with `agents` in its roster, and a Claude to be
    /// found on `PATH`, and only it: no real harness is ever in reach.
    pub fn start(agents: &[Value]) -> Result<Self> {
        let home = Home::new()?;
        for folder in [home.consensflow(), home.workspace()] {
            files::make_dir(&folder)?;
        }
        // Never run: the test is the pane host, and answers each window's opening itself.
        home.window_stand_in("claude", Path::new(AGENT))?;
        files::write(
            &home.consensflow().join("agents.json"),
            json!({ "schemaVersion": 1, "agents": agents }).to_string(),
        )?;
        let mut daemon = home.daemon()?;
        let handle = daemon.handle(WITHIN)?;

        let frames = Arc::new(Mutex::new(Vec::new()));
        let pending = Pending::default();
        let leave_after = Arc::new(Mutex::new(None::<String>));
        let (closed, output_closed) = mpsc::channel();
        let sink = daemon.sink();
        {
            let (frames, pending, leave_after) = (
                Arc::clone(&frames),
                pending.clone(),
                Arc::clone(&leave_after),
            );
            daemon.read_each(
                move |line| {
                    let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                        return true;
                    };
                    let id = frame["id"].as_str().unwrap_or_default().to_owned();
                    frames
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(frame.clone());
                    if frame["kind"] == "res" {
                        pending.answer(&id, &frame["body"]);
                    }
                    if frame["kind"] == "req" {
                        if let Some(sink) = &sink {
                            let op = frame["op"].as_str().unwrap_or_default();
                            sink.send(wire::response(&id, op, &host(&frame)));
                        }
                    }
                    let last = leave_after.lock().unwrap_or_else(PoisonError::into_inner);
                    !(frame["kind"] == "res" && last.as_deref() == Some(id.as_str()))
                },
                move || {
                    let _ = closed.send(());
                },
            );
        }
        // The daemon that answers is the native one: its log's start line says so.
        assert_started(&home.log(), daemon.id())?;
        Ok(Self {
            daemon,
            home,
            handle,
            frames,
            pending,
            asked: AtomicU64::new(0),
            leave_after,
            output_closed,
        })
    }

    /// Every frame the daemon wrote after its handle, in order.
    pub fn frames(&self) -> Vec<Value> {
        self.frames
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The events of the page's bridge that say the board moved for `reason`.
    pub fn told(&self, reason: &str) -> Vec<Value> {
        self.frames()
            .into_iter()
            .filter(|frame| {
                frame["kind"] == "evt"
                    && frame["op"] == "state.changed"
                    && frame["body"]["reason"] == reason
            })
            .collect()
    }

    /// A request as the page makes it, answered over the bridge.
    pub fn request(&self, op: &str, body: Value) -> Result<Value> {
        let id = format!("r-test-{}", self.asked.fetch_add(1, Ordering::SeqCst) + 1);
        self.ask(&id, op, &body)
    }

    fn ask(&self, id: &str, op: &str, body: &Value) -> Result<Value> {
        let answer = self.pending.expect(id);
        self.daemon.send(&wire::request(id, op, body));
        answer.recv_timeout(WITHIN).map_err(|_| {
            self.pending.forget(id);
            Error::Timeout(format!("the daemon never answered {op}:\n{}", self.seen()))
        })
    }

    /// Waits for `found` to give something, looking every 20 ms for up to ten
    /// seconds; `what` is what the daemon never did if it does not, and the
    /// error says what the daemon did instead.
    pub fn until<T>(&self, what: &str, found: impl FnMut() -> Option<T>) -> Result<T> {
        self.wait(WITHIN, what, found)
    }

    fn wait<T>(
        &self,
        within: Duration,
        what: &str,
        mut found: impl FnMut() -> Option<T>,
    ) -> Result<T> {
        let started = Instant::now();
        loop {
            if let Some(value) = found() {
                return Ok(value);
            }
            if started.elapsed() >= within {
                return Err(Error::Timeout(format!(
                    "the daemon never {what}:\n{}",
                    self.seen()
                )));
            }
            thread::sleep(EVERY);
        }
    }

    /// What the daemon has done so far, as a wait that gave up says it: see
    /// [`said`].
    fn seen(&self) -> String {
        let opened: Vec<String> = self
            .frames()
            .iter()
            .filter(|frame| frame["kind"] == "req" && frame["op"] == "pane.open")
            .map(|frame| frame["body"]["id"].as_str().unwrap_or("?").to_owned())
            .collect();
        let log = lines_of(&self.home.log(), self.daemon.id());
        said(&opened, &log, &self.daemon.errors())
    }

    /// The app's end of the bridge breaks: nobody reads the daemon's output any
    /// more, so its next write finds out. The answer to one more ping is the
    /// last thing read (the reader is not left in the middle of a read when its
    /// stream closes).
    pub fn hang_up(&self) -> Result<()> {
        let id = format!("r-test-{}", self.asked.fetch_add(1, Ordering::SeqCst) + 1);
        *self
            .leave_after
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(id.clone());
        self.ask(&id, "ping", &json!({}))?;
        self.output_closed.recv_timeout(WITHIN).map_err(|_| {
            Error::Timeout(format!(
                "the daemon's output was never given up:\n{}",
                self.seen()
            ))
        })
    }
}

impl Drop for OverBridge {
    /// The daemon is asked to stop, as the app asks, before its home goes.
    fn drop(&mut self) {
        let _ = self.daemon.stop();
    }
}

/// What a daemon had done when a wait gave up on it, on lines of their own: the
/// windows it asked the pane host to open, the last [`LOG_TAIL`] lines of its
/// log and what it wrote to its error output. A daemon that cannot open a
/// window says why in its log and sends no frame, so the log is the part that
/// tells why a window never opened.
fn said(opened: &[String], log: &[String], errors: &str) -> String {
    let windows = if opened.is_empty() {
        "none".to_owned()
    } else {
        opened.join(", ")
    };
    let tail = &log[log.len().saturating_sub(LOG_TAIL)..];
    let mut lines = vec![
        format!("  windows opened: {windows}"),
        format!("  its log, the last {} of {} lines:", tail.len(), log.len()),
    ];
    lines.extend(tail.iter().map(|line| format!("    {line}")));
    lines.push("  its error output:".to_owned());
    match errors.trim_end() {
        "" => lines.push("    nothing".to_owned()),
        written => lines.extend(written.lines().map(|line| format!("    {line}"))),
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Outcome;

    #[test]
    fn a_wait_that_gave_up_says_the_windows_opened_the_end_of_the_log_and_the_error_output() {
        let log: Vec<String> = (1..=15).map(|line| format!("line {line}")).collect();
        let opened = ["p1-chief".to_owned(), "p1-worker".to_owned()];
        let tail = (4..=15)
            .map(|line| format!("    line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            said(&opened, &log, "it broke\nand again\n"),
            format!(
                "  windows opened: p1-chief, p1-worker\n  its log, the last 12 of 15 lines:\n{tail}\n  \
                 its error output:\n    it broke\n    and again"
            )
        );
    }

    #[test]
    fn a_wait_that_gave_up_on_a_daemon_that_did_nothing_says_so_in_each_part() {
        assert_eq!(
            said(&[], &[], ""),
            "  windows opened: none\n  its log, the last 0 of 0 lines:\n  its error output:\n    nothing"
        );
    }

    #[test]
    fn a_wait_that_gives_up_names_what_it_waited_for_and_what_the_daemon_did_meanwhile() -> Outcome
    {
        let d = OverBridge::start(&[])?;
        let Err(gave_up) = d.wait::<()>(Duration::from_millis(50), "opened the chief", || None)
        else {
            panic!("a wait for what never comes found it");
        };
        let message = gave_up.to_string();
        assert!(
            message.starts_with(
                "the daemon never opened the chief:\n  windows opened: none\n  its log, the last "
            ),
            "{message}"
        );
        // The log is the daemon's own: its start line, by its process id.
        let start = format!(" info start pid {} rust ", d.daemon.id());
        assert!(message.contains(&start), "{message}");
        // A window the daemon asked the host for is named.
        d.frames
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(json!({ "kind": "req", "op": "pane.open", "body": { "id": "p7-chief" } }));
        assert!(
            d.seen().starts_with("  windows opened: p7-chief\n"),
            "{}",
            d.seen()
        );
        Ok(())
    }
}
