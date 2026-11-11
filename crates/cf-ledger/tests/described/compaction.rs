//! The size the human chooses for the project's member windows to compact at,
//! whatever their harness: set, changed and taken away, from 200,000 to 1,000,000 tokens, what each
//! change logs and tells the trace, that a refused one and one for a project that
//! is not there change nothing, that the view has it only while there is one, and
//! that a ledger at the version before gains none in every project with every row
//! kept.

use cf_ledger::{migrate, ProjectView, MIGRATIONS};
use rusqlite::Connection;
use serde_json::json;

use super::migration::{populate, rows};
use super::world::{events_of, home, project, refusal, told_of, MINUTE, START};

fn key(view: &ProjectView) -> Option<serde_json::Value> {
    serde_json::to_value(view)
        .expect("the view as the page reads it")
        .get("compaction")
        .cloned()
}

#[test]
fn the_size_is_set_changed_and_taken_away_and_the_view_has_it_only_while_there_is_one() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    assert_eq!(app.compaction, None);
    assert_eq!(
        key(&app),
        None,
        "a project on each harness's own point reads as it always did"
    );

    home.now.set(START + MINUTE);
    let set = home
        .ledger
        .set_compaction(app.id, Some(400_000))
        .expect("set");
    assert_eq!(set.compaction, Some(400_000));
    assert_eq!(key(&set), Some(json!(400_000)));
    assert_eq!(set.updated_at, "2026-10-13T12:01:00.000Z");
    assert_eq!(
        home.ledger.project(app.id).expect("read"),
        Some(set.clone())
    );
    let mut expected = app.clone();
    expected.compaction = Some(400_000);
    expected.updated_at = set.updated_at.clone();
    assert_eq!(set, expected, "the rest of the project stays");

    let changed = home
        .ledger
        .set_compaction(app.id, Some(1_000_000))
        .expect("changed");
    assert_eq!(changed.compaction, Some(1_000_000));
    let said = events_of(&home.ledger, app.id, "project.compaction");
    assert_eq!(
        said,
        [
            json!({ "from": null, "to": 400_000 }),
            json!({ "from": 400_000, "to": 1_000_000 }),
        ]
    );
    assert_eq!(
        told_of(&home, "project.compaction"),
        [(app.id, said[0].clone()), (app.id, said[1].clone())],
        "the trace is told each one"
    );

    let gone = home.ledger.set_compaction(app.id, None).expect("away");
    assert_eq!(gone.compaction, None);
    assert_eq!(key(&gone), None, "the key is gone with it");
    assert_eq!(
        events_of(&home.ledger, app.id, "project.compaction").last(),
        Some(&json!({ "from": 1_000_000, "to": null }))
    );
}

#[test]
fn the_same_size_again_or_none_where_there_is_none_log_nothing_and_touch_nothing() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    home.now.set(START + MINUTE);
    let view = home.ledger.set_compaction(app.id, None).expect("none");
    assert_eq!(view, app, "it had none");
    home.ledger
        .set_compaction(app.id, Some(400_000))
        .expect("set");
    home.now.set(START + 2 * MINUTE);
    let kept = home
        .ledger
        .set_compaction(app.id, Some(400_000))
        .expect("kept");
    assert_eq!(kept.updated_at, "2026-10-13T12:01:00.000Z");
    assert_eq!(told_of(&home, "project.compaction").len(), 1);
}

#[test]
fn a_size_outside_the_range_is_refused_and_changes_nothing() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    home.ledger
        .set_compaction(app.id, Some(400_000))
        .expect("set");
    let logged = home.told.borrow().len();
    for tokens in [0, 99_999, 100_000, 199_999, 1_000_001, u32::MAX] {
        assert_eq!(
            refusal(home.ledger.set_compaction(app.id, Some(tokens))),
            (
                Some("invalid-compaction"),
                "the compaction size is a whole number of tokens from 200000 to 1000000, or none for each harness's own point".to_owned()
            ),
            "{tokens}"
        );
    }
    let still = home.ledger.project(app.id).expect("read").expect("there");
    assert_eq!(still.compaction, Some(400_000));
    assert_eq!(home.told.borrow().len(), logged, "a refusal logs nothing");
}

#[test]
fn a_size_for_a_project_the_ledger_does_not_have_is_refused_and_one_projects_is_never_another_s() {
    let mut home = home();
    let (first, second) = (
        project(&mut home.ledger, "first"),
        project(&mut home.ledger, "second"),
    );
    let logged = home.told.borrow().len();
    for size in [Some(400_000), None] {
        assert_eq!(
            refusal(home.ledger.set_compaction(99, size)),
            (Some("unknown-project"), "no project 99".to_owned())
        );
    }
    assert_eq!(home.told.borrow().len(), logged);
    home.ledger
        .set_compaction(second.id, Some(400_000))
        .expect("set");
    let read = |id| home.ledger.project(id).expect("read").expect("there");
    assert_eq!(read(first.id).compaction, None);
    assert_eq!(read(second.id).compaction, Some(400_000));
    assert!(events_of(&home.ledger, first.id, "project.compaction").is_empty());
}

#[test]
fn a_ledger_at_the_version_before_gains_no_size_in_every_project_and_keeps_every_row() {
    let at = MIGRATIONS
        .iter()
        .position(|sql| sql.contains("ADD COLUMN claude_compaction"))
        .expect("the migration that added the Claude members' compaction (0026)");
    let db = Connection::open_in_memory().expect("a database");
    populate(&db, &MIGRATIONS[..at]);
    let every = |db: &Connection| {
        ["project", "participant", "event", "task", "message"]
            .map(|table| rows(db, &format!("SELECT * FROM {table} ORDER BY rowid")))
    };
    let before = every(&db);
    migrate(&db, &MIGRATIONS[..=at]).expect("migrated");
    let after = every(&db);
    assert_eq!(after[1..], before[1..], "the other tables' rows");
    assert_eq!(
        after[0],
        before[0]
            .iter()
            // The staff's rules, then the size: both columns of this migration.
            .map(|row| format!("{row}|Null|Null"))
            .collect::<Vec<_>>(),
        "each project's row, and no rules and no size at its end"
    );
}

/// Migration 0034 (smaller-contexts, Phase 7): the size a project chose for its Claude members
/// is its members' size, whatever their harness, under the column's new name; every other
/// value of every row is kept, and the project that chose none still has none.
#[test]
fn a_ledger_at_the_version_before_the_rename_keeps_each_projects_size_as_its_members_size() {
    let at = MIGRATIONS
        .iter()
        .position(|sql| sql.contains("RENAME COLUMN claude_compaction TO compaction"))
        .expect("the migration that renames the column (0034)");
    let db = Connection::open_in_memory().expect("a database");
    populate(&db, &MIGRATIONS[..at]);
    db.execute_batch("UPDATE project SET claude_compaction = 400000 WHERE id = 1")
        .expect("sizes chosen before");
    let every = |db: &Connection| {
        ["project", "participant", "event", "task", "message"]
            .map(|table| rows(db, &format!("SELECT * FROM {table} ORDER BY rowid")))
    };
    let before = every(&db);
    migrate(&db, &MIGRATIONS[..=at]).expect("migrated");
    assert_eq!(every(&db), before, "every value of every row, in its place");
    let sizes: Vec<(i64, Option<i64>)> = db
        .prepare("SELECT id, compaction FROM project ORDER BY id")
        .expect("the column by its new name")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("read")
        .map(Result::unwrap)
        .collect();
    assert_eq!(sizes, [(1, Some(400_000)), (2, None)]);
}
