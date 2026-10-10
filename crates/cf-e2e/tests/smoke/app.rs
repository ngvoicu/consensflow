//! The packaged app, started as a person starts it, and what it says. The app
//! started in self-test mode (`CONSENSFLOW_SELFTEST=1`) drives its own page and
//! reports what the page saw on its own standard output, a line each, `consensflow-selftest
//! {"event": …, "data": …}`; this reads them as they come and waits for the one
//! a step needs. The app's own quit is the end of its input: its page says when
//! it has finished looking, the smoke does its probes while the app is still
//! up, and closing the pipe ends it through its ordinary exit.
//!
//! The app writes its error output to `<home>/app/app.log`, not to its own
//! standard error; a failed wait quotes both, and what the page said about
//! going wrong, so that the next reader does not have to start the app by hand.

use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use cf_e2e::process::{Run, Spawned};
use cf_e2e::{files, wire, Error, Result};
use serde_json::Value;

/// How a report starts the line it is on.
const REPORT: &str = "consensflow-selftest ";

/// What the page reports when it has gone wrong, and a probe it makes: the
/// reports a failed wait quotes.
const TROUBLE: [&str; 4] = ["page-error", "page-rejection", "failed", "probe"];

/// How much of the app's error output and of its log a failure shows.
const SHOWN: usize = 4000;

/// How often a wait looks again when nothing woke it.
const POLL: Duration = Duration::from_millis(250);

/// What the app has said so far.
#[derive(Default)]
struct Reports {
    events: Vec<Value>,
    /// Whether the app's output is over: nothing more will be reported.
    ended: bool,
}

/// What the reader of the app's output and a wait share.
#[derive(Default)]
struct Shared {
    reports: Mutex<Reports>,
    wake: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Reports> {
        self.reports.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&self, event: Value) {
        self.lock().events.push(event);
        self.wake.notify_all();
    }

    fn end(&self) {
        self.lock().ended = true;
        self.wake.notify_all();
    }
}

/// A running app, and what it has reported.
pub struct App {
    process: Spawned,
    shared: Arc<Shared>,
    patience: Duration,
    /// The machine's folder, for the failures that say where the evidence is.
    root: PathBuf,
    /// ConsensFlow's home, where the app's log is.
    state: PathBuf,
}

/// The report a line of the app's output is, if it is one. What is not (the app
/// says other things), and a report that is malformed, is left: a malformed one
/// is a failure of the step that wanted it, not of the reader, which goes on
/// draining so that the app never blocks on it.
fn report_of(line: &str) -> Option<Value> {
    serde_json::from_str(line.strip_prefix(REPORT)?).ok()
}

/// Whether a report says the app has given up: nothing it reports after
/// will be what a wait is for, and waiting out the rest of the time only
/// delays the same failure.
fn is_fatal(event: &Value) -> bool {
    match event["event"].as_str() {
        Some("failed" | "page-rejection") => true,
        // WebKit reports "ResizeObserver loop completed with undelivered
        // notifications" as a window error when the terminal refits inside a
        // dock that is still settling: a frame was skipped, nothing failed.
        Some("page-error") => !event["data"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("ResizeObserver loop"),
        _ => false,
    }
}

/// What the page said about going wrong, oldest first. The last `waiting` is
/// the picture of the page at the moment it gave up: the terminal's size, what
/// was on it, how many acknowledgements had flowed.
fn trouble(events: &[Value]) -> Vec<String> {
    let mut lines: Vec<String> = events
        .iter()
        .filter_map(|event| {
            let name = event["event"].as_str()?;
            TROUBLE
                .contains(&name)
                .then(|| format!("{name}: {}", event["data"]))
        })
        .collect();
    if let Some(waiting) = events
        .iter()
        .rev()
        .find(|event| event["event"] == "waiting")
    {
        lines.push(format!("last waiting: {}", waiting["data"]));
    }
    lines
}

/// The last `count` characters of `text`.
fn tail(text: &str, count: usize) -> &str {
    let skip = text.chars().count().saturating_sub(count);
    text.char_indices()
        .nth(skip)
        .map_or("", |(start, _)| &text[start..])
}

/// The app's log in the ConsensFlow home `state`, if it is there.
pub fn app_log(state: &Path) -> Option<String> {
    files::read_string(&state.join("app").join("app.log")).ok()
}

/// The programs the app's log says it started as its daemon, each as the log
/// names it: the app writes `starting the daemon: <cf> ui --json --no-open`
/// before it starts one, so that which `cf` it started is one line of its log.
pub fn daemons_started(log: &str) -> Vec<PathBuf> {
    const SAYS: &str = "starting the daemon: ";
    const ARGS: &str = " ui --json --no-open";
    log.lines()
        .filter_map(|line| {
            let said = line.split_once(SAYS)?.1;
            Some(PathBuf::from(said.strip_suffix(ARGS)?))
        })
        .collect()
}

impl App {
    /// Starts `run` as the app, whose folder is `root` and whose ConsensFlow
    /// home is `state`, and reads what it reports. A wait is given `patience`.
    pub fn start(run: Run, root: &Path, state: &Path, patience: Duration) -> Result<Self> {
        let mut process = run.spawn()?;
        let shared = Arc::<Shared>::default();
        if let Some(output) = process.take_output() {
            let (heard, ended) = (Arc::clone(&shared), Arc::clone(&shared));
            wire::read_each(
                output,
                move |line| {
                    if let Some(report) = report_of(&line) {
                        heard.push(report);
                    }
                    true
                },
                move || ended.end(),
            );
        }
        Ok(Self {
            process,
            shared,
            patience,
            root: root.to_path_buf(),
            state: state.to_path_buf(),
        })
    }

    /// The first report named `name` (`{event, data, pid}`), whenever it was
    /// made, waited for up to the patience it was given. A wait that the app
    /// has made certain to fail, by giving up or by ending, fails at once.
    pub fn wait_for(&self, name: &str) -> Result<Value> {
        let deadline = Instant::now() + self.patience;
        let mut reports = self.shared.lock();
        loop {
            if let Some(found) = reports.events.iter().find(|event| event["event"] == name) {
                return Ok(found.clone());
            }
            if reports.events.iter().any(is_fatal) {
                return Err(self.failed(
                    format!("the app gave up before reporting \"{name}\"."),
                    &reports.events,
                ));
            }
            if reports.ended {
                return Err(self.failed(
                    format!("the app's output ended before it reported \"{name}\"."),
                    &reports.events,
                ));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                // A timeout on its own says nothing useful: the page forwards
                // its own errors, give-ups and its view of the screen over the
                // same channel, and they are quoted.
                return Err(self.failed(
                    format!(
                        "the app never reported \"{name}\" within {} ms.",
                        self.patience.as_millis()
                    ),
                    &reports.events,
                ));
            }
            reports = self
                .shared
                .wake
                .wait_timeout(reports, left.min(POLL))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// The failure that says `headline`, what the app had reported, what the
    /// page said about going wrong, and where to look.
    fn failed(&self, headline: String, events: &[Value]) -> Error {
        let reported: Vec<&str> = events
            .iter()
            .map(|event| event["event"].as_str().unwrap_or_default())
            .collect();
        let reported = if reported.is_empty() {
            "(nothing)".to_owned()
        } else {
            reported.join(", ")
        };
        let mut said = format!("{headline}\nreported: {reported}\n");
        for line in trouble(events) {
            said.push_str(&line);
            said.push('\n');
        }
        let log = app_log(&self.state);
        said.push_str(&format!(
            "sandbox kept at {}\nstderr: {}\napp.log: {}",
            self.root.display(),
            tail(&self.process.errors(), SHOWN),
            log.as_deref().map_or("(none)", |log| tail(log, SHOWN)),
        ));
        Error::Timeout(said)
    }

    /// The app's own quit: the end of its input, and then its real exit.
    pub fn quit(&self) {
        self.process.end_input();
    }

    /// How the app ended, waited for up to the patience: none if it did not.
    pub fn exit(&mut self) -> Result<Option<ExitStatus>> {
        self.process.wait(self.patience)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app that is `script`, run by the shell (a script that has just been
    /// written may be refused as a file still open for writing).
    fn app(script: &str, patience: Duration) -> (tempfile::TempDir, App) {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let run = Run::new("/bin/sh").args(["-c", script]);
        let app = App::start(run, root.path(), &state, patience).unwrap();
        (root, app)
    }

    const SECONDS: Duration = Duration::from_secs(20);

    /// A line the page's report is.
    fn says(json: &str) -> String {
        format!("printf '%s\\n' 'consensflow-selftest {json}'\n")
    }

    #[test]
    fn a_report_is_read_by_its_name_whenever_it_came_and_what_is_not_one_is_left() {
        let script = [
            "echo 'the app says other things'\n".to_owned(),
            says(r#"{"event":"boot","data":{"protocol":"tauri:"},"pid":1}"#),
            "printf '%s\\n' 'consensflow-selftest {broken'\n".to_owned(),
            says(r#"{"event":"project","data":{"ok":true},"pid":1}"#),
            "read line\n".to_owned(),
        ]
        .concat();
        let (_root, app) = app(&script, SECONDS);
        let project = app.wait_for("project").unwrap();
        assert_eq!(project["data"]["ok"], true);
        // The earlier one, after the later one was waited for, and again.
        for _ in 0..2 {
            let boot = app.wait_for("boot").unwrap();
            assert_eq!(boot["data"]["protocol"], "tauri:");
            assert_eq!(boot["pid"], 1);
        }
    }

    #[test]
    fn the_app_quits_when_its_input_ends_and_says_how_it_ended() {
        let script = [
            "read line\n".to_owned(),
            says(r#"{"event":"quit","data":{"reason":"stdin-eof"}}"#),
            "exit 3\n".to_owned(),
        ]
        .concat();
        let (_root, mut app) = app(&script, SECONDS);
        app.quit();
        assert_eq!(app.wait_for("quit").unwrap()["data"]["reason"], "stdin-eof");
        let ended = app.exit().unwrap().unwrap();
        assert_eq!(ended.code(), Some(3));
    }

    #[test]
    fn an_app_that_does_not_end_is_none_after_the_patience() {
        let (_root, mut app) = app("read line\n", Duration::from_millis(200));
        assert!(app.exit().unwrap().is_none());
    }

    #[test]
    fn a_report_that_gives_up_ends_the_wait_with_what_the_page_said_and_a_resize_is_not_one() {
        let script = [
            // A frame skipped while the terminal refits: nothing failed.
            says(
                r#"{"event":"page-error","data":{"message":"ResizeObserver loop completed with undelivered notifications."}}"#,
            ),
            says(r#"{"event":"boot","data":{}}"#),
            says(r#"{"event":"waiting","data":{"what":"the chief window appeared","polls":30}}"#),
            says(r#"{"event":"failed","data":{"error":"project.open refused"}}"#),
            "read line\n".to_owned(),
        ]
        .concat();
        let (root, app) = app(&script, SECONDS);
        let started = Instant::now();
        // What was reported before the failure is still there to be had.
        assert!(app.wait_for("boot").is_ok());
        let said = app.wait_for("project").unwrap_err().to_string();
        assert!(started.elapsed() < Duration::from_secs(10), "waited it out");
        assert!(
            said.starts_with("the app gave up before reporting \"project\".\n"),
            "{said}"
        );
        assert!(
            said.contains("reported: page-error, boot, waiting, failed\n"),
            "{said}"
        );
        assert!(
            said.contains("page-error: {\"message\":\"ResizeObserver loop completed"),
            "{said}"
        );
        assert!(
            said.contains("failed: {\"error\":\"project.open refused\"}\n"),
            "{said}"
        );
        assert!(
            said.contains("last waiting: {\"what\":\"the chief window appeared\",\"polls\":30}\n"),
            "{said}"
        );
        assert!(
            said.contains(&format!("sandbox kept at {}\n", root.path().display())),
            "{said}"
        );
    }

    #[test]
    fn a_page_error_that_is_not_a_resize_and_a_rejection_give_up_too_and_a_probe_does_not() {
        for (event, fatal) in [
            (
                r#"{"event":"page-error","data":{"message":"x is not defined"}}"#,
                true,
            ),
            // Only the one WebKit says of a skipped frame is the benign one.
            (
                r#"{"event":"page-error","data":{"message":"ResizeObserver is not defined"}}"#,
                true,
            ),
            (r#"{"event":"page-error","data":{}}"#, true),
            (
                r#"{"event":"page-rejection","data":{"reason":"Error: no"}}"#,
                true,
            ),
            (r#"{"event":"failed","data":{}}"#, true),
            (r#"{"event":"probe","data":{}}"#, false),
            (r#"{"event":"waiting","data":{}}"#, false),
            (r#"{"event":"boot","data":{}}"#, false),
            (
                r#"{"event":"page-error","data":{"message":"ResizeObserver loop limit exceeded"}}"#,
                false,
            ),
        ] {
            let event: Value = serde_json::from_str(event).unwrap();
            assert_eq!(is_fatal(&event), fatal, "{event}");
        }
    }

    #[test]
    fn a_report_that_does_not_come_is_a_timeout_that_says_what_did_and_where_to_look() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        files::write(&state.join("app").join("app.log"), "the app's own words\n").unwrap();
        let script = [
            "echo 'what the app said to stderr' >&2\n".to_owned(),
            says(r#"{"event":"boot","data":{}}"#),
            says(r#"{"event":"probe","data":{"seen":"a window"}}"#),
            "read line\n".to_owned(),
        ]
        .concat();
        let run = Run::new("/bin/sh").args(["-c", script.as_str()]);
        let app = App::start(run, root.path(), &state, Duration::from_millis(400)).unwrap();
        let said = app.wait_for("project").unwrap_err().to_string();
        assert!(
            said.starts_with(
                "the app never reported \"project\" within 400 ms.\nreported: boot, probe\n"
            ),
            "{said}"
        );
        assert!(said.contains("probe: {\"seen\":\"a window\"}\n"), "{said}");
        assert!(
            said.contains("stderr: what the app said to stderr\n"),
            "{said}"
        );
        assert!(said.ends_with("app.log: the app's own words\n"), "{said}");
        // A wait for what was reported is no wait at all.
        assert!(app.wait_for("boot").is_ok());
    }

    #[test]
    fn an_app_that_reported_nothing_says_so_and_has_no_log() {
        let (_root, app) = app("read line\n", Duration::from_millis(200));
        let said = app.wait_for("boot").unwrap_err().to_string();
        assert!(said.contains("reported: (nothing)\n"), "{said}");
        assert!(said.ends_with("app.log: (none)"), "{said}");
    }

    #[test]
    fn an_app_whose_output_is_over_fails_the_wait_at_once_and_does_not_wait_out_the_patience() {
        let script = says(r#"{"event":"boot","data":{}}"#);
        let (_root, app) = app(&script, SECONDS);
        let started = Instant::now();
        let said = app.wait_for("project").unwrap_err().to_string();
        assert!(started.elapsed() < Duration::from_secs(10), "waited it out");
        assert!(
            said.starts_with("the app's output ended before it reported \"project\".\n"),
            "{said}"
        );
        assert!(said.contains("reported: boot\n"), "{said}");
    }

    #[test]
    fn what_a_failure_quotes_is_the_end_of_the_text_whole_characters_only() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("abc", 10), "abc");
        assert_eq!(tail("", 3), "");
        assert_eq!(tail("héllo wörld", 5), "wörld");
        assert_eq!(tail("héllo", 0), "");
        let long = "x".repeat(SHOWN + 10);
        assert_eq!(tail(&long, SHOWN).len(), SHOWN);
    }

    #[test]
    fn the_cf_the_app_started_as_its_daemon_is_the_one_its_log_names() {
        let log = "\
consensflow: starting the daemon: /Applications/ConsensFlow Candidate.app/Contents/Resources/cli/bin/cf ui --json --no-open
a line of the daemon's own
consensflow: starting the daemon: /elsewhere/cf ui --json --no-open
consensflow: starting the daemon: no arguments after it
";
        assert_eq!(
            daemons_started(log),
            [
                PathBuf::from(
                    "/Applications/ConsensFlow Candidate.app/Contents/Resources/cli/bin/cf"
                ),
                PathBuf::from("/elsewhere/cf"),
            ]
        );
        assert_eq!(
            daemons_started("nothing of the kind\n"),
            Vec::<PathBuf>::new()
        );
    }
}
