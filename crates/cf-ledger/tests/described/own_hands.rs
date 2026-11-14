//! The own-hands setting must upgrade a real 0037 ledger without changing its rows.

use cf_ledger::{migrate, MIGRATIONS};
use rusqlite::Connection;
use serde_json::json;

use super::migration::{populate, rows};
use super::world::{events_of, home, project, MINUTE, START};

#[test]
fn migration_0038_keeps_the_0037_ledger_and_defaults_every_project_off() {
    assert!(
        MIGRATIONS.len() >= 38,
        "own-hands migration 0038 must exist"
    );
    let db = Connection::open_in_memory().unwrap();
    populate(&db, &MIGRATIONS[..37]);
    let tables = ["project", "participant", "event", "task", "message"];
    let before = tables.map(|table| rows(&db, &format!("SELECT * FROM {table} ORDER BY rowid")));
    migrate(&db, &MIGRATIONS[..38]).unwrap();
    migrate(&db, &MIGRATIONS[..38]).unwrap();
    let after = tables.map(|table| rows(&db, &format!("SELECT * FROM {table} ORDER BY rowid")));
    assert_eq!(&after[1..], &before[1..]);
    assert_eq!(
        after[0],
        before[0]
            .iter()
            .map(|row| format!("{row}|Integer(0)"))
            .collect::<Vec<_>>()
    );
    // A later backup migration must also leave the original rows and the
    // own-hands defaults intact; its three participant columns start null.
    migrate(&db, &MIGRATIONS).unwrap();
    let latest = tables.map(|table| rows(&db, &format!("SELECT * FROM {table} ORDER BY rowid")));
    assert_eq!(latest[0], after[0]);
    assert_eq!(
        latest[1],
        after[1]
            .iter()
            .map(|row| format!("{row}|Null|Null|Null"))
            .collect::<Vec<_>>()
    );
    assert_eq!(&latest[2..], &after[2..]);
    assert!(!db
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
}

#[test]
fn setter_changes_only_its_project_and_logs_each_change() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    let other = project(&mut home.ledger, "other");
    assert!(!app.own_hands);
    assert!(serde_json::to_value(&app)
        .unwrap()
        .get("ownHands")
        .is_none());
    assert_eq!(home.ledger.set_own_hands(app.id, false).unwrap(), app);
    home.now.set(START + MINUTE);
    let on = home.ledger.set_own_hands(app.id, true).unwrap();
    assert!(on.own_hands);
    assert_eq!(serde_json::to_value(&on).unwrap()["ownHands"], true);
    assert_eq!(home.ledger.project(app.id).unwrap(), Some(on.clone()));
    assert!(!home.ledger.project(other.id).unwrap().unwrap().own_hands);
    assert_eq!(home.ledger.set_own_hands(app.id, true).unwrap(), on);
    let off = home.ledger.set_own_hands(app.id, false).unwrap();
    assert!(!off.own_hands);
    assert!(serde_json::to_value(&off)
        .unwrap()
        .get("ownHands")
        .is_none());
    assert_eq!(
        events_of(&home.ledger, app.id, "project.own_hands"),
        [
            json!({"from": false, "to": true}),
            json!({"from": true, "to": false})
        ]
    );
    assert!(events_of(&home.ledger, other.id, "project.own_hands").is_empty());
    assert!(home.ledger.set_own_hands(99, true).is_err());
}
