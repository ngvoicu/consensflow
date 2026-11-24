//! The update to the staff as slots changes no message (`architecture/staff-slots.md` 9.5,
//! gate 3). Every older ledger we hold is opened by this version (A), twice, and once with the
//! update's migration left out (B: `testing::open_ledger_without_the_slots_update`, every
//! other line of the opening is the product's). A equals B in every row of `message`, `task`,
//! `task_need`, `conversation`, `question_binding` and `event` but the update's own events;
//! `participant` differs only in `tier`; and every window is offered, and given, the same next
//! delivery. A message an earlier version saved therefore goes on as that version's next start
//! would have taken it on.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use cf_base::time::Clock;
use cf_ledger::testing::open_ledger_without_the_slots_update;
use cf_ledger::{open_ledger, Ledger, Options};
use rusqlite::Connection;
use serde_json::Value;

struct Fixed;

impl Clock for Fixed {
    fn now_ms(&mut self) -> i64 {
        1_790_000_000_000
    }
}

fn options() -> Options {
    let mut at = 0;
    Options {
        clock: Box::new(Fixed),
        names: Box::new(move || {
            at += 1;
            format!("window-{at}")
        }),
        trace: Box::new(|_| {}),
    }
}

/// Every older ledger: the 16 of the dispatcher's `alpha13/` and the 16 of `alpha13-corpus/`
/// (written by alpha.13's own ledger), the slots' own `t2013-alpha13.db.gz`, and the two
/// alpha.10 archives.
fn ledgers() -> Vec<(String, Vec<u8>)> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut all = Vec::new();
    for (dir, count) in [
        (manifest.join("../cf-engine/tests/dispatcher/alpha13"), 16),
        (manifest.join("tests/fixtures/alpha13-corpus"), 16),
    ] {
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.to_string_lossy().ends_with(".db.gz"))
            .collect();
        found.sort();
        assert_eq!(found.len(), count, "{}", dir.display());
        for path in found {
            all.push((
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&path).unwrap(),
            ));
        }
    }
    all.push((
        "t2013-alpha13.db.gz".to_owned(),
        include_bytes!("fixtures/t2013-alpha13.db.gz").to_vec(),
    ));
    all.push((
        "t1813-reassign.db.gz".to_owned(),
        include_bytes!("../../cf-daemon/tests/goldens/t1813-alpha10/reassign.db.gz").to_vec(),
    ));
    all.push((
        "t1813-backup.db.gz".to_owned(),
        include_bytes!("../../cf-daemon/tests/goldens/t1813-alpha10/backup.db.gz").to_vec(),
    ));
    all
}

fn unpack(gz: &[u8]) -> (tempfile::TempDir, PathBuf) {
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_end(&mut raw)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    std::fs::write(&file, raw).unwrap();
    (dir, file)
}

type Table = Vec<Vec<String>>;

/// Every row of `table` in rowid order, each value as SQLite quotes it, but for `skip`, and
/// but for the rows `keep` refuses.
fn table(db: &Connection, name: &str, skip: &[&str], filter: &str) -> Table {
    let columns: Vec<String> = db
        .prepare(&format!("PRAGMA table_info(\"{name}\")"))
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(Result::unwrap)
        .filter(|column| !skip.contains(&column.as_str()))
        .collect();
    let select = columns
        .iter()
        .map(|column| format!("quote(\"{column}\")"))
        .collect::<Vec<_>>()
        .join(", ");
    db.prepare(&format!(
        "SELECT {select} FROM \"{name}\" {filter} ORDER BY rowid"
    ))
    .unwrap()
    .query_map([], |row| {
        (0..columns.len())
            .map(|at| row.get::<_, String>(at))
            .collect::<rusqlite::Result<Vec<_>>>()
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

const UPDATES: &str = "WHERE kind NOT IN ('member.settled', 'member.unsettled')";

/// What the update must leave as it was.
fn kept(ledger: &Ledger) -> Vec<Table> {
    let db = ledger.sqlite();
    vec![
        table(db, "message", &[], ""),
        table(db, "task", &[], ""),
        table(db, "task_need", &[], ""),
        table(db, "conversation", &[], ""),
        table(db, "question_binding", &[], ""),
        table(db, "event", &["id"], UPDATES),
        table(db, "participant", &["tier"], ""),
    ]
}

/// Everything in every table, to see that a second opening writes nothing.
fn everything(ledger: &Ledger) -> Vec<(String, Table)> {
    let db = ledger.sqlite();
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    names
        .into_iter()
        .map(|name| {
            let rows = table(db, &name, &[], "");
            (name, rows)
        })
        .collect()
}

fn events(ledger: &Ledger, kind: &str) -> Vec<Value> {
    ledger
        .sqlite()
        .prepare("SELECT data FROM event WHERE kind = ? ORDER BY id")
        .unwrap()
        .query_map([kind], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|data| serde_json::from_str(&data.unwrap()).unwrap())
        .collect()
}

fn participants(ledger: &Ledger) -> Vec<(i64, String, Option<String>)> {
    ledger
        .sqlite()
        .prepare("SELECT id, handle, tier FROM participant ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn every_older_ledger_opens_twice_and_the_update_changes_no_message() {
    for (name, gz) in ledgers() {
        let (_a_dir, a_file) = unpack(&gz);
        let (_b_dir, b_file) = unpack(&gz);

        let mut a = open_ledger(&a_file, options()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let first = everything(&a);
        drop(a);
        a = open_ledger(&a_file, options()).unwrap_or_else(|e| panic!("{name} again: {e}"));
        assert_eq!(
            first,
            everything(&a),
            "{name}: the second opening writes no row"
        );

        let mut b = open_ledger_without_the_slots_update(&b_file, options())
            .unwrap_or_else(|e| panic!("{name} without the update: {e}"));
        let labels = [
            "message",
            "task",
            "task_need",
            "conversation",
            "question_binding",
            "event",
            "participant but its tier",
        ];
        for ((label, from_a), from_b) in labels.iter().zip(kept(&a)).zip(kept(&b)) {
            assert_eq!(from_a, from_b, "{name}: {label} as the version without it");
        }

        // `participant` differs only in `tier`, and each difference is one the update logged:
        // a member's own (`member.settled`), or the open window or backup that follows it.
        let settled = events(&a, "member.settled");
        let to: BTreeSet<Option<String>> = settled
            .iter()
            .map(|data| data["to"].as_str().map(str::to_owned))
            .collect();
        for ((id, handle, tier_a), (_, _, tier_b)) in
            participants(&a).into_iter().zip(participants(&b))
        {
            if tier_a != tier_b {
                assert!(
                    to.contains(&tier_a),
                    "{name}: @{handle} (#{id}) went from {tier_b:?} to {tier_a:?}, which no member.settled says"
                );
            }
        }
        assert!(
            events(&b, "member.settled").is_empty() && events(&b, "member.unsettled").is_empty(),
            "{name}: B has no update"
        );

        // Every window is offered, and given, the same next delivery.
        for (id, handle, _) in participants(&a) {
            let from_a = a.next_delivery(id).unwrap();
            let from_b = b.next_delivery(id).unwrap();
            assert_eq!(
                from_a
                    .as_ref()
                    .map(|m| (m.id, m.body.clone(), m.state.clone())),
                from_b
                    .as_ref()
                    .map(|m| (m.id, m.body.clone(), m.state.clone())),
                "{name}: the next delivery of @{handle}"
            );
            if let Some(offered) = from_a {
                let given_a = a.begin_delivery(offered.id).unwrap();
                let given_b = b.begin_delivery(offered.id).unwrap();
                assert_eq!(
                    (
                        given_a.message.body,
                        given_a
                            .carried
                            .iter()
                            .map(|m| (m.id, m.body.clone()))
                            .collect::<Vec<_>>()
                    ),
                    (
                        given_b.message.body,
                        given_b
                            .carried
                            .iter()
                            .map(|m| (m.id, m.body.clone()))
                            .collect::<Vec<_>>()
                    ),
                    "{name}: what @{handle} is given"
                );
            }
        }
    }
}

fn named(file: &str) -> (tempfile::TempDir, Ledger) {
    let (_, gz) = ledgers()
        .into_iter()
        .find(|(name, _)| name.ends_with(file))
        .unwrap_or_else(|| panic!("no ledger {file}"));
    let (dir, path) = unpack(&gz);
    let ledger = open_ledger(&path, options()).unwrap();
    (dir, ledger)
}

fn states(ledger: &Ledger, kind: &str) -> Vec<(i64, String)> {
    ledger
        .sqlite()
        .prepare("SELECT id, state FROM message WHERE kind = ? ORDER BY id")
        .unwrap()
        .query_map([kind], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// A brief handed to the transport and then a tell: the brief is still on its way and the
/// tell still waits; the update writes no second brief, and withdraws nothing.
#[test]
fn a_handed_brief_and_its_tell_are_as_they_were_saved() {
    let (_dir, ledger) = named("t2136-handed.db.gz");
    let thread: Vec<(i64, String, String)> = ledger
        .sqlite()
        .prepare(
            "SELECT id, kind, state FROM message
              WHERE task_id = (SELECT task_id FROM message WHERE kind = 'task' AND state = 'delivering')
                AND kind IN ('task', 'note') ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        thread,
        vec![
            (2, "task".to_owned(), "delivering".to_owned()),
            (14, "note".to_owned(), "queued".to_owned())
        ],
        "the brief is still on its way, the tell still waits, and no second brief exists"
    );
}

/// A failed first launch with a queued tell: nothing is withdrawn and nobody is told.
#[test]
fn a_failed_first_launch_with_a_queued_tell_withdraws_nothing_and_tells_nobody() {
    let (_dir, ledger) = named("t2136-failed.db.gz");
    for kind in ["task", "note"] {
        assert!(
            states(&ledger, kind)
                .iter()
                .all(|(_, state)| state != "cancelled"),
            "{kind}"
        );
    }
    let told: i64 = ledger
        .sqlite()
        .query_row(
            "SELECT count(*) FROM event WHERE kind LIKE '%withdrawn%' OR kind LIKE 'update.%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(told, 0);
}

/// What a window read through the inbox while its paste was beginning keeps its state, and so
/// does every row still waiting behind it.
#[test]
fn what_was_read_through_the_inbox_keeps_its_state_and_every_waiting_row_still_waits() {
    for file in [
        "t2136-already-read.db.gz",
        "t2136-already-read-handed.db.gz",
        "t2136-note-received.db.gz",
        "t2175-release-early.db.gz",
        "t2175-release-brief-inbox.db.gz",
    ] {
        let (_, gz) = ledgers()
            .into_iter()
            .find(|(n, _)| n.ends_with(file))
            .unwrap();
        let (_b_dir, b_file) = unpack(&gz);
        let before: Vec<(i64, String)> = {
            let db = Connection::open(&b_file).unwrap();
            let mut read = db
                .prepare("SELECT id, state FROM message WHERE kind IN ('task','note','answer') ORDER BY id")
                .unwrap();
            let rows = read
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            rows
        };
        let (_dir, ledger) = named(file);
        let after: Vec<(i64, String)> = ["task", "note", "answer"]
            .iter()
            .flat_map(|kind| states(&ledger, kind))
            .collect::<std::collections::BTreeMap<_, _>>()
            .into_iter()
            .collect();
        assert_eq!(
            before, after,
            "{file}: every row as saved, waiting rows waiting"
        );
    }
}

/// Two tells of 500,000 characters: the ledger opens, and no note is written.
#[test]
fn a_long_tell_ledger_opens_and_no_note_is_written() {
    let (_dir, ledger) = named("t2136-long.db.gz");
    let notes: i64 = ledger
        .sqlite()
        .query_row(
            "SELECT count(*) FROM message WHERE kind = 'note' AND sender_id IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(notes, 0);
}

/// A role list that is no JSON: it opens, the member is as it was, one `member.unsettled`.
#[test]
fn a_role_list_that_is_no_json_opens_and_is_logged_once() {
    let (_, gz) = ledgers()
        .into_iter()
        .find(|(n, _)| n.ends_with("t2175-release-roles.db.gz"))
        .unwrap();
    let (_b_dir, b_file) = unpack(&gz);
    let before = participants_roles(&Connection::open(&b_file).unwrap());
    assert!(
        before
            .iter()
            .any(|(_, roles)| serde_json::from_str::<Value>(roles).is_err()),
        "the fixture holds a role list that is no JSON"
    );
    let (_dir, ledger) = named("t2175-release-roles.db.gz");
    assert_eq!(before, participants_roles(ledger.sqlite()));
    assert_eq!(events(&ledger, "member.unsettled").len(), 1);
}

fn participants_roles(db: &Connection) -> Vec<(i64, String)> {
    db.prepare("SELECT id, roles FROM participant ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// A known limit of main's, kept (`architecture/staff-slots.md` 9.6, the second problem): a
/// window whose conversation starts afresh is given the first task message it received,
/// whole. Here that is the reopen that carried the brief after the brief itself failed (T-2),
/// with the instruction it carried. A later fix of that turns this red on purpose.
#[test]
fn a_fresh_conversation_is_given_its_windows_first_message_whole() {
    let (_dir, ledger) = named("t2110-old-plain-reopen.db.gz");
    // Window 9's brief (m-2) failed; the reopen that carried it (m-14) was received.
    let first = ledger
        .first_received(9, 2)
        .unwrap()
        .expect("the first message it received");
    assert_eq!(first.message.id, 14);
    assert!(
        first
            .message
            .body
            .starts_with("Critical work: hard-problem. It changes no project file: return analysis, evidence and recommendations to the chief."),
        "whole, with the line it was saved with: {}",
        first.message.body
    );
    // A window that had its plain brief is given it again, as intended.
    let plain = ledger.first_received(8, 1).unwrap().expect("its brief");
    assert_eq!(plain.message.id, 1);
}
