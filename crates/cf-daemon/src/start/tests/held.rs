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
            // A line behind the held file, which the delete's forgetting comes after.
            rig.daemon
                .parts
                .ledger
                .borrow_mut()
                .create_project(&new_project(root.path()))
                .unwrap();
            let engine = Rc::clone(&rig.daemon.parts.engine);
            let deleting = tokio::task::spawn_local(async move { engine.delete_project(id).await });
            let took = ping_and_timer(&rig.app).await;
            assert!(took < WITHIN, "a ping and a timer took {took:?}");
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

use cf_base::file::{aside, flush, rename_durably};
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
    let forgetting = trace.forget(1);
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
async fn a_forgetting_and_the_deletes_notice_get_through_a_full_queue() {
    let dir = tempfile::tempdir().unwrap();
    let trace = trace_in(dir.path(), 5_000_000);
    let held = Held::on(trace.file());
    for n in 0..=QUEUE {
        trace.event(&event(1, &format!("line{n}")));
    }
    let forgetting = trace.forget(1);
    trace.write(&deleted(1));
    drop(held);
    forgetting.await.expect("the forgetting is not dropped");
    settled(&trace);
    assert_eq!(
        kinds(trace.file()),
        ["project.deleted", "daemon.error"],
        "the project's lines went, its notice stayed, and the count came last"
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

fn failed_rename(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::other("the folder would not flush"))
}

fn durable_trace(dir: &Path, durable: Durable) -> Trace {
    Trace::made(
        dir.join("events.jsonl"),
        5_000_000,
        Box::new(SystemClock),
        durable,
    )
}

#[test]
fn a_forgetting_answers_done_only_once_both_files_were_flushed_and_renamed() {
    let dir = tempfile::tempdir().unwrap();
    let trace = durable_trace(
        dir.path(),
        Durable {
            flush: recorded_flush,
            rename: recorded_rename,
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
    trace.forget_blocking(1).unwrap();
    assert_eq!(
        *STEPS.lock().unwrap(),
        ["flush", "rename", "flush", "rename"],
        "each replacement is flushed and then its rename, before the answer"
    );
    assert_eq!(kinds(trace.file()), ["kept"]);
}

#[test]
fn a_forgetting_whose_flush_failed_is_an_error_and_keeps_the_lines() {
    for durable in [
        Durable {
            flush: failed_flush,
            rename: rename_durably,
        },
        Durable {
            flush,
            rename: failed_rename,
        },
    ] {
        let dir = tempfile::tempdir().unwrap();
        let trace = durable_trace(dir.path(), durable);
        trace.event(&event(1, "one"));
        settled(&trace);
        let failed = trace.forget_blocking(1).unwrap_err();
        assert!(failed.to_string().contains("events.jsonl"), "{failed}");
        assert!(failed.to_string().contains("flush"), "{failed}");
        assert_eq!(
            kinds(trace.file()),
            ["one"],
            "a forgetting that could not be made durable is not an answer of done"
        );
    }
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
