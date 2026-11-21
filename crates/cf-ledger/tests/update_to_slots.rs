//! The update to the staff as slots (migration 0044, `architecture/staff-slots.md` section 3) on
//! ledgers of the version before: it changes members' tiers and nothing else, rests nobody, and
//! tells the human of a Critical member it moved, once.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Read;

use cf_ledger::{migrate, open_ledger, Ledger, Options, MIGRATIONS};
use rusqlite::Connection;
use serde_json::Value;

/// Every row of `table` in rowid order, each value as SQLite quotes it, but for `skip`.
fn rows(db: &Connection, table: &str, skip: &[&str]) -> Vec<Vec<String>> {
    let columns: Vec<String> = db
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
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
    db.prepare(&format!("SELECT {select} FROM \"{table}\" ORDER BY rowid"))
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

/// What the update must leave exactly as it was: every task, message, need and conversation,
/// and every participant but its tier (assignee, `member_id`, `backup_of` included).
fn kept(db: &Connection) -> Vec<Vec<Vec<String>>> {
    vec![
        rows(db, "task", &[]),
        rows(db, "message", &[]),
        rows(db, "task_need", &[]),
        rows(db, "conversation", &[]),
        rows(db, "participant", &["tier"]),
    ]
}

fn events_of(db: &Connection, kind: &str) -> Vec<Value> {
    db.prepare("SELECT data FROM event WHERE kind = ? ORDER BY id")
        .unwrap()
        .query_map([kind], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|data| serde_json::from_str(&data.unwrap()).unwrap())
        .collect()
}

fn tiers(db: &Connection) -> Vec<(String, Option<String>)> {
    db.prepare("SELECT handle, tier FROM participant ORDER BY id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn archive(name: &str) -> tempfile::TempDir {
    let bytes: &[u8] = match name {
        "reassign" => include_bytes!("../../cf-daemon/tests/goldens/t1813-alpha10/reassign.db.gz"),
        "backup" => include_bytes!("../../cf-daemon/tests/goldens/t1813-alpha10/backup.db.gz"),
        _ => panic!("unknown archive"),
    };
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut raw)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("consensflow.db"), raw).unwrap();
    dir
}

fn open(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("the update")
}

#[test]
fn the_archived_ledgers_of_alpha_10_are_taken_unchanged_and_every_row_is_as_before() {
    for name in ["reassign", "backup"] {
        let dir = archive(name);
        let before = {
            let db = Connection::open(dir.path().join("consensflow.db")).unwrap();
            let version: i64 = db
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(version, 40, "{name} is a schema-40 ledger");
            (kept(&db), tiers(&db), rows(&db, "event", &[]).len())
        };
        let mut ledger = open(&dir);
        let after = (
            kept(ledger.sqlite()),
            tiers(ledger.sqlite()),
            rows(ledger.sqlite(), "event", &[]).len(),
        );
        assert_eq!(before.0, after.0, "{name}: every row, compared whole");
        assert_eq!(
            before.1, after.1,
            "{name}: every worker is standard: no tier moves"
        );
        assert_eq!(
            before.2, after.2,
            "{name}: none is added: nothing was settled and nobody is told"
        );
        // The standard worker slot shows two holders, which both stand.
        let project = ledger.project(1).unwrap().unwrap();
        let workers: Vec<&str> = project
            .participants
            .iter()
            .filter(|p| {
                p.member_id.is_none()
                    && p.backup_of.is_none()
                    && p.roles.iter().any(|role| role == "worker")
            })
            .map(|p| p.handle.as_str())
            .collect();
        assert_eq!(workers.len(), 2, "{name}: {workers:?}");
        // The same T-1 then finishes in session 7: member 4's in one, backup 5's in the other.
        let owner: (i64, Option<i64>) = ledger
            .sqlite()
            .query_row(
                "SELECT s.id, s.member_id FROM task t JOIN participant s ON s.id = t.assignee_id
                  WHERE t.number = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(owner.0, 7, "{name}: T-1 is in session 7");
        let finished = ledger.record_result(1, 1, "The work is done").unwrap();
        assert_eq!(finished.task.state, "done", "{name}");
        assert_eq!(
            finished.task.assignee.as_deref().map(str::is_empty),
            Some(false)
        );
        let member = if name == "reassign" { 4 } else { 5 };
        assert_eq!(
            owner.1,
            Some(member),
            "{name}: session 7 is member {member}'s"
        );
    }
}

/// A ledger of the version before, with its rows written by SQL: this board's shape and the
/// cases the archives lack.
/// The ledger's file, what the update must keep of it, and every member's tier before it.
type Built = (
    tempfile::TempDir,
    Vec<Vec<Vec<String>>>,
    Vec<(String, Option<String>)>,
);

fn built() -> Built {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("consensflow.db");
    let db = Connection::open(&file).unwrap();
    migrate(&db, &MIGRATIONS[..43]).expect("the schema of the version before");
    db.execute_batch(
        "INSERT INTO project(id,directory,name,state,created_at,updated_at) VALUES
            (1,'/work/board','board','open','2026-11-01','2026-11-01'),
            (2,'/work/quiet','quiet','open','2026-11-02','2026-11-02');
         INSERT INTO participant(id,project_id,handle,role,roles,harness,tier,created_at) VALUES
            (1,1,'human','human','[]',NULL,NULL,'2026-11-01'),
            (2,1,'chief','chief','[]','claude-code',NULL,'2026-11-01'),
            (50,2,'human','human','[]',NULL,NULL,'2026-11-02'),
            (51,2,'chief','chief','[]','claude-code',NULL,'2026-11-02');
         -- a frontend designer, an architect, a researcher and an integrator, alone and mixed,
         -- at light, at NULL and at complex
         INSERT INTO participant(id,project_id,handle,role,roles,harness,tier,created_at) VALUES
            (10,1,'iris','frontend','[\"frontend\"]','claude-code',NULL,'2026-11-01'),
            (11,1,'daedalus','architect','[\"architect\"]','claude-code','light','2026-11-01'),
            (12,1,'argus','researcher','[\"researcher\"]','claude-code','complex','2026-11-01'),
            (13,1,'janus','integrator','[\"integrator\"]','claude-code',NULL,'2026-11-01'),
            (14,1,'themis','architect','[\"architect\",\"researcher\"]','claude-code',NULL,'2026-11-01'),
            (15,1,'hera','reviewer','[\"reviewer\",\"architect\"]','claude-code','light','2026-11-01'),
            (16,1,'zeus','worker','[\"worker\",\"frontend\"]','claude-code','standard','2026-11-01'),
            -- a critical worker alone, with a backup that has a window of its own
            (20,1,'ares','worker','[\"worker\"]','claude-code','critical','2026-11-01'),
            -- a critical worker and reviewer that is the only critical reviewer
            (21,1,'dion','worker','[\"worker\",\"reviewer\"]','claude-code','critical','2026-11-01'),
            -- two image designers, one with an image backup that holds work
            (30,1,'pygmalion','designer','[\"designer\"]','codex','critical','2026-11-01'),
            (31,1,'galatea','designer','[\"designer\"]','codex','light','2026-11-01'),
            -- a member that left
            (32,1,'departed','frontend','[\"frontend\"]','claude-code',NULL,'2026-10-01'),
            -- this board's shape (the other project): a critical architect and reviewer with a backup
            (40,2,'ate','architect','[\"architect\",\"reviewer\"]','claude-code','critical','2026-11-01'),
            (41,2,'quietly','worker','[\"worker\"]','claude-code','critical','2026-11-02');
         UPDATE participant SET left_at = '2026-10-05' WHERE id = 32;
         INSERT INTO participant(id,project_id,handle,role,roles,harness,tier,created_at,backup_of) VALUES
            (22,1,'luna','worker','[\"worker\"]','claude-code','critical','2026-11-01',20),
            (23,1,'sol','worker','[\"worker\",\"reviewer\"]','claude-code','critical','2026-11-01',21),
            (33,1,'pallas','designer','[\"designer\"]','codex','critical','2026-11-01',30),
            (42,2,'ate-backup','architect','[\"architect\",\"reviewer\"]','claude-code','critical','2026-11-01',40);
         INSERT INTO participant(id,project_id,handle,role,roles,harness,tier,created_at,member_id) VALUES
            (24,1,'luna-1','worker','[\"worker\"]','claude-code','critical','2026-11-01',22),
            (25,1,'ares-1','worker','[\"worker\"]','claude-code','critical','2026-11-01',20),
            (34,1,'pallas-1','designer','[\"designer\"]','codex','critical','2026-11-01',33);
         INSERT INTO task(id,project_id,number,title,body,requester_id,assignee_id,state,pool,tier,purpose,created_at,updated_at) VALUES
            (60,1,1,'Work','Work',2,24,'working','worker','critical','hard-problem','2026-11-01','2026-11-01'),
            (61,1,2,'Draw','Draw',2,34,'working','designer',NULL,NULL,'2026-11-01','2026-11-01'),
            (62,1,3,'Wait','Wait',2,NULL,'open','reviewer','standard',NULL,'2026-11-01','2026-11-01');
         INSERT INTO task_need(task_id,needs_id) VALUES (62,60);
         INSERT INTO conversation(id,participant_id,harness,started_at) VALUES (70,24,'claude-code','2026-11-01');",
    )
    .unwrap();
    let before = kept(&db);
    let tiers_before = tiers(&db);
    db.close().unwrap();
    (dir, before, tiers_before)
}

#[test]
fn a_built_ledger_gains_each_members_home_tier_and_moves_a_critical_coder_with_its_backup_and_windows(
) {
    let (dir, before, tiers_before) = built();
    let ledger = open(&dir);
    let db = ledger.sqlite();
    // Nothing but tiers moved: every task, message, need, conversation and participant (its
    // assignee, member, backup link) is as it was.
    let after = kept(db);
    for (at, (was, now)) in before.iter().zip(&after).enumerate() {
        // The only new rows are the human's note (and the event of it, apart): every row there
        // was is as it was, and a table that gains none has the same rows.
        assert_eq!(&now[..was.len()], &was[..], "table {at}");
        if at != 1 {
            assert_eq!(now.len(), was.len(), "table {at} gains no row");
        }
    }
    assert_eq!(
        after[1].len(),
        before[1].len() + 2,
        "a note for each of the two projects"
    );
    let now: std::collections::BTreeMap<String, Option<String>> = tiers(db).into_iter().collect();
    let was: std::collections::BTreeMap<String, Option<String>> =
        tiers_before.into_iter().collect();
    let tier = |name: &str| now[name].as_deref();
    // Step 1: a tierless member takes its home; a tier kept where its roles offer it.
    assert_eq!(tier("iris"), Some("standard"), "frontend, NULL");
    assert_eq!(
        tier("daedalus"),
        Some("complex"),
        "an architect's `light` was nobody's choice"
    );
    assert_eq!(tier("argus"), Some("complex"), "complex is kept");
    assert_eq!(tier("janus"), Some("standard"), "integrator, NULL");
    assert_eq!(
        tier("themis"),
        Some("complex"),
        "architect and researcher: the architect's home"
    );
    assert_eq!(
        tier("hera"),
        Some("light"),
        "a member with a role that had tiers keeps its tier"
    );
    assert_eq!(tier("zeus"), Some("standard"));
    // Step 2: a Critical member with a coding role moves to complex, with its backup and
    // the windows of both.
    for moved in ["ares", "luna", "luna-1", "ares-1", "dion", "sol"] {
        assert_eq!(tier(moved), Some("complex"), "{moved}");
    }
    assert_eq!(tier("quietly"), Some("complex"), "the other project's");
    // An image designer's row is left as it is, whatever tier an old Add member stored.
    for kept in ["pygmalion", "galatea", "pallas", "pallas-1", "departed"] {
        assert_eq!(now[kept], was[kept], "{kept}");
    }
    // A Critical architect and reviewer, with its backup, keeps its slots.
    assert_eq!(tier("ate"), Some("critical"));
    assert_eq!(tier("ate-backup"), Some("critical"));
    // The update is logged as `member.settled`, with the slots that emptied.
    let settled = events_of(db, "member.settled");
    let step2: Vec<&Value> = settled.iter().filter(|event| event["step"] == 2).collect();
    let emptied = |handle: &str| -> Value {
        step2
            .iter()
            .find(|event| event["handle"] == handle)
            .unwrap_or_else(|| panic!("no move of {handle}"))["emptied"]
            .clone()
    };
    assert_eq!(
        emptied("ares"),
        serde_json::json!([]),
        "a Critical worker alone empties none"
    );
    assert_eq!(
        emptied("dion"),
        serde_json::json!(["reviewer"]),
        "the only critical reviewer"
    );
    // The human has one note in each project where step 2 moved a member.
    let notes = |project: i64| -> Vec<String> {
        db.prepare(
            "SELECT m.body FROM message m JOIN participant p ON p.id = m.recipient_id
              WHERE m.project_id = ? AND p.role = 'human' AND m.kind = 'note' ORDER BY m.id",
        )
        .unwrap()
        .query_map([project], |row| row.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
    };
    let told = notes(1);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains("The update made @dion complex: a worker is never critical. The critical reviewer slot it held is empty now: add a critical reviewer under Project settings. Until then, critical reviews go to the nearest reviewer held."),
        "{}",
        told[0]
    );
    assert!(told[0].contains("The update made @ares complex: a worker is never critical."));
    assert_eq!(notes(2).len(), 1, "the other project too");
    drop(ledger);
    // A second open sends no note.
    let again = open(&dir);
    assert_eq!(notes_after(&again, 1), 1);
    assert_eq!(notes_after(&again, 2), 1);
}

fn notes_after(ledger: &Ledger, project: i64) -> i64 {
    ledger
        .sqlite()
        .query_row(
            "SELECT COUNT(*) FROM message m JOIN participant p ON p.id = m.recipient_id
              WHERE m.project_id = ? AND p.role = 'human' AND m.kind = 'note'",
            [project],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn a_task_a_critical_coder_held_stays_with_its_backup_and_the_backups_window_finishes_it() {
    let (dir, _before, _) = built();
    let mut ledger = open(&dir);
    // T-1 is Critical work with its purpose, in the window of the critical worker's backup,
    // which the update moved to complex: the task keeps its assignment, and finishes there.
    let finished = ledger
        .record_result(1, 1, "Done in the backup's window")
        .unwrap();
    assert_eq!(finished.task.state, "done");
    let window: String = ledger
        .sqlite()
        .query_row(
            "SELECT p.handle FROM task t JOIN participant p ON p.id = t.assignee_id WHERE t.number = 1 AND t.project_id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(window, "luna-1");
    // The backup, now at complex, may not take that Critical task anew: the sweep's guard.
    let luna: i64 = ledger
        .sqlite()
        .query_row(
            "SELECT id FROM participant WHERE handle = 'luna'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let refused = ledger.check_taker(1, 1, luna).unwrap_err();
    assert_eq!(refused.code(), Some("not-a-candidate"));
}
