//! The index on stores made from Muse 1.4.13's own schema
//! (`tests/fixtures/muse/session-index.schema.sql`), beside a writer as Muse is one,
//! and the rule it serves against what a recording saw the index say
//! (`tests/fixtures/muse/index-states.json`).

use std::fs;
use std::path::Path;
use std::sync::Arc;

use rusqlite::Connection;
use serde_json::Value;

use super::*;
use crate::records::{Item, Record};

const SCHEMA: &str = include_str!("../../../tests/fixtures/muse/session-index.schema.sql");

/// A Muse data folder whose index holds `rows` (a session, its prompts, whether its
/// latest segment ended), in the journal mode `journal`.
struct Data {
    home: tempfile::TempDir,
}

impl Data {
    fn new() -> Self {
        let data = Self {
            home: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(data.home.path().join("muse")).unwrap();
        data
    }

    fn env(&self) -> Env {
        Env::from_vars([("XDG_DATA_HOME", self.home.path().to_str().unwrap())])
    }

    fn index(&self) -> std::path::PathBuf {
        self.home.path().join("muse").join("session-index.db")
    }

    /// Muse's index, made as Muse makes it, holding `rows`.
    fn write(&self, journal: &str, rows: &[(&str, i64, i64)]) -> Connection {
        let connection = Connection::open(self.index()).unwrap();
        connection
            .pragma_update(None, "journal_mode", journal)
            .unwrap();
        connection.execute_batch(SCHEMA).unwrap();
        for (session, prompts, terminated) in rows {
            insert(&connection, session, *prompts, *terminated);
        }
        connection
    }
}

/// A row as Muse writes one, of the columns its constraints ask for.
fn insert(connection: &Connection, session: &str, prompts: i64, terminated: i64) {
    let log = format!("/muse-home/data/muse/sessions/2026/10/26/{session}/session.jsonl");
    connection
        .execute(
            "INSERT INTO sessions (session_id, session_stream_id, session_dir, session_log_path, \
             layout, title, search_text, prompt_count, status, status_rank, indexed_at_us, \
             latest_segment_terminated) \
             VALUES (?1, ?1, ?2, ?3, 'session_jsonl', '', '', ?4, 'valid', 0, 0, ?5)",
            rusqlite::params![
                session,
                Path::new(&log).parent().unwrap().to_str(),
                log,
                prompts,
                terminated
            ],
        )
        .unwrap();
}

#[test]
fn a_row_says_its_prompts_and_no_row_or_no_index_is_none() {
    let data = Data::new();
    assert_eq!(row("s1", &data.env()), Ok(None), "no index");
    let _muse = data.write("delete", &[("s1", 2, 1), ("s0", 0, 0)]);
    assert_eq!(row("s1", &data.env()), Ok(Some(Row { prompts: 2 })));
    assert_eq!(row("s0", &data.env()), Ok(Some(Row { prompts: 0 })));
    assert_eq!(row("s2", &data.env()), Ok(None));
    // An index that is no database is unreadable.
    fs::write(data.index(), "not sqlite").unwrap();
    assert!(row("s1", &data.env()).is_err());
}

#[test]
fn the_index_is_read_beside_a_muse_writing_it_and_holds_nothing_after() {
    let data = Data::new();
    let muse = data.write("wal", &[("s1", 1, 1)]);
    // Muse writes, and has not committed: the read sees what was committed, at once.
    muse.execute_batch("BEGIN IMMEDIATE").unwrap();
    insert(&muse, "s2", 3, 0);
    assert_eq!(row("s1", &data.env()), Ok(Some(Row { prompts: 1 })));
    assert_eq!(row("s2", &data.env()), Ok(None));
    muse.execute_batch("COMMIT").unwrap();
    assert_eq!(row("s2", &data.env()), Ok(Some(Row { prompts: 3 })));
    // No read holds the log: Muse's checkpoint takes all of it back into the database.
    let (busy, _, _): (i64, i64, i64) = muse
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |found| {
            Ok((found.get(0)?, found.get(1)?, found.get(2)?))
        })
        .unwrap();
    assert_eq!(busy, 0, "a reader still held a snapshot of the index");
    // Nor is the index written: its rows are Muse's alone.
    let rows: i64 = muse
        .query_row("SELECT count(*) FROM sessions", [], |found| found.get(0))
        .unwrap();
    assert_eq!(rows, 2);
}

fn known(roles: &[Role]) -> Reading {
    let mut record = Record::new();
    record.items = roles
        .iter()
        .map(|&role| Item {
            id: Arc::from("i"),
            role,
            text: Arc::from("t"),
            complete: true,
            at: None,
            commentary: false,
            compaction: false,
            synthetic: false,
        })
        .collect();
    Reading::Known(record)
}

#[test]
fn the_log_decides_and_the_index_says_only_yes_where_the_log_cannot_be_read() {
    let counted = || Ok(Some(Row { prompts: 1 }));
    let missing = Reading::Missing("unreadable: no muse session log for s1".to_owned());
    let unreadable = Reading::Unknown("unreadable: torn".to_owned());
    // A log that holds a message of the user's is a yes, whatever the index says.
    assert_eq!(
        resumable(&known(&[Role::Custom, Role::User]), Ok(None)),
        Some(true)
    );
    assert_eq!(
        resumable(&known(&[Role::User]), Err("locked".to_owned())),
        Some(true)
    );
    // One that holds none is a no, and so is no log: a session pruned by retention
    // whose row was left behind, too.
    assert_eq!(resumable(&known(&[Role::Custom]), counted()), Some(false));
    assert_eq!(resumable(&missing, counted()), Some(false));
    // A log there and unreadable: only a row that counts a prompt says yes.
    assert_eq!(resumable(&unreadable, counted()), Some(true));
    assert_eq!(resumable(&unreadable, Ok(Some(Row { prompts: 0 }))), None);
    assert_eq!(resumable(&unreadable, Ok(None)), None);
    assert_eq!(resumable(&unreadable, Err("locked".to_owned())), None);
}

/// A step of the recording: its name, the index's rows (a session, its prompts,
/// whether its latest segment ended) and the sessions whose logs were on disk.
type State = (String, Vec<(String, i64, i64)>, Vec<String>);

/// What the recording saw, step by step.
fn states() -> Vec<State> {
    let states: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/muse/index-states.json"
    ))
    .unwrap();
    states
        .as_array()
        .unwrap()
        .iter()
        .map(|state| {
            let rows = state["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    let row = row.as_array().unwrap();
                    (
                        row[0].as_str().unwrap().to_owned(),
                        row[1].as_i64().unwrap(),
                        row[2].as_i64().unwrap(),
                    )
                })
                .collect();
            let logs = state["logs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|log| log.as_str().unwrap().to_owned())
                .collect();
            (state["step"].as_str().unwrap().to_owned(), rows, logs)
        })
        .collect()
}

#[test]
fn the_index_lags_the_logs_so_a_missing_row_is_no_no_and_a_segment_not_ended_no_cut() {
    let states = states();
    let at = |step: &str| states.iter().find(|(name, ..)| name == step).unwrap();
    // Every session that was prompted kept its log; a window quit before its first
    // message kept none.
    let (_, _, logs) = at("the fourth window quit");
    assert_eq!(logs.len(), 3);
    // A session answered and quit cleanly has no row until another Muse starts: the
    // brief's "no row is a no" would leave its conversation behind.
    let (_, rows, logs) = at("first session quit cleanly");
    assert!(rows.is_empty() && logs.len() == 1, "{rows:?} {logs:?}");
    let (_, rows, _) = at("second session up");
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].1, rows[0].2), (1, 1));
    // A session killed at rest, its turn answered, reads as a segment that did not end:
    // the index cannot tell a cut turn (the third session's log reads it settled:
    // `muse::record`'s `at-rest-killed`).
    let (_, rows, logs) = at("third session resumed");
    let third = &logs[2];
    assert_eq!(
        rows.iter()
            .find(|(id, ..)| id == third)
            .map(|row| (row.1, row.2)),
        Some((1, 0))
    );
    // Resumed and quit, its row is gone, and comes back when the next Muse starts.
    let (_, rows, logs) = at("third session resumed and quit");
    assert!(rows.iter().all(|(id, ..)| id != third) && logs.contains(third));
    let (_, rows, _) = at("a fourth window up");
    assert_eq!(
        rows.iter()
            .find(|(id, ..)| id == third)
            .map(|row| (row.1, row.2)),
        Some((1, 1))
    );
}
