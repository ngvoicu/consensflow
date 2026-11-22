//! The event file in the home, `events.jsonl`: one JSON line per ledger event,
//! per change of a window's activity and per error nobody caught, appended as
//! it happens, for whoever watches the daemon from outside (a tail, a reviewer
//! reading along). The ledger's own log stays the record; this is its running
//! copy. A daemon a test starts with `CONSENSFLOW_TOOLING_API_CALLS=1` also writes an
//! `api.answered` line for each request a window's token answered ([`crate::api::calls`]): the
//! app never sets it. Append-only, one previous file kept once it passes its limit, as the
//! daemon's log does, and never a reason for the daemon to fail.
//! [`Trace::forget`] drops a deleted project's lines from both files: a project
//! deleted leaves no trace but the line that says it was.
//!
//! One thread writes the file. A line is queued where it is said and never
//! waits: a file that is held (another process's lock on it, a slow disk) holds
//! that thread and nothing of the daemon's. The thread appends in the order the
//! lines were sent, moves the file aside at its limit, and forgets a deleted
//! project's lines as a job in the same order, so a line sent before a forgetting
//! is never left behind it and one sent after it is never lost to it.
//!
//! The queue holds [`QUEUE`] ordinary lines. Past that a line is dropped and
//! counted, and the count is written as one `daemon.error` line once the queue
//! has drained. A forgetting, the line that says a project was deleted and the
//! count have room of their own, and are never dropped. A kill, or a stop whose
//! writing does not end within [`SETTLE`], can leave the last lines unwritten.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use cf_base::file::{append_rotating, aside, flush, rename_durably};
use cf_base::time::{iso, Clock, SystemClock};
use cf_ledger::Event;
use cf_proto::trace::{TraceLine, Traced};
use serde_json::Value;
use tokio::sync::oneshot;

/// How big the trace may get before it is moved aside: 5 MB.
pub const LIMIT: u64 = 5_000_000;

/// How many ordinary lines wait for the thread that writes the file: those
/// sent and not yet written, the one being written among them.
pub const QUEUE: usize = 4096;

/// How long a stop waits for the thread that writes the file.
pub const SETTLE: Duration = Duration::from_millis(250);

/// Where a replaced file is made durable: its bytes, then its rename. A
/// test's own fails one of them.
#[derive(Clone, Copy)]
pub(crate) struct Durable {
    pub(crate) flush: fn(&File) -> io::Result<()>,
    pub(crate) rename: fn(&Path, &Path) -> io::Result<()>,
}

impl Durable {
    const DISK: Self = Self {
        flush,
        rename: rename_durably,
    };
}

/// A piece of the writer's work.
enum Job {
    /// An ordinary line, which a full queue drops.
    Line(String),
    /// A line that is never dropped: the one that says a project was deleted.
    Notice(String),
    /// A deleted project's lines to drop, and who waits to hear it is done.
    Forget {
        project: i64,
        answer: oneshot::Sender<io::Result<()>>,
    },
}

#[derive(Default)]
struct State {
    jobs: VecDeque<Job>,
    /// Ordinary lines sent and not yet written, the one in hand included.
    ordinary: usize,
    /// Ordinary lines dropped since the count was last written.
    dropped: u64,
    /// The writer has a job in hand.
    busy: bool,
    /// The trace is gone: the writer ends once it has done what is left.
    closed: bool,
}

struct Shared {
    state: Mutex<State>,
    /// The writer waits here for work.
    work: Condvar,
    /// A stop waits here for the writer to be done.
    done: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn settled(&self, within: Duration) -> bool {
        let until = Instant::now() + within;
        let mut state = self.lock();
        while !(state.jobs.is_empty() && !state.busy && state.dropped == 0) {
            let Some(left) = until.checked_duration_since(Instant::now()) else {
                return false;
            };
            state = self
                .done
                .wait_timeout(state, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        true
    }
}

type SharedClock = Arc<Mutex<Box<dyn Clock + Send>>>;

/// The trace of a home.
pub struct Trace {
    file: PathBuf,
    clock: SharedClock,
    shared: Arc<Shared>,
}

impl Drop for Trace {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.work.notify_all();
    }
}

fn now(clock: &SharedClock) -> String {
    iso(clock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .now_ms())
}

impl Trace {
    /// The trace of `home`, `events.jsonl`, on the system's clock.
    pub fn new(home: &Path) -> Self {
        Self::with_clock(home.join("events.jsonl"), LIMIT, Box::new(SystemClock))
    }

    /// A trace in `file` that moves it aside past `limit` bytes and reads the
    /// time on `clock`, for the lines that carry none of their own.
    pub fn with_clock(file: PathBuf, limit: u64, clock: Box<dyn Clock + Send>) -> Self {
        Self::made(file, limit, clock, Durable::DISK)
    }

    pub(crate) fn made(
        file: PathBuf,
        limit: u64,
        clock: Box<dyn Clock + Send>,
        durable: Durable,
    ) -> Self {
        let clock: SharedClock = Arc::new(Mutex::new(clock));
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            work: Condvar::new(),
            done: Condvar::new(),
        });
        #[cfg(test)]
        registry()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(file.clone(), Arc::downgrade(&shared));
        let writer = Writer {
            file: file.clone(),
            limit,
            clock: Arc::clone(&clock),
            shared: Arc::clone(&shared),
            durable,
        };
        // The thread is the trace's own: it ends once the trace is dropped and
        // what was sent is written. A trace that cannot get one loses its lines.
        let _ = std::thread::Builder::new()
            .name("trace".to_owned())
            .spawn(move || writer.run());
        Self {
            file,
            clock,
            shared,
        }
    }

    /// Where the trace is.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Queues one line, as it is. It never waits: past [`QUEUE`] lines waiting
    /// it is counted and not kept.
    pub fn write(&self, line: &TraceLine) {
        let Ok(json) = serde_json::to_string(line) else {
            return;
        };
        let text = format!("{json}\n");
        let mut state = self.shared.lock();
        if matches!(line.what, Traced::ProjectDeleted(_)) {
            state.jobs.push_back(Job::Notice(text));
        } else if state.ordinary >= QUEUE {
            state.dropped += 1;
        } else {
            state.ordinary += 1;
            state.jobs.push_back(Job::Line(text));
        }
        drop(state);
        self.shared.work.notify_one();
    }

    /// An event the ledger logged, at the time it logged it.
    pub fn event(&self, event: &Event) {
        self.write(&TraceLine {
            at: event.at.clone(),
            what: Traced::Event {
                project: event.project,
                kind: event.kind.clone(),
                data: event.data.clone(),
            },
        });
    }

    /// A line of the tooling's own (`kind`, of `project`, with `data`) at the time now: what
    /// only a test switches on ([`crate::api::calls`]), in the shape of an event's line.
    pub fn tooling(&self, project: i64, kind: &str, data: Value) {
        self.write(&TraceLine {
            at: now(&self.clock),
            what: Traced::Event {
                project,
                kind: kind.to_owned(),
                data,
            },
        });
    }

    /// An error nobody caught (`daemon.error`): `reason` says what failed and
    /// what it said, and the line names no project.
    pub fn error(&self, reason: &str) {
        self.write(&TraceLine {
            at: now(&self.clock),
            what: Traced::DaemonError {
                reason: reason.to_owned(),
            },
        });
    }

    /// Drops every line of `project` from the trace and the file moved aside,
    /// after every line sent before this: the future ends with the writer's
    /// answer. A file with nothing of the project is not written at all, and
    /// one that is not there has nothing to forget; one that cannot be read or
    /// rewritten keeps its lines, and the first such failure is the answer, said
    /// with the file's name once the other file was done too. The answer is
    /// that it is done only once each replaced file is on the disk: the new
    /// file flushed, then its rename flushed through the folder.
    pub fn forget(&self, project: i64) -> impl std::future::Future<Output = io::Result<()>> {
        let (answer, answered) = oneshot::channel();
        self.shared
            .lock()
            .jobs
            .push_back(Job::Forget { project, answer });
        self.shared.work.notify_one();
        async move {
            answered
                .await
                .unwrap_or_else(|_| Err(io::Error::other("the trace's writer ended")))
        }
    }

    /// [`Trace::forget`] for a caller that is no task: waits on the thread.
    pub fn forget_blocking(&self, project: i64) -> io::Result<()> {
        let (answer, answered) = oneshot::channel();
        self.shared
            .lock()
            .jobs
            .push_back(Job::Forget { project, answer });
        self.shared.work.notify_one();
        answered
            .blocking_recv()
            .unwrap_or_else(|_| Err(io::Error::other("the trace's writer ended")))
    }

    /// Whether the writer wrote all that was sent before this, the count of
    /// what was dropped too, within `within`.
    pub fn settled(&self, within: Duration) -> bool {
        self.shared.settled(within)
    }
}

/// The writers of this process's traces by their file, so that a test that reads
/// a home's `events.jsonl` after an action can wait for its writer first.
#[cfg(test)]
fn registry() -> &'static Mutex<std::collections::HashMap<PathBuf, std::sync::Weak<Shared>>> {
    static WRITERS: std::sync::OnceLock<
        Mutex<std::collections::HashMap<PathBuf, std::sync::Weak<Shared>>>,
    > = std::sync::OnceLock::new();
    WRITERS.get_or_init(Mutex::default)
}

/// Waits until the writer of the trace in `home` has written all that was
/// sent: a test that reads `events.jsonl` after an action calls it first.
#[cfg(test)]
pub(crate) fn settle_in(home: &Path) {
    let shared = registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&home.join("events.jsonl"))
        .and_then(std::sync::Weak::upgrade);
    if let Some(shared) = shared {
        assert!(
            shared.settled(Duration::from_secs(10)),
            "the trace's writer is done"
        );
    }
}

/// The text of the trace in `home`, once its writer has written all that was
/// sent.
#[cfg(test)]
pub(crate) fn settled_trace(home: &Path) -> io::Result<String> {
    settle_in(home);
    fs::read_to_string(home.join("events.jsonl"))
}

/// The thread that owns the file.
struct Writer {
    file: PathBuf,
    limit: u64,
    clock: SharedClock,
    shared: Arc<Shared>,
    durable: Durable,
}

impl Writer {
    fn run(&self) {
        loop {
            let mut state = self.shared.lock();
            let job = loop {
                if let Some(job) = state.jobs.pop_front() {
                    break Some(job);
                }
                if state.dropped > 0 {
                    break None;
                }
                if state.closed {
                    self.shared.done.notify_all();
                    return;
                }
                self.shared.done.notify_all();
                state = self
                    .shared
                    .work
                    .wait(state)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            };
            state.busy = true;
            let counted = if job.is_none() {
                std::mem::take(&mut state.dropped)
            } else {
                0
            };
            drop(state);
            let ordinary = match job {
                Some(Job::Line(text)) => {
                    self.append(&text);
                    true
                }
                Some(Job::Notice(text)) => {
                    self.append(&text);
                    false
                }
                Some(Job::Forget { project, answer }) => {
                    // A caller that stopped waiting is no reason to stop.
                    let _ = answer.send(self.forget(project));
                    false
                }
                None => {
                    self.count(counted);
                    false
                }
            };
            let mut state = self.shared.lock();
            if ordinary {
                state.ordinary -= 1;
            }
            state.busy = false;
        }
    }

    fn append(&self, text: &str) {
        // The home may be read-only or gone mid-run; the ledger has the event.
        let _ = append_rotating(&self.file, text, self.limit);
    }

    /// The one line that says how many lines the full queue dropped.
    fn count(&self, dropped: u64) {
        let line = TraceLine {
            at: now(&self.clock),
            what: Traced::DaemonError {
                reason: format!(
                    "{dropped} trace {} dropped: events.jsonl was not written as fast as its lines came",
                    if dropped == 1 { "line was" } else { "lines were" }
                ),
            },
        };
        if let Ok(json) = serde_json::to_string(&line) {
            self.append(&format!("{json}\n"));
        }
    }

    fn forget(&self, project: i64) -> io::Result<()> {
        let mut failed = Ok(());
        for name in [self.file.clone(), aside(&self.file)] {
            match forget_in(&name, project, self.durable) {
                Err(error) if error.kind() != io::ErrorKind::NotFound && failed.is_ok() => {
                    failed = Err(io::Error::new(
                        error.kind(),
                        format!("{}: {error}", name.display()),
                    ));
                }
                _ => {}
            }
        }
        failed
    }
}

/// `name` without the lines of `project`, written beside it, flushed, and
/// renamed over it, the rename flushed through the folder.
fn forget_in(name: &Path, project: i64, durable: Durable) -> io::Result<()> {
    let text = String::from_utf8_lossy(&fs::read(name)?).into_owned();
    let lines: Vec<&str> = text.split('\n').filter(|line| !line.is_empty()).collect();
    let kept: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| !is_of(line, project))
        .collect();
    if kept.len() == lines.len() {
        return Ok(());
    }
    let mut temporary = name.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let mut file = File::create(&temporary)?;
    file.write_all(
        kept.iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>()
            .as_bytes(),
    )?;
    (durable.flush)(&file)?;
    drop(file);
    (durable.rename)(&temporary, name)
}

/// Whether a line of the trace names `project` (`JSON.parse(line).project ===
/// project`): a line that is no JSON, or no object, or names another, is not.
fn is_of(line: &str, project: i64) -> bool {
    let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    // The ids are whole numbers JavaScript read as doubles; so are these.
    #[allow(clippy::cast_precision_loss)]
    let wanted = project as f64;
    fields
        .get("project")
        .and_then(Value::as_f64)
        .is_some_and(|named| named == wanted)
}

/// What the engine writes down of what happens at a window and of a project
/// deleted.
impl cf_engine::seams::Trace for Trace {
    fn line(&self, line: TraceLine) {
        self.write(&line);
    }

    fn forget(&self, project: i64) -> cf_harness::contract::Work<'_, Result<(), String>> {
        let answer = Trace::forget(self, project);
        Box::pin(async move { answer.await.map_err(|failed| failed.to_string()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_proto::trace::WindowEvent;
    use serde_json::json;

    /// A clock that reads a moment of 2026-10-05 and moves a second a reading.
    struct Ticks(i64);

    impl Clock for Ticks {
        fn now_ms(&mut self) -> i64 {
            self.0 += 1000;
            self.0
        }
    }

    fn trace_in(dir: &Path, limit: u64) -> Trace {
        Trace::with_clock(
            dir.join("events.jsonl"),
            limit,
            Box::new(Ticks(1_791_194_400_000)),
        )
    }

    /// The lines of `file`, once the writer has written all it was sent.
    fn lines(trace: &Trace, file: &Path) -> Vec<String> {
        assert!(trace.settled(Duration::from_secs(10)), "the writer is done");
        fs::read_to_string(file)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn event(project: i64, kind: &str) -> Event {
        Event {
            at: "2026-10-05T09:59:00.000Z".to_owned(),
            project,
            kind: kind.to_owned(),
            data: json!({ "n": project }),
        }
    }

    #[test]
    fn a_ledger_event_goes_down_with_the_time_the_ledger_gave_it() {
        let dir = tempfile::tempdir().unwrap();
        let trace = trace_in(dir.path(), LIMIT);
        trace.event(&event(2, "task.created"));
        assert_eq!(
            lines(&trace, trace.file()),
            [
                r#"{"at":"2026-10-05T09:59:00.000Z","project":2,"kind":"task.created","data":{"n":2}}"#
            ]
        );
    }

    #[test]
    fn an_error_nobody_caught_goes_down_with_the_time_it_was_traced() {
        let dir = tempfile::tempdir().unwrap();
        let trace = trace_in(dir.path(), LIMIT);
        trace.error("a pass failed: boom");
        assert_eq!(
            lines(&trace, trace.file()),
            [
                r#"{"at":"2026-10-05T10:00:01.000Z","kind":"daemon.error","project":null,"reason":"a pass failed: boom"}"#
            ]
        );
    }

    #[test]
    fn the_engine_s_lines_are_written_as_they_come() {
        let dir = tempfile::tempdir().unwrap();
        let trace = trace_in(dir.path(), LIMIT);
        cf_engine::seams::Trace::line(
            &trace,
            TraceLine {
                at: "2026-10-05T10:00:00.000Z".to_owned(),
                what: Traced::Window {
                    project: Some(1),
                    participant: Some("chief".to_owned()),
                    event: WindowEvent::Activity {
                        state: "idle".to_owned(),
                        reason: None,
                    },
                },
            },
        );
        assert_eq!(
            lines(&trace, trace.file()),
            [
                r#"{"at":"2026-10-05T10:00:00.000Z","kind":"window.activity","project":1,"participant":"chief","state":"idle","reason":null}"#
            ]
        );
    }

    #[test]
    fn the_trace_is_moved_aside_past_its_limit_and_one_previous_file_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let trace = trace_in(dir.path(), 100);
        for project in 1..=6 {
            trace.event(&event(project, "project.created"));
        }
        let kept = lines(&trace, &aside(trace.file()));
        let current = lines(&trace, trace.file());
        assert!(!kept.is_empty() && !current.is_empty());
        assert!(!kept.iter().any(|line| line.contains("\"project\":1,")));
        assert!(current.last().unwrap().contains("\"project\":6,"));
    }

    #[test]
    fn the_trace_of_a_home_is_moved_aside_past_five_million_bytes_and_not_at_them() {
        let dir = tempfile::tempdir().unwrap();
        let trace = Trace::new(dir.path());
        fs::write(trace.file(), vec![b'x'; 5_000_000]).unwrap();
        trace.error("at the limit");
        assert!(trace.settled(Duration::from_secs(10)));
        assert!(
            !aside(trace.file()).exists(),
            "five million are not past it"
        );
        trace.error("past the limit");
        assert!(trace.settled(Duration::from_secs(10)));
        assert!(aside(trace.file()).exists(), "past it, it is moved aside");
    }

    #[test]
    fn forgetting_a_project_drops_its_lines_from_both_files_and_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let trace = trace_in(dir.path(), LIMIT);
        let previous = aside(trace.file());
        fs::write(
            &previous,
            "{\"at\":\"a\",\"project\":1,\"kind\":\"x\"}\nnot json\n{\"at\":\"b\",\"project\":2,\"kind\":\"x\"}\n",
        )
        .unwrap();
        trace.event(&event(1, "one"));
        trace.event(&event(2, "two"));
        trace.error("kept: it names no project");
        trace.forget_blocking(1).unwrap();
        assert_eq!(
            lines(&trace, &previous),
            ["not json", "{\"at\":\"b\",\"project\":2,\"kind\":\"x\"}"],
            "a line that is no JSON is kept"
        );
        let current = lines(&trace, trace.file());
        assert_eq!(current.len(), 2);
        assert!(current[0].contains("\"kind\":\"two\""));
        assert!(current[1].contains("daemon.error"));
        assert!(!trace.file().with_extension("jsonl.tmp").exists());
    }

    #[test]
    fn a_file_with_nothing_of_the_project_is_not_written_and_a_missing_one_is_no_failure() {
        let dir = tempfile::tempdir().unwrap();
        let trace = trace_in(dir.path(), LIMIT);
        trace.forget_blocking(9).unwrap();
        assert!(!trace.file().exists());
        trace.event(&event(2, "two"));
        assert!(trace.settled(Duration::from_secs(10)));
        // A rewrite renames another file over it, which is another file.
        let which = || cf_base::file::identity(&fs::File::open(trace.file()).unwrap()).unwrap();
        let before = which();
        trace.forget_blocking(9).unwrap();
        assert_eq!(which(), before);
        assert_eq!(lines(&trace, trace.file()).len(), 1);
        trace.forget_blocking(2).unwrap();
        assert_ne!(which(), before, "a file with a line of it is rewritten");
        assert!(lines(&trace, trace.file()).is_empty());
    }

    #[test]
    fn a_project_is_named_by_a_number_however_json_wrote_it() {
        assert!(is_of(r#"{"project":3}"#, 3));
        assert!(is_of(r#"{"project":3.0}"#, 3));
        assert!(!is_of(r#"{"project":"3"}"#, 3));
        assert!(!is_of(r#"{"project":null}"#, 3));
        assert!(!is_of(r#"{"kind":"daemon.error"}"#, 3));
        assert!(!is_of("[3]", 3));
        assert!(!is_of("", 3));
    }

    #[test]
    fn a_home_that_cannot_be_written_loses_the_line_and_not_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let trace = Trace::with_clock(
            dir.path().join("gone").join("events.jsonl"),
            LIMIT,
            Box::new(Ticks(0)),
        );
        trace.event(&event(1, "lost"));
        trace.error("lost");
        trace.forget_blocking(1).unwrap();
        assert!(!trace.file().exists());
    }

    #[test]
    fn a_file_that_cannot_be_read_keeps_its_lines_and_says_so_while_the_other_is_still_done() {
        let dir = tempfile::tempdir().unwrap();
        let trace = trace_in(dir.path(), LIMIT);
        let previous = aside(trace.file());
        fs::write(&previous, "{\"at\":\"a\",\"project\":1,\"kind\":\"x\"}\n").unwrap();
        // A folder where the trace should be: it cannot be read.
        fs::create_dir(trace.file()).unwrap();

        let failed = trace.forget_blocking(1).unwrap_err();

        assert!(failed.to_string().contains("events.jsonl"), "{failed}");
        assert!(
            lines(&trace, &previous).is_empty(),
            "the file that could be was done"
        );
    }

    #[test]
    fn a_file_that_cannot_be_rewritten_keeps_its_lines_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let trace = trace_in(dir.path(), LIMIT);
        trace.event(&event(1, "one"));
        // A folder where the new file is written beside it.
        fs::create_dir(dir.path().join("events.jsonl.tmp")).unwrap();

        let failed = trace.forget_blocking(1).unwrap_err();

        assert!(failed.to_string().contains("events.jsonl"), "{failed}");
        assert_eq!(
            lines(&trace, trace.file()).len(),
            1,
            "the deleted project's line stays"
        );
    }
}
