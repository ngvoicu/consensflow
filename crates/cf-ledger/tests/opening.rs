//! The ledger as a file (Node's ledger suite, "opening the ledger"): the daemon
//! is the only process that holds it, and a file that is no ledger, or a newer
//! build's, is named rather than failed on. Another process is this test binary
//! run again, as a child that opens the file.

// The tests start the child process themselves, and the child reads what to
// do from its environment; their helpers expect, as the tests do.
#![allow(clippy::disallowed_methods, clippy::expect_used)]

use std::cell::RefCell;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;

use cf_base::time::Clock;
use cf_ledger::{open_ledger, Event, LedgerError, NewChief, NewProject, Options, SCHEMA_VERSION};
use rusqlite::{Connection, OpenFlags};

/// The ledger tests' clock: 2026-09-19 at 10:00, a second later at each reading.
struct Ticks(i64);

impl Clock for Ticks {
    fn now_ms(&mut self) -> i64 {
        self.0 += 1000;
        self.0
    }
}

fn options() -> Options {
    Options {
        clock: Box::new(Ticks(1_789_812_000_000)),
        ..Options::default()
    }
}

fn project(name: &str, harness: &str) -> NewProject {
    NewProject {
        directory: format!("/work/{name}"),
        name: name.into(),
        chief: NewChief {
            harness: harness.into(),
            agent: None,
        },
        staff: Vec::new(),
        gate: false,
    }
}

fn code(result: Result<impl Sized, LedgerError>) -> Option<&'static str> {
    result.err().and_then(|error| error.code())
}

/// What the child is asked: `open` (say "opened" or the refusal's code) or
/// `hold` (open, say "ready", and wait to be killed), and the file.
const CHILD: &str = "CF_LEDGER_TEST_CHILD";

/// The child's half: nothing unless the parent started this binary as one.
#[test]
fn child() {
    let Ok(task) = std::env::var(CHILD) else {
        return;
    };
    let (what, file) = task.split_once(':').expect("what and the file");
    let opened = open_ledger(Path::new(file), options());
    match (what, opened) {
        ("open", Ok(_)) => println!("child said: opened"),
        ("open", Err(error)) => println!("child said: {}", error.code().unwrap_or("error")),
        ("hold", Ok(_ledger)) => {
            println!("child said: ready");
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        (_, outcome) => panic!("cannot {what}: {:?}", outcome.err()),
    }
}

fn spawn_child(what: &str, file: &Path) -> Child {
    Command::new(std::env::current_exe().expect("this test binary"))
        .args(["--exact", "child", "--nocapture", "--test-threads=1"])
        .env(CHILD, format!("{what}:{}", file.display()))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the child starts")
}

/// The first thing the child said.
fn said(child: &mut Child) -> String {
    let lines = BufReader::new(child.stdout.take().expect("its output")).lines();
    lines
        .map_while(Result::ok)
        // The harness writes "test child ... " first, on the same line.
        .find_map(|line| {
            line.split_once("child said: ")
                .map(|(_, said)| said.to_string())
        })
        .unwrap_or_default()
}

#[test]
fn creates_the_schema_at_the_current_version_and_keeps_its_data_across_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    let mut first = open_ledger(&file, options()).unwrap();
    let created = first.create_project(&project("app", "opencode")).unwrap();
    first.close().unwrap();

    let raw = Connection::open_with_flags(&file, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let version: i64 = raw
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let journal: String = raw
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!((version, journal.as_str()), (SCHEMA_VERSION as i64, "wal"));
    drop(raw);

    let second = open_ledger(&file, options()).unwrap();
    let projects: Vec<_> = second
        .projects()
        .unwrap()
        .into_iter()
        .map(|project| (project.id, project.name, project.state))
        .collect();
    assert_eq!(
        projects,
        [(created.id, "app".to_string(), "open".to_string())]
    );
}

#[test]
fn tells_a_trace_every_event_as_it_is_logged_with_the_time_the_ledger_gave_it() {
    let dir = tempfile::tempdir().unwrap();
    let entries = Rc::new(RefCell::new(Vec::<Event>::new()));
    let told = Rc::clone(&entries);
    let mut ledger = open_ledger(
        &dir.path().join("consensflow.db"),
        Options {
            trace: Box::new(move |event| told.borrow_mut().push(event.clone())),
            ..options()
        },
    )
    .unwrap();
    let project = ledger.create_project(&project("app", "pi")).unwrap();
    let logged = ledger.events(project.id, 0, 500).unwrap();
    let entries = entries.borrow();
    assert_eq!(
        entries[0],
        Event {
            at: logged[0].at.clone(),
            project: project.id,
            kind: "project.created".into(),
            data: serde_json::json!({ "name": "app", "directory": "/work/app" }),
        }
    );
    let told: Vec<_> = entries
        .iter()
        .map(|event| (event.at.clone(), event.kind.clone()))
        .collect();
    let kept: Vec<_> = logged
        .iter()
        .map(|event| (event.at.clone(), event.kind.clone()))
        .collect();
    assert_eq!(told, kept, "the trace is the event log, as it happens");
}

#[test]
fn refuses_a_second_open_while_the_first_holds_the_file_in_this_process_and_in_another() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    let holder = open_ledger(&file, options()).unwrap();
    let refused = open_ledger(&file, options()).err().unwrap();
    assert_eq!(
        (refused.code(), refused.to_string()),
        (
            Some("ledger-locked"),
            format!("another ConsensFlow has {} open", file.display())
        )
    );
    let mut other = spawn_child("open", &file);
    assert_eq!(said(&mut other), "ledger-locked");
    other.wait().unwrap();
    holder.close().unwrap();
    open_ledger(&file, options()).unwrap().close().unwrap();
}

#[test]
fn closes_in_place_where_it_is_shared_and_frees_the_file_with_its_data_kept() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    // Shared as the daemon shares it: every task holds the one cell.
    let shared = Rc::new(RefCell::new(open_ledger(&file, options()).unwrap()));
    let other_holder = Rc::clone(&shared);
    shared
        .borrow_mut()
        .create_project(&project("app", "claude-code"))
        .unwrap();
    assert_eq!(code(open_ledger(&file, options())), Some("ledger-locked"));

    other_holder.borrow_mut().close_in_place().unwrap();

    let reopened = open_ledger(&file, options()).unwrap();
    assert_eq!(
        reopened.projects().unwrap().len(),
        1,
        "what was written stays"
    );
    reopened.close().unwrap();
    // What is left of the closed one answers with an error, not a panic.
    assert!(shared.borrow().projects().is_err());
    assert!(shared
        .borrow_mut()
        .create_project(&project("late", "claude-code"))
        .is_err());
}

#[test]
fn is_free_again_once_a_holder_is_killed() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    let mut holder = spawn_child("hold", &file);
    assert_eq!(said(&mut holder), "ready");
    assert_eq!(code(open_ledger(&file, options())), Some("ledger-locked"));
    holder.kill().unwrap();
    holder.wait().unwrap();
    open_ledger(&file, options()).unwrap().close().unwrap();
}

#[test]
fn refuses_a_database_written_by_a_newer_consensflow() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    let raw = Connection::open(&file).unwrap();
    raw.execute_batch(&format!("PRAGMA user_version = {}", SCHEMA_VERSION + 1))
        .unwrap();
    drop(raw);
    assert_eq!(code(open_ledger(&file, options())), Some("ledger-newer"));
}

#[test]
fn names_a_file_that_is_not_a_ledger_instead_of_failing_somewhere_inside_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    std::fs::write(
        &file,
        "this is not a database, it is a text file ".repeat(200),
    )
    .unwrap();
    let refused = open_ledger(&file, options()).err().unwrap();
    assert_eq!(
        (refused.code(), refused.to_string()),
        (
            Some("ledger-unreadable"),
            format!(
                "{} is not a readable ConsensFlow ledger: file is not a database",
                file.display()
            )
        )
    );
}
