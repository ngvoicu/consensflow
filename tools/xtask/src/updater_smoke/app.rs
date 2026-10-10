//! The packaged app, started as the smoke starts it, and what it says: the
//! self-test's lines on its own stdout (`consensflow-selftest {…}`), which the
//! updater's page driver writes (app/ui/update-selftest.js).
//!
//! A pipe that is the app's input is destroyed with the process that was given
//! it, and the updater restarts the app as another process. So the app's input
//! is a FIFO this side keeps open: the restarted app inherits its read end, and
//! closing the write end is the quit that both read as the end of input.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use cf_base::env::Env;
use serde_json::{json, Value};

use super::evidence::tail;
use super::processes::{alive, kill_group, kill_tree, signal_of, Waits};
use super::sandbox::{make_fifo, open_fifo, Ends};
use super::say::Say;
use super::{Error, Result};
use crate::process::{self, Invocation};

/// What the page reports when something went wrong, which a case that wants one says so.
const FAILURES: [&str; 5] = [
    "update-failure",
    "page-error",
    "page-rejection",
    "failed",
    "deadline",
];

/// How a self-test report starts the line it is on.
const REPORT: &str = "consensflow-selftest ";

/// What the page tells the install's blocked step, in the one line it reads.
const CONTINUE: &[u8] = b"continue-updater\n";

/// What of the app's error stream a failure shows.
const STDERR_SHOWN: usize = 4000;

/// One report of the page: `{ event, pid, data }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Event(Value);

impl Event {
    /// The report's name: `update-boot`, `update-blocked`, `update-failure`, ...
    pub fn name(&self) -> &str {
        self.0
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// The pid the page says the app is, where it says a pid.
    pub fn pid(&self) -> Option<u32> {
        self.0
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok())
            .filter(|pid| *pid > 0)
    }

    /// What the report says under `key` in its data.
    pub fn data(&self, key: &str) -> Option<&Value> {
        self.0.get("data")?.get(key)
    }

    /// The report as the page wrote it.
    pub fn json(&self) -> String {
        self.0.to_string()
    }
}

/// What the app has said and done so far.
#[derive(Default)]
struct Reports {
    events: Vec<Event>,
    /// The failures the case did not expect.
    failures: Vec<Event>,
    stderr: String,
    /// The pids the app has been: the one that was started and each the page reported.
    pids: BTreeSet<u32>,
}

/// How the process that was started ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

impl From<ExitStatus> for Exit {
    fn from(status: ExitStatus) -> Self {
        Self {
            code: status.code(),
            signal: signal_of(&status),
        }
    }
}

struct Inner {
    reports: Arc<Mutex<Reports>>,
    child: Mutex<Option<Child>>,
    control: Mutex<Option<File>>,
    waits: Waits,
}

/// The app that was started: a handle that every copy of shares.
#[derive(Clone)]
pub struct App(Arc<Inner>);

/// What an app is started with.
pub struct Launch<'a> {
    pub binary: &'a Path,
    /// Its whole environment.
    pub env: &'a Env,
    pub cwd: &'a Path,
    /// The failure reports that the case is about: they are waited for like any
    /// report and fail nothing else.
    pub expected: &'a [&'a str],
    pub waits: Waits,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Reads the app's output to its end: each line that is a report is kept, and
/// said as it comes.
fn read_reports(stream: impl Read, reports: &Mutex<Reports>, expected: &[String], say: &Say) {
    let mut lines = BufReader::new(stream);
    let mut raw = Vec::new();
    loop {
        raw.clear();
        match lines.read_until(b'\n', &mut raw) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&raw);
        let line = text.strip_suffix('\n').unwrap_or(&text);
        let Some(report) = line.strip_prefix(REPORT) else {
            continue;
        };
        let mut kept = locked(reports);
        match serde_json::from_str::<Value>(report) {
            Ok(parsed) => {
                let event = Event(parsed);
                say.out(format!("updater probe {}", event.json()));
                kept.pids.extend(event.pid());
                if FAILURES.contains(&event.name())
                    && !expected.iter().any(|name| name == event.name())
                {
                    kept.failures.push(event.clone());
                }
                kept.events.push(event);
            }
            Err(_) => kept.failures.push(Event(
                json!({ "event": "malformed-report", "data": { "line": line } }),
            )),
        }
    }
}

/// Reads the app's error stream to its end.
fn read_errors(mut stream: impl Read, reports: &Mutex<Reports>) {
    let mut chunk = [0_u8; 4096];
    while let Ok(count) = stream.read(&mut chunk) {
        if count == 0 {
            return;
        }
        locked(reports)
            .stderr
            .push_str(&String::from_utf8_lossy(&chunk[..count]));
    }
}

/// Starts the app at `launch.binary` in `launch.cwd`, as its own process group, on a FIFO of its own for its input.
pub fn launch_app(launch: &Launch, say: &Say) -> Result<App> {
    let fifo = make_fifo(launch.cwd)?;
    let Ends { input, control } = open_fifo(&fifo)?;
    let invocation = Invocation::new(launch.binary, launch.cwd);
    let mut child = process::spawn(&invocation, launch.env, Stdio::from(input), true)?;
    let reports = Arc::new(Mutex::new(Reports::default()));
    locked(&reports).pids.insert(child.id());
    let expected: Vec<String> = launch.expected.iter().map(ToString::to_string).collect();
    if let Some(stdout) = child.stdout.take() {
        let (reports, say) = (Arc::clone(&reports), say.clone());
        thread::spawn(move || read_reports(stdout, &reports, &expected, &say));
    }
    if let Some(stderr) = child.stderr.take() {
        let reports = Arc::clone(&reports);
        thread::spawn(move || read_errors(stderr, &reports));
    }
    Ok(App(Arc::new(Inner {
        reports,
        child: Mutex::new(Some(child)),
        control: Mutex::new(Some(control)),
        waits: launch.waits,
    })))
}

impl App {
    /// The first report that `predicate` takes, or the failure the app reported meanwhile.
    pub fn wait_for(&self, label: &str, predicate: impl Fn(&Event) -> bool) -> Result<Event> {
        self.wait_for_unless(label, predicate, |_| None)
    }

    /// The first report that `predicate` takes, or the failure the app reported
    /// meanwhile. `unless` names a report that settles the wait the other way, and
    /// says what it means: the case that waits for a refusal is not left to time
    /// out when the app took the update.
    pub fn wait_for_unless(
        &self,
        label: &str,
        predicate: impl Fn(&Event) -> bool,
        unless: impl Fn(&Event) -> Option<&'static str>,
    ) -> Result<Event> {
        self.0.waits.until(label, || {
            let reports = locked(&self.0.reports);
            if let Some(found) = reports.events.iter().find(|event| predicate(event)) {
                return Ok(Some(found.clone()));
            }
            for event in &reports.events {
                if let Some(meaning) = unless(event) {
                    return Err(Error::new(format!("{meaning}: {}", event.json())));
                }
            }
            if let Some(failure) = reports.failures.last() {
                return Err(Error::new(format!(
                    "packaged app reported failure: {}\nstderr: {}",
                    failure.json(),
                    tail(&reports.stderr, STDERR_SHOWN)
                )));
            }
            Ok(None)
        })
    }

    /// Every report the app has made so far.
    pub fn events(&self) -> Vec<Event> {
        locked(&self.0.reports).events.clone()
    }

    /// Tells the page's blocked install to go on: the panes it was waiting for are closed.
    pub fn continue_updater(&self) -> Result {
        let mut control = locked(&self.0.control);
        let Some(file) = control.as_mut() else {
            return Err(Error::new("the app's input is closed: it was told to quit"));
        };
        file.write_all(CONTINUE)
            .and_then(|()| file.flush())
            .map_err(|cause| Error::new(format!("could not tell the app to go on: {cause}")))
    }

    /// The app's own quit: the end of its input, and its real exit.
    pub fn close_input(&self) {
        locked(&self.0.control).take();
    }

    /// Whether any process the app has been (the original and the restarted) still runs.
    pub fn any_alive(&self) -> bool {
        let pids: Vec<u32> = locked(&self.0.reports).pids.iter().copied().collect();
        pids.into_iter().any(alive)
    }

    /// How the process that was started ended, which is waited for.
    pub fn exited(&self) -> Result<Exit> {
        let mut child = locked(&self.0.child);
        let Some(mut started) = child.take() else {
            return Err(Error::new("the app's exit was asked for twice"));
        };
        started
            .wait()
            .map(Exit::from)
            .map_err(|cause| Error::new(format!("could not wait for the app: {cause}")))
    }

    /// Ends every process the app has been, its group and whatever is under it.
    pub fn kill_recorded(&self) {
        self.close_input();
        let pids: Vec<u32> = locked(&self.0.reports).pids.iter().copied().collect();
        for pid in pids {
            kill_group(pid);
            kill_tree(pid);
        }
    }
}

#[cfg(test)]
mod tests;
