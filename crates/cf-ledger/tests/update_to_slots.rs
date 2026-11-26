//! The update to the staff as slots (migration 0045, `architecture/staff-slots.md` section 3) on
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

/// The columns the later migration of the turn retries gives every task: this update does not
/// write them, and a ledger opened by this build gains them.
const TRIES: [&str; 3] = ["turn_retries", "retry_hold", "turn_retry_at"];

/// What the update must leave exactly as it was: every task, message, need and conversation,
/// and every participant but its tier (assignee, `member_id`, `backup_of` included) and the closed
/// form of an account hold (0048), which this update does not write and a ledger opened by this
/// build gains.
fn kept(db: &Connection) -> Vec<Vec<Vec<String>>> {
    vec![
        rows(db, "task", &TRIES),
        rows(db, "message", &[]),
        rows(db, "task_need", &[]),
        rows(db, "conversation", &[]),
        rows(db, "participant", &["tier", "out_form"]),
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
        // Written by alpha.13's own ledger (`fixtures/t2013-alpha13-writer.rs.txt`, run at
        // v3.0.7-alpha.13): what the update must carry on, as a person's board has it.
        "alpha13" => include_bytes!("fixtures/t2013-alpha13.db.gz"),
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
         -- a backup of ares that was replaced, and its window, closed: history
         INSERT INTO participant(id,project_id,handle,role,roles,harness,tier,created_at,backup_of,left_at) VALUES
            (26,1,'ares-old','worker','[\"worker\"]','claude-code','critical','2026-10-01',20,'2026-10-20'),
            (28,1,'iris-old','frontend','[\"frontend\"]','claude-code',NULL,'2026-10-01',10,'2026-10-20');
         INSERT INTO participant(id,project_id,handle,role,roles,harness,tier,created_at,member_id,left_at) VALUES
            (27,1,'ares-old-1','worker','[\"worker\"]','claude-code','critical','2026-10-01',26,'2026-10-20'),
            (29,1,'iris-old-1','frontend','[\"frontend\"]','claude-code',NULL,'2026-10-01',28,'2026-10-20');
         -- iris's current backup, and a window each of iris and of it open and one closed: the
         -- closed windows (the human's End session, at alpha.13's `light`) are history
         INSERT INTO participant(id,project_id,handle,role,roles,harness,tier,created_at,backup_of) VALUES
            (35,1,'irene','frontend','[\"frontend\"]','claude-code','light','2026-11-01',10);
         INSERT INTO participant(id,project_id,handle,role,roles,harness,tier,created_at,member_id,left_at) VALUES
            (36,1,'irene-1','frontend','[\"frontend\"]','claude-code','light','2026-11-01',35,NULL),
            (37,1,'irene-2','frontend','[\"frontend\"]','claude-code','light','2026-11-01',35,'2026-11-10'),
            (38,1,'iris-2','frontend','[\"frontend\"]','claude-code','light','2026-11-01',10,'2026-11-10'),
            (39,1,'iris-3','frontend','[\"frontend\"]','claude-code','light','2026-11-01',10,NULL);
         INSERT INTO task(id,project_id,number,title,body,requester_id,assignee_id,state,pool,tier,purpose,created_at,updated_at) VALUES
            (60,1,1,'Work','Work',2,24,'working','worker','critical','hard-problem','2026-11-01','2026-11-01'),
            (61,1,2,'Draw','Draw',2,34,'working','designer',NULL,NULL,'2026-11-01','2026-11-01'),
            (62,1,3,'Wait','Wait',2,NULL,'open','reviewer','standard',NULL,'2026-11-01','2026-11-01');
         INSERT INTO task_need(task_id,needs_id) VALUES (62,60);
         -- T-1's brief, queued for its window
         INSERT INTO message(id,project_id,recipient_id,sender_id,kind,task_id,body,state,attempts,created_at) VALUES
            (80,1,24,2,'task',60,'Work','queued',0,'2026-11-01');
         INSERT INTO conversation(id,participant_id,harness,started_at) VALUES (70,24,'claude-code','2026-11-01');",
    )
    .unwrap();
    let before = kept(&db);
    let tiers_before = tiers(&db);
    db.close().unwrap();
    (dir, before, tiers_before)
}

#[test]
fn a_built_ledger_gains_each_members_home_tier_and_moves_no_critical_member() {
    let (dir, before, tiers_before) = built();
    let ledger = open(&dir);
    let db = ledger.sqlite();
    // Nothing but tiers moved: every task, message, need, conversation and participant (its
    // assignee, member, backup link) is as it was.
    let after = kept(db);
    for (at, (was, now)) in before.iter().zip(&after).enumerate() {
        // Nothing is added: every row there was is as it was, and nobody is told.
        assert_eq!(now, was, "table {at}");
    }
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
    // A Critical member stays Critical whatever roles it holds, with its backup and the windows
    // of both: every role offers every tier (the owner, 2026-11-22).
    for kept in ["ares", "luna", "luna-1", "ares-1", "dion", "sol", "quietly"] {
        assert_eq!(tier(kept), Some("critical"), "{kept}");
    }
    // An image designer's row is left as it is, whatever tier an old Add member stored.
    for kept in ["pygmalion", "galatea", "pallas", "pallas-1", "departed"] {
        assert_eq!(now[kept], was[kept], "{kept}");
    }
    // A backup that was replaced and its closed window are history: the update leaves their
    // tier as it was, and the member's current backup and windows move.
    for history in ["ares-old", "ares-old-1"] {
        assert_eq!(now[history].as_deref(), Some("critical"), "{history}");
    }
    for history in ["iris-old", "iris-old-1"] {
        assert_eq!(
            now[history], None,
            "{history}: a tierless frontend backup that left"
        );
    }
    for kept in ["ares-old", "ares-old-1", "iris-old", "iris-old-1"] {
        assert_eq!(now[kept], was[kept], "{kept}");
    }
    // The member's and its current backup's open windows move with it; their closed windows,
    // ended by the human before the update, keep the tier they worked at.
    for moved in ["irene", "irene-1", "iris-3"] {
        assert_eq!(tier(moved), Some("standard"), "{moved}");
    }
    for closed in ["iris-2", "irene-2"] {
        assert_eq!(
            tier(closed),
            Some("light"),
            "{closed}: a closed window is history"
        );
    }
    // A Critical architect and reviewer, with its backup, keeps its slots.
    assert_eq!(tier("ate"), Some("critical"));
    assert_eq!(tier("ate-backup"), Some("critical"));
    // The update is logged as `member.settled` for step 1's moves alone, and tells nobody.
    let settled = events_of(db, "member.settled");
    assert!(!settled.is_empty() && settled.iter().all(|event| event["step"] == 1));
    assert_eq!(notes_after(&ledger, 1) + notes_after(&ledger, 2), 0);
    drop(ledger);
    let again = open(&dir);
    assert_eq!(notes_after(&again, 1) + notes_after(&again, 2), 0);
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
fn a_task_a_critical_coder_held_finishes_in_its_window_and_its_critical_backup_may_take_it_anew() {
    let (dir, _before, _) = built();
    let mut ledger = open(&dir);
    // T-1 is Critical work with its purpose, in the window of the critical worker's backup: the
    // task keeps its assignment, and finishes there.
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
    // The backup is still Critical: it may take that Critical task with its purpose anew.
    let luna: i64 = ledger
        .sqlite()
        .query_row(
            "SELECT id FROM participant WHERE handle = 'luna'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    ledger
        .check_taker(1, 1, luna)
        .expect("a critical backup takes critical work");
}

/// A question the moved backup's window asks, and the follow-up of the task it finishes: after
/// the update the chief's answer returns the task to the same window, and the follow-up is
/// bound to that window, as before the update.
#[test]
fn a_question_the_moved_backups_window_asked_is_answered_and_the_follow_up_goes_to_that_window() {
    use cf_ledger::{NewQuestion, NewTask};
    let (dir, _before, _) = built();
    let mut ledger = open(&dir);
    // The window takes its brief in, as it does after an update.
    ledger.begin_delivery(80).expect("its delivery begins");
    ledger
        .confirm_delivery(80, Some(&serde_json::json!({ "item": "i-1" })))
        .expect("its delivery is confirmed");
    let asked = ledger
        .ask(
            1,
            &NewQuestion {
                from: Some("luna-1".into()),
                to: "chief".into(),
                body: Some("Which format?".into()),
                task: Some(1),
                ..NewQuestion::default()
            },
        )
        .expect("the window asks");
    assert_eq!(
        ledger.task(1, 1).unwrap().unwrap().task.state,
        "waiting",
        "the task waits for the answer"
    );
    let chief: i64 = ledger
        .sqlite()
        .query_row(
            "SELECT id FROM participant WHERE project_id = 1 AND handle = 'chief'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    ledger
        .answer(asked.id, chief, Some(&serde_json::json!("JSON")), None)
        .expect("the answer");
    let resumed = ledger.task(1, 1).unwrap().unwrap().task;
    assert_eq!(resumed.state, "working", "the answer sets it going again");
    assert_eq!(
        resumed.assignee.as_deref(),
        Some("luna-1"),
        "in the same window"
    );
    ledger.record_result(1, 1, "Done").unwrap();
    ledger.accept_task(1, 1, "chief").unwrap();
    let follow = ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                after: Some(1),
                body: "Follow up".into(),
                ..NewTask::default()
            },
        )
        .expect("the follow-up")
        .task;
    assert_eq!(
        follow.assignee.as_deref(),
        Some("luna-1"),
        "bound to the window of the task it follows: {follow:?}"
    );
    // Its brief is delivered into that window, and the follow-up works there.
    let brief: i64 = ledger
        .sqlite()
        .query_row(
            "SELECT m.id FROM message m JOIN participant p ON p.id = m.recipient_id
              WHERE m.task_id = (SELECT id FROM task WHERE project_id = 1 AND number = ?)
                AND m.kind = 'task' AND m.state = 'queued' AND p.handle = 'luna-1'",
            [follow.number],
            |row| row.get(0),
        )
        .expect("the follow-up's brief, queued for luna-1");
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&serde_json::json!({ "item": "i-2" })))
        .expect("its delivery is confirmed");
    let follow = ledger.task(1, follow.number).unwrap().unwrap().task;
    assert_eq!(follow.state, "working", "{follow:?}");
    assert_eq!(follow.assignee.as_deref(), Some("luna-1"));
}

/// What alpha.13's own ledger wrote ([`archive`] "alpha13"), counted before the update, so that
/// a test over it cannot pass on a fixture that lost what it carries.
fn alpha13() -> tempfile::TempDir {
    let dir = archive("alpha13");
    let db = Connection::open(dir.path().join("consensflow.db")).unwrap();
    let count = |sql: &str| -> i64 { db.query_row(sql, [], |row| row.get(0)).unwrap() };
    assert_eq!(count("PRAGMA user_version"), 43, "a schema-43 ledger");
    assert_eq!(
        count("SELECT count(*) FROM message WHERE kind = 'question' AND state = 'queued'"),
        1,
        "the window's question, unanswered"
    );
    assert_eq!(
        count("SELECT count(*) FROM task WHERE project_id = 1 AND number = 2 AND state = 'open' AND assignee_id = 6"),
        1,
        "the follow-up, bound to the backup's window"
    );
    assert_eq!(
        count("SELECT count(*) FROM participant WHERE left_at IS NOT NULL"),
        2,
        "the two windows the human ended"
    );
    dir
}

fn handle_tier(ledger: &Ledger, id: i64) -> (String, Option<String>) {
    ledger
        .sqlite()
        .query_row(
            "SELECT handle, tier FROM participant WHERE id = ?",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

/// The update moves a member that had no tier to its home, with its current backup and the
/// windows of both that are open; the windows the human ended keep the tier they worked at.
#[test]
fn alpha_13s_closed_windows_keep_their_tier_and_the_open_ones_move_with_their_member() {
    let dir = alpha13();
    let ledger = open(&dir);
    // iris, its backup irene, and irene's open window (T-1's).
    for moved in [3, 5, 6] {
        let (handle, tier) = handle_tier(&ledger, moved);
        assert_eq!(tier.as_deref(), Some("standard"), "@{handle}");
    }
    // iris's window and irene's, each ended by the human after its task was accepted.
    for closed in [7, 8] {
        let (handle, tier) = handle_tier(&ledger, closed);
        assert_eq!(tier.as_deref(), Some("light"), "@{handle} is history");
    }
}

/// The window's question alpha.13 left unanswered and the follow-up it bound to that window
/// carry over the update. The question reaches the chief; its window's binding ended with the
/// engine that held it, so the answer is refused in the words alpha.13 itself gives on this same
/// file (`question-moved`, read with `t2013-alpha13-writer.rs.txt`'s `answer_on_alpha_13`). The
/// window then finishes T-1, and once it is accepted the follow-up's brief is delivered into that
/// same window and its work starts there.
#[test]
fn alpha_13s_question_and_bound_follow_up_go_on_in_the_same_window_through_delivery() {
    let dir = alpha13();
    let mut ledger = open(&dir);
    // The engine starts as after the update, and ends what the one before held.
    ledger.begin_question_engine();
    ledger.close_questions_of_ended_engine().unwrap();
    let window = handle_tier(&ledger, 6).0;
    assert_eq!(
        ledger.task(1, 1).unwrap().unwrap().task.assignee.as_deref(),
        Some(window.as_str())
    );
    let question = ledger.message(2).unwrap().unwrap();
    if question.state == "queued" {
        ledger
            .begin_delivery(2)
            .expect("the question's delivery begins");
        ledger
            .confirm_delivery(2, Some(&serde_json::json!({ "item": "i-q" })))
            .expect("the chief has it");
    }
    let refused = ledger
        .answer(2, 2, Some(&serde_json::json!("JSON")), None)
        .unwrap_err();
    match refused {
        cf_ledger::LedgerError::Refused(refusal) => {
            assert_eq!(refusal.code, "question-moved");
            assert_eq!(
                refusal.message,
                "T-1 is waiting with backup @irene of @iris. Use cf tell T-1 \"…\" for what you meant to answer.",
                "as alpha.13 says it"
            );
        }
        other => panic!("{other}"),
    }
    // The window finishes T-1; the chief accepts it.
    ledger.record_result(1, 1, "Done").unwrap();
    ledger.accept_task(1, 1, "chief").unwrap();
    // The follow-up's brief, queued for the same window, is delivered there, and it works.
    let follow = ledger.task(1, 2).unwrap().unwrap().task;
    assert_eq!(
        follow.assignee.as_deref(),
        Some(window.as_str()),
        "{follow:?}"
    );
    let brief: i64 = ledger
        .sqlite()
        .query_row(
            "SELECT m.id FROM message m JOIN task t ON t.id = m.task_id
              WHERE t.project_id = 1 AND t.number = 2 AND m.kind = 'task' AND m.state = 'queued'
                AND m.recipient_id = 6",
            [],
            |row| row.get(0),
        )
        .expect("the follow-up's brief, queued for the window");
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&serde_json::json!({ "item": "i-f" })))
        .expect("the window has it");
    let follow = ledger.task(1, 2).unwrap().unwrap().task;
    assert_eq!(follow.state, "working", "{follow:?}");
    assert_eq!(follow.assignee.as_deref(), Some(window.as_str()));
    assert_eq!(handle_tier(&ledger, 6).1.as_deref(), Some("standard"));
}

/// A Critical worker's brief alpha.13 wrote and had not handed over, queued for its window or
/// held at the human's gate, is delivered as it was saved: the update changes no message, so it
/// still leads with "It changes no project file" (`architecture/staff-slots.md` 9.6, a known
/// limit). A brief written after the update has the line of now.
#[test]
fn alpha_13s_undelivered_critical_worker_briefs_are_delivered_as_saved() {
    const BEFORE: &str =
        "It changes no project file: return analysis, evidence and recommendations to the chief.";
    let dir = alpha13();
    let mut ledger = open(&dir);
    let body = |ledger: &Ledger, id: i64| ledger.message(id).unwrap().unwrap();
    let saved = |purpose: &str, work: &str| format!("Critical work: {purpose}. {BEFORE}\n\n{work}");
    // T-5's delivered brief: history.
    assert_eq!(body(&ledger, 7).state, "delivered");
    // T-6's, queued: delivered byte for byte.
    ledger.begin_delivery(8).expect("its delivery begins");
    ledger
        .confirm_delivery(8, Some(&serde_json::json!({ "item": "i-1" })))
        .expect("delivered");
    let queued = body(&ledger, 8);
    assert_eq!(queued.state, "delivered");
    assert_eq!(queued.body, saved("hard-problem", "Modify a project file"));
    // The gated project's worker brief: the human passes it on, and it is delivered so.
    ledger.approve_message(9, "human").expect("passed on");
    ledger.begin_delivery(9).expect("its delivery begins");
    ledger
        .confirm_delivery(9, Some(&serde_json::json!({ "item": "i-2" })))
        .expect("delivered");
    let gated = body(&ledger, 9);
    assert_eq!(gated.state, "delivered");
    assert_eq!(gated.body, saved("hard-problem", "Modify a project file"));
    // The gated reviewer's: a reviewer changes no file.
    assert_eq!(
        body(&ledger, 10).body,
        saved("critical-review", "Review it")
    );
    // T-7, open: given now, briefed by the rule of now.
    let brief = ledger
        .assign_task(1, 7, 4)
        .expect("assigned")
        .message
        .expect("its brief");
    assert_eq!(
        brief.body,
        "Critical work: hard-problem.\n\nModify another file"
    );
}
