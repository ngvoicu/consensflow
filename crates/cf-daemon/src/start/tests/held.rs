//! A held trace file holds nothing of the daemon's: not a timer, not a ping,
//! not a warning, not a project's delete. The trace's own lock file is taken on
//! a thread of the test's, as another process appending would take it.

use std::fs::OpenOptions;
use std::sync::mpsc;
use std::thread::JoinHandle;

use cf_base::file::lock_beside;
use cf_ledger::NewProject;

use super::*;
use crate::files::Log;

/// What a daemon that is held up shows: every one of these answers within it.
const WITHIN: Duration = Duration::from_secs(2);

/// The longest a test's hold lasts if the test never lets go: a daemon that
/// waits for it fails the test by time, and does not hang it.
const AT_MOST: Duration = Duration::from_secs(5);

/// The lock beside `file`, held on a thread of its own until dropped.
struct Held {
    release: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Held {
    fn on(file: &Path) -> Self {
        let lock = lock_beside(file);
        let (taken_tx, taken) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let beside = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(lock)
                .unwrap();
            beside.lock().unwrap();
            taken_tx.send(()).unwrap();
            let _ = wait.recv_timeout(AT_MOST);
            drop(beside);
        });
        taken.recv().unwrap();
        Self {
            release: Some(release),
            thread: Some(thread),
        }
    }

    /// Lets go, and waits until it has.
    fn release(&mut self) {
        drop(self.release.take());
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.release();
    }
}

/// A `ping` answered and a 50 ms timer fired: how long both took.
async fn ping_and_timer(app: &Bridge) -> Duration {
    let begun = Instant::now();
    assert_eq!(ask(app, "ping", json!({})).await, json!({ "ok": true }));
    tokio::time::sleep(Duration::from_millis(50)).await;
    begun.elapsed()
}

fn new_project(root: &Path) -> NewProject {
    NewProject::from_json(&json!({
        "directory": root.join("workspace").to_string_lossy(),
        "name": "app",
        "chief": {"harness": "claude-code"},
        "staff": [{"harness": "claude-code", "roles": ["worker"], "tier": "standard"}]
    }))
    .unwrap()
}

#[tokio::test]
async fn a_held_trace_file_holds_neither_a_timer_nor_a_ping() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let held = Held::on(&rig.home.join("events.jsonl"));
            let begun = Instant::now();
            // An event the ledger logs, which the trace is told.
            rig.daemon
                .parts
                .ledger
                .borrow_mut()
                .create_project(&new_project(root.path()))
                .unwrap();
            ping_and_timer(&rig.app).await;
            let took = begun.elapsed();
            drop(held);
            assert!(took < WITHIN, "the event, a ping and a timer took {took:?}");
        })
        .await;
}

#[tokio::test]
async fn a_held_trace_file_holds_neither_a_timer_nor_a_ping_nor_does_a_warning_meanwhile() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let held = Held::on(&rig.home.join("events.jsonl"));
            // Another thread of the daemon's process says a line to the trace, and
            // is behind the held file.
            let home = rig.home.clone();
            let writer = std::thread::spawn(move || {
                crate::files::Trace::new(&home).error("said behind the held file");
            });
            std::thread::sleep(Duration::from_millis(200));
            let begun = Instant::now();
            Log::new(&rig.home).warn("a warning meanwhile", None);
            let warned = begun.elapsed();
            let took = ping_and_timer(&rig.app).await;
            drop(held);
            writer.join().unwrap();
            assert!(warned < WITHIN, "the warning took {warned:?}");
            assert!(took < WITHIN, "a ping and a timer took {took:?}");
            let lines = log_lines(&rig.home);
            assert!(
                lines.iter().any(|line| line.contains("a warning meanwhile")),
                "{lines:?}"
            );
        })
        .await;
}

#[tokio::test]
async fn a_held_trace_file_holds_neither_a_timer_nor_a_ping_nor_does_a_projects_delete() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let id = rig
                .daemon
                .parts
                .ledger
                .borrow_mut()
                .create_project(&new_project(root.path()))
                .unwrap()
                .id;
            rig.daemon.parts.engine.close_project(id).await.unwrap();
            let mut held = Held::on(&rig.home.join("events.jsonl"));
            // The measurement begins before the line behind the held file, which the
            // delete's forgetting comes after: on a trace that is written where it is
            // said that line itself waits out the hold.
            let begun = Instant::now();
            rig.daemon
                .parts
                .ledger
                .borrow_mut()
                .create_project(&new_project(root.path()))
                .unwrap();
            let engine = Rc::clone(&rig.daemon.parts.engine);
            let deleting = tokio::task::spawn_local(async move { engine.delete_project(id).await });
            ping_and_timer(&rig.app).await;
            let took = begun.elapsed();
            assert!(
                took < WITHIN,
                "the line behind the held file, a ping and a timer took {took:?}"
            );
            assert!(!deleting.is_finished(), "the delete waits for the held file");
            held.release();
            let deleted = tokio::time::timeout(WITHIN, deleting)
                .await
                .expect("the delete ends once the file is let go")
                .unwrap()
                .unwrap();
            assert_eq!(deleted.id, id);
        })
        .await;
}

// What the trace's own thread does with what it is sent.

use std::sync::Mutex;

use cf_base::file::{aside, flush, flush_folder, rename_durably};
use cf_base::time::SystemClock;
use cf_ledger::{DeletedProject, Event};
use cf_proto::trace::{TraceLine, Traced};

use crate::files::{Durable, Trace, QUEUE, SETTLE};

fn trace_in(dir: &Path, limit: u64) -> Trace {
    Trace::with_clock(dir.join("events.jsonl"), limit, Box::new(SystemClock))
}

fn event(project: i64, kind: &str) -> Event {
    Event {
        at: "2026-10-05T09:59:00.000Z".to_owned(),
        project,
        kind: kind.to_owned(),
        data: json!({}),
    }
}

fn deleted(project: i64) -> TraceLine {
    TraceLine {
        at: "2026-10-05T10:00:00.000Z".to_owned(),
        what: Traced::ProjectDeleted(DeletedProject {
            id: project,
            name: "app".to_owned(),
            directory: "/w".to_owned(),
            created_at: "2026-10-05T09:00:00.000Z".to_owned(),
            members: 1,
            sessions: 0,
            tasks: 0,
            messages: 0,
        }),
    }
}

fn settled(trace: &Trace) {
    assert!(trace.settled(Duration::from_secs(20)), "the writer is done");
}

/// The kinds of the lines of a file, in order.
fn kinds(file: &Path) -> Vec<String> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let line: Value = serde_json::from_str(line).unwrap();
            line["kind"].as_str().unwrap().to_owned()
        })
        .collect()
}

#[tokio::test]
async fn lines_a_move_aside_and_a_forgetting_are_done_in_the_order_they_were_sent() {
    let dir = tempfile::tempdir().unwrap();
    let trace = trace_in(dir.path(), 1000);
    let mut sent = Vec::new();
    for n in 0..8 {
        trace.event(&event(1, &format!("before{n}")));
    }
    for n in 0..2 {
        trace.event(&event(2, &format!("kept_before{n}")));
        sent.push(format!("kept_before{n}"));
    }
    let forgetting = trace.forget(1, None);
    for n in 0..12 {
        trace.event(&event(1, &format!("after{n}")));
        sent.push(format!("after{n}"));
    }
    for n in 0..4 {
        trace.event(&event(2, &format!("kept{n}")));
        sent.push(format!("kept{n}"));
    }
    forgetting.await.unwrap();
    settled(&trace);
    let mut found = kinds(&aside(trace.file()));
    assert!(!found.is_empty(), "a file was moved aside meanwhile");
    found.extend(kinds(trace.file()));
    assert_eq!(
        found, sent,
        "every line after the forgetting, none before it, in the order sent"
    );
}

#[test]
fn lines_past_the_queue_behind_a_held_file_are_one_count_once_it_drains() {
    let dir = tempfile::tempdir().unwrap();
    let trace = trace_in(dir.path(), 5_000_000);
    let held = Held::on(trace.file());
    for n in 0..=QUEUE {
        trace.event(&event(1, &format!("line{n}")));
    }
    // Sent behind a full queue and dropped: the queue is not given a way to grow.
    trace.event(&event(1, "dropped_too"));
    drop(held);
    settled(&trace);
    let found = kinds(trace.file());
    assert_eq!(found.len(), QUEUE + 1, "the queue's lines and one count");
    assert_eq!(found[0], "line0");
    assert_eq!(found[QUEUE - 1], format!("line{}", QUEUE - 1));
    assert_eq!(found[QUEUE], "daemon.error", "the count comes last");
    let text = std::fs::read_to_string(trace.file()).unwrap();
    let count = text.lines().last().unwrap();
    assert!(count.contains("2 trace lines were dropped"), "{count}");
    assert_eq!(text.matches("daemon.error").count(), 1);
}

#[tokio::test]
async fn a_forgetting_and_the_deletes_notice_get_through_a_full_queue_and_the_caller_does_not_wait() {
    let dir = tempfile::tempdir().unwrap();
    let trace = trace_in(dir.path(), 5_000_000);
    let held = Held::on(trace.file());
    let begun = Instant::now();
    for n in 0..=QUEUE {
        trace.event(&event(1, &format!("line{n}")));
    }
    let forgetting = trace.forget(1, Some(&deleted(1)));
    // The hold lets go by itself after five seconds: a caller that waited for the
    // file would be past three.
    assert!(
        begun.elapsed() < Duration::from_secs(3),
        "the lines, the forgetting and the notice were queued, not written: {:?}",
        begun.elapsed()
    );
    drop(held);
    forgetting.await.expect("the forgetting is not dropped");
    settled(&trace);
    assert_eq!(
        kinds(trace.file()),
        ["daemon.error", "project.deleted"],
        "the project's lines went, the count of the dropped one stayed, and the notice came last"
    );
}

/// What the durable steps of a replacement did, in order.
static STEPS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

fn recorded_flush(file: &std::fs::File) -> io::Result<()> {
    STEPS.lock().unwrap().push("flush");
    flush(file)
}

fn recorded_rename(from: &Path, to: &Path) -> io::Result<()> {
    STEPS.lock().unwrap().push("rename");
    rename_durably(from, to)
}

fn failed_flush(_: &std::fs::File) -> io::Result<()> {
    Err(io::Error::other("the disk would not flush"))
}

fn failed_rename(from: &Path, _: &Path) -> io::Result<()> {
    let _ = std::fs::remove_file(from);
    Err(io::Error::other("the rename would not go"))
}

fn failed_folder(_: &Path) -> io::Result<()> {
    Err(io::Error::other("the folder would not flush"))
}

/// The replacement is gone before its rename, as another owner of the name took it.
fn vanished_replacement(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::remove_file(from)?;
    rename_durably(from, to)
}

fn recorded_folder(folder: &Path) -> io::Result<()> {
    STEPS.lock().unwrap().push("folder");
    flush_folder(folder)
}

fn durable_trace(dir: &Path, durable: Durable) -> Trace {
    Trace::made(
        dir.join("events.jsonl"),
        5_000_000,
        Box::new(SystemClock),
        durable,
    )
}

const DISK: Durable = Durable {
    flush,
    rename: rename_durably,
    folder: flush_folder,
};

#[test]
fn a_forgetting_answers_done_only_once_both_files_were_flushed_renamed_and_the_folder_flushed() {
    let dir = tempfile::tempdir().unwrap();
    let trace = durable_trace(
        dir.path(),
        Durable {
            flush: recorded_flush,
            rename: recorded_rename,
            folder: recorded_folder,
        },
    );
    std::fs::write(
        aside(trace.file()),
        "{\"at\":\"a\",\"project\":1,\"kind\":\"old\"}\n",
    )
    .unwrap();
    trace.event(&event(1, "gone"));
    trace.event(&event(2, "kept"));
    settled(&trace);
    STEPS.lock().unwrap().clear();
    trace.forget_blocking(1, None).unwrap();
    assert_eq!(
        *STEPS.lock().unwrap(),
        ["flush", "rename", "flush", "rename", "folder"],
        "each replacement is flushed and renamed, then the folder, before the answer"
    );
    assert_eq!(kinds(trace.file()), ["kept"]);
}

#[test]
fn a_forgetting_whose_flush_failed_is_an_error_and_keeps_the_lines() {
    for (durable, said) in [
        (Durable { flush: failed_flush, ..DISK }, "would not flush"),
        (Durable { rename: failed_rename, ..DISK }, "rename would not go"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let trace = durable_trace(dir.path(), durable);
        trace.event(&event(1, "one"));
        settled(&trace);
        let failed = trace.forget_blocking(1, None).unwrap_err();
        assert!(failed.to_string().contains("events.jsonl"), "{failed}");
        assert!(failed.to_string().contains(said), "{failed}");
        assert_eq!(
            kinds(trace.file()),
            ["one"],
            "a forgetting that could not be made durable is not an answer of done"
        );
    }
}

#[test]
fn a_retry_that_finds_nothing_to_remove_still_needs_the_folder_flushed() {
    // The first forgetting renamed both files and then could not flush the folder.
    let dir = tempfile::tempdir().unwrap();
    let trace = durable_trace(dir.path(), Durable { folder: failed_folder, ..DISK });
    trace.event(&event(1, "one"));
    settled(&trace);
    assert!(trace.forget_blocking(1, None).is_err());
    assert!(kinds(trace.file()).is_empty(), "the name is clean already");
    // A retry on a folder that still cannot be flushed finds clean names and no answer of done.
    let retry = trace.forget_blocking(1, None).unwrap_err();
    assert!(retry.to_string().contains("the folder would not flush"), "{retry}");
    // One that can be flushed is done.
    let dir = tempfile::tempdir().unwrap();
    let trace = durable_trace(dir.path(), DISK);
    trace.forget_blocking(1, None).unwrap();
}

#[test]
fn a_replacement_that_is_not_there_for_its_rename_is_no_success() {
    let dir = tempfile::tempdir().unwrap();
    let trace = durable_trace(dir.path(), Durable { rename: vanished_replacement, ..DISK });
    trace.event(&event(7, "original"));
    settled(&trace);
    let failed = trace.forget_blocking(7, None).unwrap_err();
    assert_eq!(failed.kind(), io::ErrorKind::NotFound, "{failed}");
    assert_eq!(kinds(trace.file()), ["original"]);
}

#[test]
fn an_abandoned_replacement_is_removed_by_a_forgetting_and_by_a_start_before_any_answer() {
    let line = "{\"at\":\"a\",\"project\":2,\"kind\":\"left\"}\n";
    // By a forgetting: one that could not be renamed leaves its replacement.
    let dir = tempfile::tempdir().unwrap();
    let trace = durable_trace(dir.path(), Durable { flush: failed_flush, ..DISK });
    trace.event(&event(1, "one"));
    trace.event(&event(2, "two"));
    settled(&trace);
    drop(trace.forget_blocking(1, None));
    let temporary = dir.path().join("events.jsonl.tmp");
    let again = durable_trace(dir.path(), DISK);
    // The writer has started once it answers.
    again.forget_blocking(9, None).unwrap();
    std::fs::write(&temporary, line).unwrap();
    again.forget_blocking(2, None).unwrap();
    assert!(!temporary.exists(), "the replacement a forgetting left is removed");
    assert_eq!(kinds(again.file()), ["one"], "project 2 is gone from the names");
    // By a start: the writer removes both before it writes anything.
    let dir = tempfile::tempdir().unwrap();
    let names = [dir.path().join("events.jsonl.tmp"), dir.path().join("events.jsonl.1.tmp")];
    for name in &names {
        std::fs::write(name, line).unwrap();
    }
    let started = durable_trace(dir.path(), DISK);
    started.event(&event(3, "three"));
    settled(&started);
    assert!(names.iter().all(|name| !name.exists()), "a start removes them");
}

#[test]
fn a_file_whose_last_line_was_cut_is_ended_before_the_first_line_written_after() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("events.jsonl"), "{\"project\":6,\"kind\":\"unfinished").unwrap();
    let trace = trace_in(dir.path(), 5_000_000);
    trace.event(&event(8, "first_after_restart"));
    trace.event(&event(8, "second"));
    settled(&trace);
    let text = std::fs::read_to_string(trace.file()).unwrap();
    let whole: Vec<Value> = text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    assert_eq!(whole.len(), 2, "the new lines are whole: {text}");
    assert_eq!(text.lines().count(), 3, "the fragment is a bad line of its own: {text}");
    // A whole last line takes no extra newline.
    let again = trace_in(dir.path(), 5_000_000);
    again.event(&event(8, "third"));
    settled(&again);
    assert_eq!(std::fs::read_to_string(again.file()).unwrap().lines().count(), 4);
}

#[test]
fn the_dropped_lines_report_has_a_place_of_its_own_that_later_lines_cannot_put_off() {
    let dir = tempfile::tempdir().unwrap();
    let trace = trace_in(dir.path(), 5_000_000);
    let held = Held::on(trace.file());
    for n in 0..=QUEUE {
        trace.event(&event(1, &format!("line{n}")));
    }
    drop(held);
    // Once the writer has made room, later lines come in behind the report.
    let until = Instant::now() + Duration::from_secs(10);
    while kinds(trace.file()).is_empty() {
        assert!(Instant::now() < until, "the writer wrote nothing");
        std::thread::sleep(Duration::from_millis(1));
    }
    trace.event(&event(1, "late"));
    settled(&trace);
    let found = kinds(trace.file());
    let report = found.iter().position(|kind| kind == "daemon.error").unwrap();
    let late = found.iter().position(|kind| kind == "late").unwrap();
    assert!(report < late, "the report of the lost line came before a later line: {report} {late}");
}

#[test]
fn a_trace_whose_writer_could_not_start_says_so_and_answers_no_forgetting_with_done() {
    let dir = tempfile::tempdir().unwrap();
    let trace = trace_in(dir.path(), 5_000_000);
    trace.shared_failed_for_test("no thread");
    assert!(trace.started().unwrap_err().to_string().contains("no thread"));
    assert!(trace.forget_blocking(1, None).is_err());
}

#[tokio::test]
async fn the_stop_waits_a_quarter_of_a_second_for_a_held_trace_and_ends_without_it() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let mut held = Held::on(&rig.home.join("events.jsonl"));
            rig.daemon
                .parts
                .ledger
                .borrow_mut()
                .create_project(&new_project(root.path()))
                .unwrap();
            let begun = Instant::now();
            rig.daemon.stop("test");
            rig.daemon.finished().await;
            let took = begun.elapsed();
            assert!(
                took >= SETTLE && took < Duration::from_millis(1500),
                "the stop waited {took:?} for the held trace"
            );
            assert_eq!(*rig.exited.borrow(), [0], "it ended");
            let written = std::fs::read_to_string(rig.home.join("events.jsonl")).unwrap_or_default();
            assert!(!written.contains("project.created"), "{written}");
            assert!(
                log_lines(&rig.home)
                    .iter()
                    .any(|line| line.contains("the trace was not written in time")),
                "{:?}",
                log_lines(&rig.home)
            );
            held.release();
            let written = crate::files::settled_trace(&rig.home).unwrap();
            assert!(written.contains("project.created"), "{written}");
        })
        .await;
}

#[tokio::test]
async fn the_stop_writes_what_the_trace_was_told_before_it_ends() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            rig.daemon
                .parts
                .ledger
                .borrow_mut()
                .create_project(&new_project(root.path()))
                .unwrap();
            rig.daemon.stop("test");
            rig.daemon.finished().await;
            let written = std::fs::read_to_string(rig.home.join("events.jsonl")).unwrap();
            assert!(written.contains("project.created"), "{written}");
        })
        .await;
}

// What the stop does with the home while the trace is written.

/// Whether the ledger's file can be read by another connection: it can once the
/// daemon has let the home go.
fn ledger_is_let_go(home: &Path) -> bool {
    let Ok(reader) = rusqlite::Connection::open_with_flags(
        home.join("consensflow.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        return false;
    };
    reader.busy_timeout(Duration::ZERO).unwrap();
    reader
        .query_row("SELECT count(*) FROM project", [], |row| row.get::<_, i64>(0))
        .is_ok()
}

#[tokio::test]
async fn the_home_is_let_go_only_after_the_trace_is_written() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let held = Held::on(&rig.home.join("events.jsonl"));
            rig.daemon
                .parts
                .ledger
                .borrow_mut()
                .create_project(&new_project(root.path()))
                .unwrap();
            // The hold lets go 100 ms into the stop, inside its quarter of a second.
            let (released_at, released) = mpsc::channel();
            let letting = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                drop(held);
                released_at.send(Instant::now()).unwrap();
            });
            let home = rig.home.clone();
            let (let_go_at, let_go) = mpsc::channel();
            let watching = std::thread::spawn(move || {
                let until = Instant::now() + Duration::from_secs(10);
                while !ledger_is_let_go(&home) {
                    assert!(Instant::now() < until, "the home was never let go");
                    std::thread::sleep(Duration::from_millis(1));
                }
                let_go_at.send(Instant::now()).unwrap();
            });
            std::thread::sleep(Duration::from_millis(50));
            rig.daemon.stop("test");
            rig.daemon.finished().await;
            letting.join().unwrap();
            watching.join().unwrap();
            assert!(
                let_go.recv().unwrap() >= released.recv().unwrap(),
                "the ledger was let go while the trace's writer still held the file"
            );
            let written = crate::files::settled_trace(&rig.home).unwrap();
            assert!(written.contains("project.created"), "{written}");
        })
        .await;
}

#[tokio::test]
async fn a_trace_that_is_not_written_in_time_keeps_the_home_until_the_process_ends() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let held = Held::on(&rig.home.join("events.jsonl"));
            rig.daemon
                .parts
                .ledger
                .borrow_mut()
                .create_project(&new_project(root.path()))
                .unwrap();
            rig.daemon.stop("test");
            rig.daemon.finished().await;
            assert_eq!(*rig.exited.borrow(), [0], "the stop ended");
            assert!(
                !ledger_is_let_go(&rig.home),
                "a second daemon could take the home while the writer is still at the trace"
            );
            drop(held);
        })
        .await;
}

#[tokio::test]
async fn a_stop_after_a_delete_that_is_asked_has_the_deletes_notice_and_none_of_its_lines() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let id = rig
                .daemon
                .parts
                .ledger
                .borrow_mut()
                .create_project(&new_project(root.path()))
                .unwrap()
                .id;
            rig.daemon.parts.engine.close_project(id).await.unwrap();
            crate::files::settle_in(&rig.home);
            let engine = Rc::clone(&rig.daemon.parts.engine);
            let deleting = tokio::task::spawn_local(async move { engine.delete_project(id).await });
            // The delete runs to its first wait: the ledger has it, and the trace was asked.
            tokio::task::yield_now().await;
            assert!(rig.daemon.parts.ledger.borrow().project(id).unwrap().is_none());
            // The stop takes the daemon's one thread from here: the delete's continuation
            // does not run before it ends.
            rig.daemon.stop("test");
            rig.daemon.finished().await;
            let written = std::fs::read_to_string(rig.home.join("events.jsonl")).unwrap();
            let lines: Vec<Value> = written
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect();
            assert!(
                lines.iter().any(|line| line["kind"] == "project.deleted"
                    && line["data"]["id"].as_i64() == Some(id)),
                "the notice of the committed delete is in the trace at exit: {written}"
            );
            assert!(
                lines.iter().all(|line| line["project"].as_i64() != Some(id)),
                "none of the project's lines: {written}"
            );
            drop(deleting);
        })
        .await;
}
