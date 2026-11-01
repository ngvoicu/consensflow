//! The rules the human writes for a project's staff: set, changed and taken
//! away, at most 8,000 bytes and 150 lines of plain text, what each change logs
//! and tells the trace, that a refused one and one for a project that is not
//! there change nothing, that the view has them only while there are some, and
//! that a ledger at the version before them gains none in every project with
//! every row kept.

use cf_ledger::{migrate, ProjectView, MIGRATIONS};
use rusqlite::Connection;
use serde_json::json;

use super::migration::{populate, rows};
use super::world::{events_of, home, project, refusal, told_of, MINUTE, START};

const RULES: &str = "- Push to origin only.\n- Tests use the .smoke bundle id.";

fn key(view: &ProjectView) -> Option<serde_json::Value> {
    serde_json::to_value(view)
        .expect("the view as the page reads it")
        .get("staffRules")
        .cloned()
}

#[test]
fn the_rules_are_set_changed_and_taken_away_and_the_view_has_them_only_while_there_are_some() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    assert_eq!(app.staff_rules, None);
    assert_eq!(
        key(&app),
        None,
        "a project with none reads as it always did"
    );

    home.now.set(START + MINUTE);
    let set = home
        .ledger
        .set_staff_rules(app.id, Some(RULES))
        .expect("set");
    assert_eq!(set.staff_rules.as_deref(), Some(RULES));
    assert_eq!(key(&set), Some(json!(RULES)));
    assert_eq!(set.updated_at, "2026-10-13T12:01:00.000Z");
    assert_eq!(
        home.ledger.project(app.id).expect("read"),
        Some(set.clone())
    );
    let mut expected = app.clone();
    expected.staff_rules = set.staff_rules.clone();
    expected.updated_at = set.updated_at.clone();
    assert_eq!(set, expected, "the rest of the project stays");

    let changed = home
        .ledger
        .set_staff_rules(app.id, Some("\n  - Push to origin only.  \n\n"))
        .expect("changed");
    assert_eq!(
        changed.staff_rules.as_deref(),
        Some("- Push to origin only."),
        "kept trimmed"
    );
    let said = events_of(&home.ledger, app.id, "project.staff_rules");
    assert_eq!(
        said,
        [
            json!({ "from": null, "to": RULES }),
            json!({ "from": RULES, "to": "- Push to origin only." }),
        ]
    );
    assert_eq!(
        told_of(&home, "project.staff_rules"),
        [(app.id, said[0].clone()), (app.id, said[1].clone())],
        "the trace is told each one"
    );

    for away in [None, Some(""), Some(" \n\t ")] {
        home.ledger
            .set_staff_rules(app.id, Some(RULES))
            .expect("set again");
        let gone = home.ledger.set_staff_rules(app.id, away).expect("away");
        assert_eq!(gone.staff_rules, None, "{away:?}");
        assert_eq!(key(&gone), None, "{away:?}: the key is gone with them");
        assert_eq!(
            events_of(&home.ledger, app.id, "project.staff_rules").last(),
            Some(&json!({ "from": RULES, "to": null })),
            "{away:?}"
        );
    }
}

#[test]
fn the_same_rules_again_or_none_where_there_are_none_log_nothing_and_touch_nothing() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    home.now.set(START + MINUTE);
    for none in [None, Some(""), Some("   ")] {
        let view = home.ledger.set_staff_rules(app.id, none).expect("none");
        assert_eq!(view, app, "{none:?}: it had none");
    }
    home.ledger
        .set_staff_rules(app.id, Some(RULES))
        .expect("set");
    home.now.set(START + 2 * MINUTE);
    let kept = home
        .ledger
        .set_staff_rules(app.id, Some(&format!("  {RULES}\n")))
        .expect("kept");
    assert_eq!(kept.updated_at, "2026-10-13T12:01:00.000Z");
    assert_eq!(told_of(&home, "project.staff_rules").len(), 1);
}

#[test]
fn rules_over_the_bounds_or_with_a_control_character_are_refused_and_change_nothing() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    home.ledger
        .set_staff_rules(app.id, Some(RULES))
        .expect("set");
    let logged = home.told.borrow().len();
    for (text, words) in [
        (
            "a".repeat(8_001),
            "the rules for the staff are plain text: they are at most 8000 bytes, and these are 8001",
        ),
        (
            vec!["a rule"; 151].join("\n"),
            "the rules for the staff are plain text: they are at most 150 lines, and these are 151",
        ),
        (
            "a\r\nb".to_owned(),
            "the rules for the staff are plain text: they have no control characters but line breaks and tabs",
        ),
    ] {
        assert_eq!(
            refusal(home.ledger.set_staff_rules(app.id, Some(&text))),
            (Some("invalid-staff-rules"), words.to_owned())
        );
    }
    // Checked before the project is looked for.
    assert_eq!(
        refusal(home.ledger.set_staff_rules(99, Some(&"a".repeat(8_001)))).0,
        Some("invalid-staff-rules")
    );
    let still = home.ledger.project(app.id).expect("read").expect("there");
    assert_eq!(still.staff_rules.as_deref(), Some(RULES));
    assert_eq!(home.told.borrow().len(), logged, "a refusal logs nothing");
}

#[test]
fn rules_for_a_project_the_ledger_does_not_have_are_refused_and_one_projects_are_never_another_s() {
    let mut home = home();
    let (first, second) = (
        project(&mut home.ledger, "first"),
        project(&mut home.ledger, "second"),
    );
    let logged = home.told.borrow().len();
    for rules in [Some(RULES), None] {
        assert_eq!(
            refusal(home.ledger.set_staff_rules(99, rules)),
            (Some("unknown-project"), "no project 99".to_owned())
        );
    }
    assert_eq!(home.told.borrow().len(), logged);
    home.ledger
        .set_staff_rules(second.id, Some(RULES))
        .expect("set");
    let read = |id| home.ledger.project(id).expect("read").expect("there");
    assert_eq!(read(first.id).staff_rules, None);
    assert_eq!(read(second.id).staff_rules.as_deref(), Some(RULES));
    assert!(events_of(&home.ledger, first.id, "project.staff_rules").is_empty());
}

#[test]
fn a_ledger_at_the_version_before_gains_no_rules_in_every_project_and_keeps_every_row() {
    let at = MIGRATIONS
        .iter()
        .position(|sql| sql.contains("ADD COLUMN staff_rules"))
        .expect("a migration that adds the staff's rules");
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
            // The staff's rules, then the Claude members' compaction: both columns of
            // this migration.
            .map(|row| format!("{row}|Null|Null"))
            .collect::<Vec<_>>(),
        "each project's row, and no rules at its end"
    );
}
