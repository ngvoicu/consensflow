//! The daemon as the app runs it, on a home of its own: its handle line, then
//! the bridge's JSON lines both ways. The test is the app's side of the bridge:
//! it asks what the page asks, and as the pane host it opens every window it
//! is asked to, though no window's program ever runs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::daemon::{Daemon, Home};
use cf_e2e::daemon_log::assert_started;
use cf_e2e::wire::{self, Pending};
use cf_e2e::{files, Error, Result};
use serde_json::{json, Value};

/// How long the daemon has to say it is ready, and to answer a request.
const WITHIN: Duration = Duration::from_secs(10);

/// How often what is waited for is looked at again.
const EVERY: Duration = Duration::from_millis(20);

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
        // Never run: the test opens no window.
        home.stand_in("claude")?;
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
            Error::Timeout(format!(
                "the daemon never answered {op}: {}",
                self.daemon.errors()
            ))
        })
    }

    /// Waits for `found` to give something, looking every 20 ms for up to ten
    /// seconds; `what` is what the daemon never did if it does not.
    pub fn until<T>(&self, what: &str, mut found: impl FnMut() -> Option<T>) -> Result<T> {
        let started = Instant::now();
        loop {
            if let Some(value) = found() {
                return Ok(value);
            }
            if started.elapsed() >= WITHIN {
                return Err(Error::Timeout(format!(
                    "the daemon never {what}: {}",
                    self.daemon.errors()
                )));
            }
            thread::sleep(EVERY);
        }
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
        self.output_closed
            .recv_timeout(WITHIN)
            .map_err(|_| Error::Timeout("the daemon's output was never given up".to_owned()))
    }
}

impl Drop for OverBridge {
    /// The daemon is asked to stop, as the app asks, before its home goes.
    fn drop(&mut self) {
        let _ = self.daemon.stop();
    }
}
