//! The event file in the home, `events.jsonl` (`src/core/trace.js`): one JSON
//! line per ledger event, per change of a window's activity and per error
//! nobody caught, appended as it happens, for whoever watches the daemon from
//! outside (a tail, a reviewer reading along). The ledger's own log stays the
//! record; this is its running copy. Append-only, one previous file kept once
//! it passes its limit, as the daemon's log does, and never a reason for the
//! daemon to fail. [`Trace::forget`] drops a deleted project's lines from
//! both files: a project deleted leaves no trace but the line that says it
//! was.
//!
//! Each line is appended with the file opened afresh, as Node did, and
//! forgetting is done where it is asked, on the thread, so a line appended
//! meanwhile is never lost to a rewrite.

use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cf_base::file::{append_rotating, aside};
use cf_base::time::{iso, Clock, SystemClock};
use cf_ledger::Event;
use cf_proto::trace::{TraceLine, Traced};
use serde_json::Value;

/// How big the trace may get before it is moved aside: 5 MB.
pub const LIMIT: u64 = 5_000_000;

/// The trace of a home.
pub struct Trace {
    file: PathBuf,
    limit: u64,
    clock: RefCell<Box<dyn Clock>>,
}

impl Trace {
    /// The trace of `home`, `events.jsonl`, on the system's clock.
    pub fn new(home: &Path) -> Self {
        Self::with_clock(home.join("events.jsonl"), LIMIT, Box::new(SystemClock))
    }

    /// A trace in `file` that moves it aside past `limit` bytes and reads the
    /// time on `clock`, for the lines that carry none of their own.
    pub fn with_clock(file: PathBuf, limit: u64, clock: Box<dyn Clock>) -> Self {
        Self {
            file,
            limit,
            clock: RefCell::new(clock),
        }
    }

    /// Where the trace is.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Appends one line, as it is.
    pub fn write(&self, line: &TraceLine) {
        if let Ok(json) = serde_json::to_string(line) {
            // The home may be read-only or gone mid-run; the ledger has the event.
            let _ = append_rotating(&self.file, &format!("{json}\n"), self.limit);
        }
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

    /// An error nobody caught (`daemon.error`): `reason` says what failed and
    /// what it said, and the line names no project.
    pub fn error(&self, reason: &str) {
        let at = iso(self.clock.borrow_mut().now_ms());
        self.write(&TraceLine {
            at,
            what: Traced::DaemonError {
                reason: reason.to_owned(),
            },
        });
    }

    /// Drops every line of `project` from the trace and the file moved aside.
    /// A file with nothing of the project is not written at all, and one that
    /// cannot be read or rewritten has nothing to forget.
    pub fn forget(&self, project: i64) {
        for name in [self.file.clone(), aside(&self.file)] {
            let _ = forget_in(&name, project);
        }
    }
}

/// `name` without the lines of `project`, written beside it and renamed over it.
fn forget_in(name: &Path, project: i64) -> io::Result<()> {
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
    fs::write(
        &temporary,
        kept.iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>(),
    )?;
    fs::rename(&temporary, name)
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

    fn forget(&self, project: i64) {
        Trace::forget(self, project);
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

    fn lines(file: &Path) -> Vec<String> {
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
            lines(trace.file()),
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
            lines(trace.file()),
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
            lines(trace.file()),
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
        let kept = lines(&aside(trace.file()));
        let current = lines(trace.file());
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
        assert!(
            !aside(trace.file()).exists(),
            "five million are not past it"
        );
        trace.error("past the limit");
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
        trace.forget(1);
        assert_eq!(
            lines(&previous),
            ["not json", "{\"at\":\"b\",\"project\":2,\"kind\":\"x\"}"],
            "a line that is no JSON is kept"
        );
        let current = lines(trace.file());
        assert_eq!(current.len(), 2);
        assert!(current[0].contains("\"kind\":\"two\""));
        assert!(current[1].contains("daemon.error"));
        assert!(!trace.file().with_extension("jsonl.tmp").exists());
    }

    #[test]
    fn a_file_with_nothing_of_the_project_is_not_written_and_a_missing_one_is_no_failure() {
        let dir = tempfile::tempdir().unwrap();
        let trace = trace_in(dir.path(), LIMIT);
        trace.forget(9);
        assert!(!trace.file().exists());
        trace.event(&event(2, "two"));
        // A rewrite renames another file over it, which is another file.
        let which = || cf_base::file::identity(&fs::File::open(trace.file()).unwrap()).unwrap();
        let before = which();
        trace.forget(9);
        assert_eq!(which(), before);
        assert_eq!(lines(trace.file()).len(), 1);
        trace.forget(2);
        assert_ne!(which(), before, "a file with a line of it is rewritten");
        assert!(lines(trace.file()).is_empty());
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
        trace.forget(1);
        assert!(!trace.file().exists());
    }
}
