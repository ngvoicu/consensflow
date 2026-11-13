//! BKV-02: a backup and Vacation survive a ledger upgrade and preserve the staff's work.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_ledger::{migrate, MIGRATIONS};
use rusqlite::Connection;

#[test]
fn upgrading_main_keeps_projects_members_tasks_and_history_and_adds_the_marks() {
    let db = Connection::open_in_memory().unwrap();
    migrate(&db, &MIGRATIONS[..37]).expect("the schema of main");
    db.execute_batch(
        "INSERT INTO project(id,directory,name,state,created_at,updated_at)
             VALUES(10,'/app','App','open','2026-11-01','2026-11-01'),
                   (20,'/site','Site','suspended','2026-11-02','2026-11-02');
         INSERT INTO participant(id,project_id,handle,role,roles,harness,tier,created_at,former_handles)
             VALUES(30,10,'chief','chief','[]','codex',NULL,'2026-11-01','[]'),
                   (40,10,'worker','worker','[\"worker\"]','codex','standard','2026-11-01','[\"former\"]'),
                   (50,20,'other','worker','[\"worker\"]','pi','light','2026-11-02','[]');
         INSERT INTO participant(id,project_id,handle,role,roles,harness,tier,created_at,member_id)
             VALUES(41,10,'worker-amber-pine','worker','[\"worker\"]','codex','standard','2026-11-01',40);
         INSERT INTO task(id,project_id,number,title,body,requester_id,assignee_id,state,pool,tier,created_at,updated_at)
             VALUES(60,10,1,'Work','Work',30,41,'working','worker','standard','2026-11-01','2026-11-01');
         INSERT INTO conversation(id,participant_id,harness,started_at) VALUES(70,41,'codex','2026-11-01');
         INSERT INTO event(id,project_id,at,kind,data) VALUES(80,10,'2026-11-01','old','{\"kept\":true}');"
    ).unwrap();
    let before: (i64, i64, i64, i64, String) = db.query_row(
        "SELECT p.id, t.id, c.id, e.id, p.former_handles FROM participant p JOIN task t ON t.assignee_id=41 JOIN conversation c ON c.participant_id=41 JOIN event e ON e.project_id=p.project_id WHERE p.id=40",
        [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))
    ).unwrap();
    migrate(&db, &MIGRATIONS).expect("upgrade from today's main");
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert!(
        version >= 38,
        "the upgrade must add migration 0038; got {version}"
    );
    let after: (i64, i64, i64, i64, String) = db.query_row(
        "SELECT p.id, t.id, c.id, e.id, p.former_handles FROM participant p JOIN task t ON t.assignee_id=41 JOIN conversation c ON c.participant_id=41 JOIN event e ON e.project_id=p.project_id WHERE p.id=40",
        [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))
    ).unwrap();
    assert_eq!(before, after);
    let columns: Vec<String> = db
        .prepare("PRAGMA table_info(participant)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for field in ["backup_of", "vacation_since", "out_kind"] {
        assert!(
            columns.iter().any(|column| column == field),
            "missing {field}"
        );
    }
    let broken: i64 = db
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(broken, 0);
    let projects: i64 = db
        .query_row("SELECT COUNT(*) FROM project", [], |r| r.get(0))
        .unwrap();
    assert_eq!(projects, 2);
}

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options, ProjectView,
};
use serde_json::{json, Value};

fn member(model: &str, role: &str, tier: &str) -> NewMember {
    NewMember {
        harness: "codex".into(),
        model: Some(model.into()),
        effort: None,
        model_name: None,
        designer: false,
        roles: vec![role.into()],
        tier: tier.into(),
    }
}

fn project(ledger: &mut Ledger, staff: Vec<NewMember>) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/backup".into(),
            name: "backup".into(),
            chief: NewChief {
                harness: "codex".into(),
                model: None,
                effort: None,
                model_name: None,
            },
            staff,
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
            prompt_suggestion: false,
        })
        .expect("a project")
}

fn id(project: &ProjectView, handle: &str) -> i64 {
    project
        .participants
        .iter()
        .find(|p| p.handle == handle)
        .unwrap()
        .id
}

// Give main's schema just the columns under test. The migration test above independently
// proves they are added on upgrade; these tests exercise existing ledger reads and writes.
fn stage_columns(ledger: &Ledger) {
    let db = ledger.sqlite();
    let columns: Vec<String> = db
        .prepare("PRAGMA table_info(participant)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for (name, sql) in [
        (
            "backup_of",
            "ALTER TABLE participant ADD COLUMN backup_of INTEGER",
        ),
        (
            "vacation_since",
            "ALTER TABLE participant ADD COLUMN vacation_since TEXT",
        ),
        (
            "out_kind",
            "ALTER TABLE participant ADD COLUMN out_kind TEXT",
        ),
    ] {
        if !columns.iter().any(|column| column == name) {
            db.execute_batch(sql).unwrap();
        }
    }
}

fn work(role: &str, tier: &str) -> NewTask {
    NewTask {
        from: "chief".into(),
        pool: Some(role.into()),
        tier: Some(tier.into()),
        body: "Do the work".into(),
        ..NewTask::default()
    }
}

#[test]
fn candidates_give_each_primary_its_backup_and_both_marks() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(
        &mut ledger,
        vec![
            member("alpha", "worker", "standard"),
            member("beta", "worker", "standard"),
        ],
    );
    let (primary, backup) = (id(&p, "alpha"), id(&p, "beta"));
    stage_columns(&ledger);
    ledger.sqlite().execute("UPDATE participant SET out_until='9999-12-31T23:59:59.998Z', out_kind='account' WHERE id=?", [primary]).unwrap();
    ledger.sqlite().execute("UPDATE participant SET backup_of=?, vacation_since='2026-11-13T12:00:00.000Z' WHERE id=?", [primary,backup]).unwrap();
    let task = ledger
        .create_task(p.id, &work("worker", "standard"))
        .unwrap()
        .task;
    let candidates = ledger.candidates(p.id, task.number).unwrap();
    assert_eq!(
        candidates.len(),
        1,
        "a backup is not a second primary candidate"
    );
    let candidate = serde_json::to_value(&candidates[0]).unwrap();
    assert_eq!(candidate["outKind"], "account");
    assert_eq!(candidate["backup"]["id"], backup);
    assert_eq!(
        candidate["backup"]["vacationSince"],
        "2026-11-13T12:00:00.000Z"
    );
}

#[test]
fn a_backup_does_not_create_a_tier_but_a_primary_on_vacation_still_holds_its_tier() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(
        &mut ledger,
        vec![
            member("alpha", "worker", "standard"),
            member("beta", "reviewer", "complex"),
        ],
    );
    let (primary, backup) = (id(&p, "alpha"), id(&p, "beta"));
    stage_columns(&ledger);
    ledger
        .sqlite()
        .execute(
            "UPDATE participant SET vacation_since='2026-11-13T12:00:00.000Z' WHERE id=?",
            [primary],
        )
        .unwrap();
    ledger
        .sqlite()
        .execute(
            "UPDATE participant SET backup_of=? WHERE id=?",
            [primary, backup],
        )
        .unwrap();
    let worker = ledger
        .create_task(p.id, &work("worker", "standard"))
        .unwrap()
        .task;
    assert_eq!(worker.tier.as_deref(), Some("standard"));
    let review = ledger.create_task(p.id, &work("reviewer", "complex"));
    assert!(
        review.is_err(),
        "a backup alone cannot hold the reviewer tier"
    );
}

#[test]
fn vacation_refuses_a_new_assignment_without_changing_the_open_task() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    let primary = id(&p, "alpha");
    stage_columns(&ledger);
    ledger
        .sqlite()
        .execute(
            "UPDATE participant SET vacation_since='2026-11-13T12:00:00.000Z' WHERE id=?",
            [primary],
        )
        .unwrap();
    let task = ledger
        .create_task(p.id, &work("worker", "standard"))
        .unwrap()
        .task;
    let assigned = ledger.assign_task(p.id, task.number, primary);
    assert!(
        assigned.is_err(),
        "a member on Vacation takes no new assignment"
    );
    let after = ledger.task(p.id, task.number).unwrap().unwrap().task;
    assert_eq!((after.state.as_str(), after.assignee), ("open", None));
}

#[test]
fn staff_and_board_reads_name_the_link_and_vacation() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(
        &mut ledger,
        vec![
            member("alpha", "worker", "standard"),
            member("beta", "worker", "standard"),
        ],
    );
    let (primary, backup) = (id(&p, "alpha"), id(&p, "beta"));
    stage_columns(&ledger);
    ledger.sqlite().execute("UPDATE participant SET backup_of=?, vacation_since='2026-11-13T12:00:00.000Z' WHERE id=?", [primary,backup]).unwrap();
    let staff = serde_json::to_value(ledger.last_staff().unwrap()).unwrap();
    let row = staff
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["model"] == "beta")
        .unwrap();
    assert_eq!(row["backupOf"], primary);
    assert_eq!(row["vacationSince"], "2026-11-13T12:00:00.000Z");
    let board = serde_json::to_value(ledger.board(p.id).unwrap()).unwrap();
    let lane: &Value = board["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["participant"]["id"] == backup)
        .unwrap();
    assert_eq!(lane["participant"]["backupOf"], json!(primary));
    assert_eq!(
        lane["participant"]["vacationSince"],
        "2026-11-13T12:00:00.000Z"
    );
}

#[test]
fn a_primarys_roles_and_tier_follow_to_its_backup() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(
        &mut ledger,
        vec![
            member("alpha", "worker", "standard"),
            member("beta", "worker", "standard"),
        ],
    );
    let (primary, backup) = (id(&p, "alpha"), id(&p, "beta"));
    stage_columns(&ledger);
    ledger
        .sqlite()
        .execute(
            "UPDATE participant SET backup_of=? WHERE id=?",
            [primary, backup],
        )
        .unwrap();
    ledger
        .set_roles(p.id, "alpha", &["worker", "reviewer"], Some("complex"))
        .unwrap();
    let (roles, tier): (String, String) = ledger
        .sqlite()
        .query_row(
            "SELECT roles, tier FROM participant WHERE id=?",
            [backup],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&roles).unwrap(),
        json!(["worker", "reviewer"])
    );
    assert_eq!(tier, "complex");
}

#[test]
fn a_backup_of_an_untiered_member_keeps_the_null_tier_even_when_edited() {
    use cf_ledger::MemberEdit;
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    ledger
        .set_roles(p.id, "alpha", &["frontend"], None)
        .unwrap();
    // Older ledgers can contain a null tier for an untiered member.
    ledger
        .sqlite()
        .execute(
            "UPDATE participant SET tier = NULL WHERE id = ?",
            [id(&p, "alpha")],
        )
        .unwrap();
    let backup = ledger
        .add_backup(p.id, "alpha", &member("beta", "worker", "standard"))
        .unwrap();
    assert_eq!(
        backup.tier, None,
        "the backup takes its primary's null tier"
    );
    let edited = ledger
        .edit_member(
            p.id,
            &backup.handle,
            &MemberEdit {
                harness: "codex".into(),
                model: Some("gamma".into()),
                effort: None,
                model_name: None,
                tier: "light".into(),
            },
        )
        .expect("a backup of an untiered member can change model");
    assert_eq!(edited.tier, None);
}

#[test]
fn a_member_leaves_with_its_backup() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(
        &mut ledger,
        vec![
            member("alpha", "worker", "standard"),
            member("beta", "worker", "standard"),
        ],
    );
    let (primary, backup) = (id(&p, "alpha"), id(&p, "beta"));
    stage_columns(&ledger);
    ledger
        .sqlite()
        .execute(
            "UPDATE participant SET backup_of=? WHERE id=?",
            [primary, backup],
        )
        .unwrap();
    ledger.remove_member(p.id, "alpha").unwrap();
    let left: Option<String> = ledger
        .sqlite()
        .query_row(
            "SELECT left_at FROM participant WHERE id=?",
            [backup],
            |r| r.get(0),
        )
        .unwrap();
    assert!(left.is_some(), "the primary's backup leaves with it");
}

#[test]
fn adding_a_primary_never_rejoins_a_departed_backup_with_the_same_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(
        &mut ledger,
        vec![
            member("alpha", "worker", "standard"),
            member("beta", "worker", "standard"),
        ],
    );
    let (primary, backup) = (id(&p, "alpha"), id(&p, "beta"));
    stage_columns(&ledger);
    ledger
        .sqlite()
        .execute(
            "UPDATE participant SET backup_of=?, left_at='2026-11-13T12:00:00.000Z' WHERE id=?",
            [primary, backup],
        )
        .unwrap();
    let added = ledger
        .add_member(p.id, &member("beta", "worker", "standard"))
        .unwrap();
    assert_ne!(
        added.id, backup,
        "a former backup does not rejoin as a primary"
    );
}

#[test]
fn a_backup_cannot_be_repurposed_as_an_independent_member_by_editing_its_roles() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(
        &mut ledger,
        vec![
            member("alpha", "worker", "standard"),
            member("beta", "worker", "standard"),
        ],
    );
    let (primary, backup) = (id(&p, "alpha"), id(&p, "beta"));
    stage_columns(&ledger);
    ledger
        .sqlite()
        .execute(
            "UPDATE participant SET backup_of=? WHERE id=?",
            [primary, backup],
        )
        .unwrap();
    let changed = ledger.set_roles(p.id, "beta", &["reviewer"], None);
    assert!(changed.is_err(), "the backup's roles follow its member");
}

#[test]
fn a_live_member_cannot_have_two_live_backups() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(
        &mut ledger,
        vec![
            member("alpha", "worker", "standard"),
            member("beta", "worker", "standard"),
            member("gamma", "worker", "standard"),
        ],
    );
    let (primary, first, second) = (id(&p, "alpha"), id(&p, "beta"), id(&p, "gamma"));
    stage_columns(&ledger);
    ledger
        .sqlite()
        .execute(
            "UPDATE participant SET backup_of=? WHERE id=?",
            [primary, first],
        )
        .unwrap();
    let duplicate = ledger.sqlite().execute(
        "UPDATE participant SET backup_of=? WHERE id=?",
        [primary, second],
    );
    assert!(duplicate.is_err(), "a second live backup is refused");
}

#[test]
fn a_backup_cannot_back_itself_another_backup_or_form_a_ring() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(
        &mut ledger,
        vec![
            member("alpha", "worker", "standard"),
            member("beta", "worker", "standard"),
            member("gamma", "worker", "standard"),
        ],
    );
    let (primary, backup, third) = (id(&p, "alpha"), id(&p, "beta"), id(&p, "gamma"));
    stage_columns(&ledger);
    let itself = ledger.sqlite().execute(
        "UPDATE participant SET backup_of=? WHERE id=?",
        [primary, primary],
    );
    assert!(itself.is_err(), "a row cannot back itself");
    ledger
        .sqlite()
        .execute(
            "UPDATE participant SET backup_of=? WHERE id=?",
            [primary, backup],
        )
        .unwrap();
    let chain = ledger.sqlite().execute(
        "UPDATE participant SET backup_of=? WHERE id=?",
        [backup, third],
    );
    assert!(chain.is_err(), "a backup has no backup");
    let ring = ledger.sqlite().execute(
        "UPDATE participant SET backup_of=? WHERE id=?",
        [backup, primary],
    );
    assert!(
        ring.is_err(),
        "a primary cannot become the backup of its backup"
    );
}

#[test]
fn the_chief_and_a_session_cannot_be_backups_or_have_backups() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(
        &mut ledger,
        vec![
            member("alpha", "worker", "standard"),
            member("beta", "worker", "standard"),
        ],
    );
    let (primary, backup, chief) = (id(&p, "alpha"), id(&p, "beta"), id(&p, "chief"));
    let task = ledger
        .create_task(p.id, &work("worker", "standard"))
        .unwrap()
        .task;
    let session_handle = ledger
        .assign_task(p.id, task.number, primary)
        .unwrap()
        .task
        .assignee
        .unwrap();
    let session = id(&ledger.project(p.id).unwrap().unwrap(), &session_handle);
    stage_columns(&ledger);
    let chief_has = ledger.sqlite().execute(
        "UPDATE participant SET backup_of=? WHERE id=?",
        [chief, backup],
    );
    assert!(chief_has.is_err(), "the chief cannot have a backup");
    let chief_is = ledger.sqlite().execute(
        "UPDATE participant SET backup_of=? WHERE id=?",
        [primary, chief],
    );
    assert!(chief_is.is_err(), "the chief cannot be a backup");
    let session_has = ledger.sqlite().execute(
        "UPDATE participant SET backup_of=? WHERE id=?",
        [session, backup],
    );
    assert!(session_has.is_err(), "a session cannot have a backup");
    let session_is = ledger.sqlite().execute(
        "UPDATE participant SET backup_of=? WHERE id=?",
        [primary, session],
    );
    assert!(session_is.is_err(), "a session cannot be a backup");
}

#[test]
fn the_human_cannot_be_a_backup_or_have_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(
        &mut ledger,
        vec![
            member("alpha", "worker", "standard"),
            member("beta", "worker", "standard"),
        ],
    );
    let (primary, backup, human) = (id(&p, "alpha"), id(&p, "beta"), id(&p, "human"));
    let human_has = ledger.sqlite().execute(
        "UPDATE participant SET backup_of=? WHERE id=?",
        [human, backup],
    );
    assert!(human_has.is_err(), "the human cannot have a backup");
    let human_is = ledger.sqlite().execute(
        "UPDATE participant SET backup_of=? WHERE id=?",
        [primary, human],
    );
    assert!(human_is.is_err(), "the human cannot be a backup");
}

#[test]
fn a_backup_must_belong_to_the_same_project_as_a_live_member() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let first = project(
        &mut ledger,
        vec![
            member("alpha", "worker", "standard"),
            member("beta", "worker", "standard"),
        ],
    );
    let second = project(&mut ledger, vec![member("other", "worker", "standard")]);
    let (primary, backup, elsewhere) = (
        id(&first, "alpha"),
        id(&first, "beta"),
        id(&second, "other"),
    );
    stage_columns(&ledger);
    let across = ledger.sqlite().execute(
        "UPDATE participant SET backup_of=? WHERE id=?",
        [elsewhere, backup],
    );
    assert!(across.is_err(), "a backup cannot point across projects");
    ledger.remove_member(first.id, "alpha").unwrap();
    let departed = ledger.sqlite().execute(
        "UPDATE participant SET backup_of=? WHERE id=?",
        [primary, backup],
    );
    assert!(
        departed.is_err(),
        "a backup cannot point at a departed member"
    );
}

#[test]
fn a_backup_insert_cannot_link_across_projects_or_to_a_chief() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let first = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    let second = project(&mut ledger, vec![member("other", "worker", "standard")]);
    let sql =
        "INSERT INTO participant(project_id,handle,role,roles,harness,tier,created_at,backup_of)
               VALUES(?1,'rogue','worker','[\"worker\"]','codex','standard','2026-11-13',?2)";
    for invalid in [id(&second, "other"), id(&first, "chief")] {
        assert!(ledger.sqlite().execute(sql, [first.id, invalid]).is_err());
    }
    let inserted: i64 = ledger
        .sqlite()
        .query_row(
            "SELECT COUNT(*) FROM participant WHERE handle = 'rogue'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(inserted, 0);
}

#[test]
fn add_backup_keeps_a_separate_row_even_when_it_runs_the_same_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    let primary = id(&p, "alpha");
    let added = ledger.add_backup(p.id, "alpha", &member("alpha", "worker", "standard"));
    assert!(
        added.is_ok(),
        "adding a dedicated backup succeeds: {added:?}"
    );
    let backup = added.unwrap();
    assert_ne!(backup.id, primary);
    let owner: i64 = ledger
        .sqlite()
        .query_row(
            "SELECT backup_of FROM participant WHERE id=?",
            [backup.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(owner, primary);
    assert!(ledger
        .add_backup(p.id, "alpha", &member("beta", "worker", "standard"))
        .is_err());
    assert!(ledger
        .add_backup(p.id, "chief", &member("beta", "worker", "standard"))
        .is_err());
}

#[test]
fn each_harness_can_back_a_member_and_get_a_session_of_its_own() {
    for harness in ["claude-code", "codex", "pi", "devin", "muse"] {
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
        let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
        let mut config = member("beta", "worker", "standard");
        config.harness = harness.into();
        let backup = ledger.add_backup(p.id, "alpha", &config).unwrap();
        let task = ledger
            .create_task(p.id, &work("worker", "standard"))
            .unwrap()
            .task;
        let assigned = ledger
            .assign_task(p.id, task.number, backup.id)
            .unwrap()
            .task;
        let session = id(
            &ledger.project(p.id).unwrap().unwrap(),
            assigned.assignee.as_deref().unwrap(),
        );
        let (owner, kind): (i64, String) = ledger
            .sqlite()
            .query_row(
                "SELECT member_id, harness FROM participant WHERE id = ?",
                [session],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((owner, kind.as_str()), (backup.id, harness));
    }
}

#[test]
fn vacation_and_return_do_not_clear_or_shorten_a_quota_hold() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    let primary = id(&p, "alpha");
    ledger
        .mark_out(primary, "2099-01-01T00:00:00.000Z", "out of quota")
        .unwrap();
    let sent = ledger.send_on_vacation(p.id, "alpha", &[]);
    assert!(sent.is_ok(), "the member goes on Vacation: {sent:?}");
    let row: (Option<String>, Option<String>) = ledger
        .sqlite()
        .query_row(
            "SELECT vacation_since, out_until FROM participant WHERE id=?",
            [primary],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(row.0.is_some());
    assert_eq!(row.1.as_deref(), Some("2099-01-01T00:00:00.000Z"));
    let returned = ledger.bring_back(p.id, "alpha");
    assert!(
        returned.is_ok(),
        "the member returns from Vacation: {returned:?}"
    );
    let row: (Option<String>, Option<String>) = ledger
        .sqlite()
        .query_row(
            "SELECT vacation_since, out_until FROM participant WHERE id=?",
            [primary],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row.0, None);
    assert_eq!(row.1.as_deref(), Some("2099-01-01T00:00:00.000Z"));
}

#[test]
fn vacation_refuses_a_fresh_task_named_for_the_member_or_its_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    let task = ledger
        .create_task(p.id, &work("worker", "standard"))
        .unwrap()
        .task;
    let assigned = ledger
        .assign_task(p.id, task.number, id(&p, "alpha"))
        .unwrap()
        .task;
    let session = assigned.assignee.unwrap();
    ledger.fail_task(p.id, task.number, "finished").unwrap();
    ledger.send_on_vacation(p.id, "alpha", &[]).unwrap();
    for recipient in ["alpha", session.as_str()] {
        let fresh = NewTask {
            from: "chief".into(),
            to: Some(recipient.into()),
            body: "New work".into(),
            ..NewTask::default()
        };
        let refused = ledger.create_task(p.id, &fresh).unwrap_err();
        assert_eq!(refused.code(), Some("member-on-vacation"), "{recipient}");
    }
}

#[test]
fn vacation_returns_only_a_confirmed_unopened_windows_new_task_to_the_board() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    let primary = id(&p, "alpha");
    let first = ledger
        .create_task(p.id, &work("worker", "standard"))
        .unwrap()
        .task;
    let next = ledger
        .assign_task(p.id, first.number, primary)
        .unwrap()
        .task;
    let first_session = id(
        &ledger.project(p.id).unwrap().unwrap(),
        next.assignee.as_deref().unwrap(),
    );
    let second = ledger
        .create_task(p.id, &work("worker", "standard"))
        .unwrap()
        .task;
    let running = ledger
        .assign_task(p.id, second.number, primary)
        .unwrap()
        .task;
    let running_session = id(
        &ledger.project(p.id).unwrap().unwrap(),
        running.assignee.as_deref().unwrap(),
    );
    ledger.start_conversation(running_session, "codex").unwrap();
    let sent = ledger.send_on_vacation(p.id, "alpha", &[first_session]);
    assert!(
        sent.is_ok(),
        "Vacation releases an unopened window's task: {sent:?}"
    );
    let first = ledger.task(p.id, first.number).unwrap().unwrap().task;
    let second = ledger.task(p.id, second.number).unwrap().unwrap().task;
    assert_eq!((first.state.as_str(), first.assignee), ("open", None));
    assert_eq!(second.assignee.as_deref(), running.assignee.as_deref());
    assert_eq!(second.state, "queued");
}

#[test]
fn a_typed_busy_hold_survives_reopening_and_is_cleared_only_with_the_hold() {
    use cf_ledger::HoldKind;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ledger.db");
    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    let primary = id(&p, "alpha");
    let marked = ledger.mark_out_with_kind(
        primary,
        "2099-01-01T00:00:00.000Z",
        "server busy",
        HoldKind::Busy,
    );
    assert!(
        marked.is_ok(),
        "the busy hold is marked with its kind: {marked:?}"
    );
    ledger.close().unwrap();
    let mut ledger = open_ledger(&file, Options::default()).unwrap();
    let kind: Option<String> = ledger
        .sqlite()
        .query_row(
            "SELECT out_kind FROM participant WHERE id=?",
            [primary],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kind.as_deref(), Some("busy"));
    ledger.mark_back(primary, "renewed").unwrap();
    let row: (Option<String>, Option<String>) = ledger
        .sqlite()
        .query_row(
            "SELECT out_until, out_kind FROM participant WHERE id=?",
            [primary],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (None, None));
}

#[test]
fn switching_the_chief_clears_the_kind_with_the_old_hold() {
    use cf_ledger::{ChiefSwitch, HoldKind};
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    let chief = id(&p, "chief");
    ledger
        .mark_out_with_kind(chief, "2099-01-01T00:00:00.000Z", "busy", HoldKind::Busy)
        .unwrap();
    let switch =
        ChiefSwitch::from_json(&json!({"harness": "codex", "model": "gpt-6-luna"})).unwrap();
    ledger.switch_chief(p.id, &switch).unwrap();
    let marks: (Option<String>, Option<String>) = ledger
        .sqlite()
        .query_row(
            "SELECT out_until, out_kind FROM participant WHERE id = ?",
            [chief],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(marks, (None, None));
}

#[test]
fn copying_staff_rejects_a_link_to_an_absent_primary_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let before = ledger.projects().unwrap().len();
    let request = json!({
        "directory": "/work/copy", "name": "copy", "chief": {"harness": "codex"},
        "staff": [{"id": 2, "backupOf": 999, "harness": "codex", "model": "beta", "roles": ["worker"], "tier": "standard"}]
    });
    assert!(ledger.create_project_with_staff(&request).is_err());
    assert_eq!(ledger.projects().unwrap().len(), before);
}

#[test]
fn changing_a_backup_leaves_its_sessions_and_tasks_on_the_old_row() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    let owner = id(&p, "alpha");
    let old = ledger
        .add_backup(p.id, "alpha", &member("beta", "worker", "standard"))
        .unwrap();
    let task = ledger
        .create_task(p.id, &work("worker", "standard"))
        .unwrap()
        .task;
    let assigned = ledger.assign_task(p.id, task.number, old.id).unwrap().task;
    let changed = ledger.change_backup(p.id, "alpha", &member("gamma", "worker", "standard"));
    assert!(changed.is_ok(), "replacing a backup succeeds: {changed:?}");
    let new = changed.unwrap();
    assert_ne!(new.id, old.id);
    let old_left: Option<String> = ledger
        .sqlite()
        .query_row(
            "SELECT left_at FROM participant WHERE id=?",
            [old.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(old_left.is_some());
    let link: i64 = ledger
        .sqlite()
        .query_row(
            "SELECT backup_of FROM participant WHERE id=?",
            [new.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(link, owner);
    let still = ledger.task(p.id, task.number).unwrap().unwrap().task;
    assert_eq!(still.assignee, assigned.assignee);
    assert_eq!(still.state, assigned.state);
    let mut bad = member("invalid", "worker", "standard");
    bad.harness = "no-such-harness".into();
    assert!(ledger.change_backup(p.id, "alpha", &bad).is_err());
    let active: i64 = ledger
        .sqlite()
        .query_row(
            "SELECT id FROM participant WHERE backup_of=? AND left_at IS NULL",
            [owner],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active, new.id, "a failed replacement is atomic");
}

#[test]
fn removing_a_backup_keeps_its_work_and_readding_does_not_reuse_its_row() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    let old = ledger
        .add_backup(p.id, "alpha", &member("beta", "worker", "standard"))
        .unwrap();
    let task = ledger
        .create_task(p.id, &work("worker", "standard"))
        .unwrap()
        .task;
    let assigned = ledger.assign_task(p.id, task.number, old.id).unwrap().task;
    let removed = ledger.remove_backup(p.id, "alpha");
    assert!(removed.is_ok(), "removing the backup succeeds: {removed:?}");
    let still = ledger.task(p.id, task.number).unwrap().unwrap().task;
    assert_eq!(
        (still.assignee, still.state),
        (assigned.assignee, assigned.state)
    );
    let new = ledger
        .add_backup(p.id, "alpha", &member("beta", "worker", "standard"))
        .unwrap();
    assert_ne!(new.id, old.id);
}

#[test]
fn a_reopen_and_a_follow_up_stay_in_their_session_on_vacation() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    let primary = id(&p, "alpha");
    let task = ledger
        .create_task(p.id, &work("worker", "standard"))
        .unwrap()
        .task;
    let assigned = ledger.assign_task(p.id, task.number, primary).unwrap().task;
    let session_handle = assigned.assignee.unwrap();
    let session = id(&ledger.project(p.id).unwrap().unwrap(), &session_handle);
    ledger
        .fail_task(p.id, task.number, "window did not start")
        .unwrap();
    ledger
        .reopen_task(p.id, task.number, "chief", "Continue")
        .unwrap();
    ledger.send_on_vacation(p.id, "alpha", &[session]).unwrap();
    let reopened = ledger.task(p.id, task.number).unwrap().unwrap().task;
    assert_eq!(
        (reopened.state.as_str(), reopened.assignee.as_deref()),
        ("queued", Some(session_handle.as_str()))
    );
    ledger
        .fail_task(p.id, task.number, "window still did not start")
        .unwrap();
    let follow = ledger
        .create_task(
            p.id,
            &NewTask {
                from: "chief".into(),
                after: Some(task.number),
                body: "Follow up".into(),
                ..NewTask::default()
            },
        )
        .unwrap()
        .task;
    assert_eq!(
        (follow.state.as_str(), follow.assignee.as_deref()),
        ("queued", Some(session_handle.as_str()))
    );
    ledger.send_on_vacation(p.id, "alpha", &[session]).unwrap();
    let still = ledger.task(p.id, follow.number).unwrap().unwrap().task;
    assert_eq!(
        (still.state.as_str(), still.assignee.as_deref()),
        ("queued", Some(session_handle.as_str()))
    );
}

#[test]
fn a_new_project_can_copy_the_last_staffs_backup_and_vacation_without_copying_old_ids() {
    let dir = tempfile::tempdir().unwrap();
    let mut ledger = open_ledger(&dir.path().join("ledger.db"), Options::default()).unwrap();
    let p = project(&mut ledger, vec![member("alpha", "worker", "standard")]);
    let owner = id(&p, "alpha");
    let backup = ledger
        .add_backup(p.id, "alpha", &member("beta", "worker", "standard"))
        .unwrap();
    ledger.send_on_vacation(p.id, &backup.handle, &[]).unwrap();
    let staff = serde_json::to_value(ledger.last_staff().unwrap()).unwrap();
    let opened = ledger.create_project_with_staff(&json!({
        "directory": "/work/copy", "name": "copy", "chief": {"harness": "codex"}, "staff": staff
    }));
    assert!(
        opened.is_ok(),
        "a project can copy its previous staff: {opened:?}"
    );
    let opened = opened.unwrap();
    let new_owner = id(&opened, "alpha");
    let new_backup = id(&opened, "beta");
    assert_ne!(new_owner, owner);
    assert_ne!(new_backup, backup.id);
    let (link, vacation): (Option<i64>, Option<String>) = ledger
        .sqlite()
        .query_row(
            "SELECT backup_of, vacation_since FROM participant WHERE id=?",
            [new_backup],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(link, Some(new_owner));
    assert!(vacation.is_some());
    assert_eq!(
        opened
            .participants
            .iter()
            .filter(|p| p.role == "worker")
            .count(),
        2
    );
}
