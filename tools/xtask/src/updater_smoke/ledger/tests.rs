use std::fs;

use super::*;

const SCHEMA: &str = "
    CREATE TABLE project (id INTEGER PRIMARY KEY, directory TEXT NOT NULL, name TEXT NOT NULL, state TEXT NOT NULL, updated_at TEXT NOT NULL);
    CREATE TABLE participant (id INTEGER PRIMARY KEY, project_id INTEGER NOT NULL REFERENCES project (id), handle TEXT NOT NULL, left_at TEXT);
    CREATE TABLE event (id INTEGER PRIMARY KEY, project_id INTEGER NOT NULL REFERENCES project (id), at TEXT NOT NULL, kind TEXT NOT NULL, data TEXT NOT NULL);
";

/// The rows a run of the smoke leaves.
const ROWS: [&str; 3] = [
    "INSERT INTO project VALUES (1, '/w/a', 'a', 'open', 't1'), (2, '/w/b', 'b', 'open', 't1')",
    "INSERT INTO participant VALUES (1, 1, 'chief', NULL), (2, 2, 'chief', NULL)",
    "INSERT INTO event VALUES (1, 1, 't1', 'project.created', '{\"name\":\"a\"}'), (2, 2, 't2', 'project.created', '{\"name\":\"b\"}')",
];

/// A ledger with the tables the smoke reads at the schema `version`, and the `rows`.
fn ledger_file(folder: &Path, version: i64, rows: &[&str]) -> PathBuf {
    let file = folder.join("consensflow.db");
    let db = Connection::open(&file).unwrap();
    db.execute_batch(SCHEMA).unwrap();
    db.execute_batch(&format!("PRAGMA user_version = {version};"))
        .unwrap();
    for sql in rows {
        db.execute_batch(sql).unwrap();
    }
    file
}

/// The ledger as `statements` leave it, then read.
fn changed(file: &Path, statements: &[&str]) -> Ledger {
    let db = Connection::open(file).unwrap();
    for sql in statements {
        db.execute_batch(sql).unwrap();
    }
    drop(db);
    read_ledger(file).unwrap()
}

/// What a check that refused said.
fn refusal(result: Result) -> String {
    result.unwrap_err().to_string()
}

#[test]
fn reads_every_table_with_its_rows_the_schemas_version_and_what_sqlite_says_of_the_file() {
    let folder = tempfile::tempdir().unwrap();
    let file = ledger_file(folder.path(), 10, &ROWS);
    let ledger = read_ledger(&file).unwrap();
    assert_eq!(ledger.version, 10);
    assert_eq!(ledger.integrity, ["ok"]);
    assert!(ledger.references.is_empty());
    assert_eq!(
        ledger.tables.keys().collect::<Vec<_>>(),
        ["event", "participant", "project"]
    );
    let projects = &ledger.tables["project"];
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].get("id"), Some(&Cell::Integer(1)));
    assert_eq!(
        projects[0].get("directory"),
        Some(&Cell::Text("/w/a".into()))
    );
    assert_eq!(
        ledger.tables["participant"][0].get("left_at"),
        Some(&Cell::Null)
    );
    assert_eq!(projects[0].get("nothing"), None);
}

#[test]
fn reads_every_kind_of_cell_a_column_can_hold() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("kinds.db");
    Connection::open(&file)
        .unwrap()
        .execute_batch(
            "CREATE TABLE \"odd name\" (a, b, c, d, e);
             INSERT INTO \"odd name\" VALUES (NULL, 7, 1.5, 'text', x'0102');",
        )
        .unwrap();
    let ledger = read_ledger(&file).unwrap();
    let row = &ledger.tables["odd name"][0];
    assert_eq!(
        ["a", "b", "c", "d", "e"].map(|column| row.get(column).cloned().unwrap()),
        [
            Cell::Null,
            Cell::Integer(7),
            Cell::Real(1.5),
            Cell::Text("text".into()),
            Cell::Blob(vec![1, 2])
        ]
    );
    assert_eq!(
        row.json().to_string(),
        r#"{"a":null,"b":7,"c":1.5,"d":"text","e":[1,2]}"#
    );
}

#[test]
fn a_file_that_is_no_ledger_is_said_so() {
    let folder = tempfile::tempdir().unwrap();
    let said = read_ledger(&folder.path().join("nowhere.db"))
        .unwrap_err()
        .to_string();
    assert!(said.starts_with("could not read the ledger "), "{said}");
    let garbage = folder.path().join("garbage.db");
    fs::write(
        &garbage,
        "this is not a database file at all, not even close to one",
    )
    .unwrap();
    assert!(read_ledger(&garbage).is_err());
}

#[test]
fn is_whole_sound_its_projects_there_and_nothing_it_held_lost_or_changed() {
    let folder = tempfile::tempdir().unwrap();
    let file = ledger_file(folder.path(), 10, &ROWS);
    let before = read_ledger(&file).unwrap();
    assert_sound(&before, 10).unwrap();
    assert_projects(&before, &[PathBuf::from("/w/a"), PathBuf::from("/w/b")]).unwrap();
    let after = changed(
        &file,
        &[
            // What a restart rewrites by design.
            "UPDATE project SET state = 'suspended', updated_at = 't9'",
            "UPDATE participant SET left_at = 't9'",
            "INSERT INTO event VALUES (3, 1, 't9', 'project.state', '{}')",
            "PRAGMA user_version = 11",
        ],
    );
    assert_kept(&before, &after).unwrap();
}

#[test]
fn is_no_longer_whole_when_a_row_is_gone_a_stable_column_changed_a_table_lost_or_the_schema_went_back(
) {
    let lost: [(&str, &[&str], &str); 5] = [
        (
            "a project is gone",
            &[
                "DELETE FROM event WHERE project_id = 2",
                "DELETE FROM participant WHERE project_id = 2",
                "DELETE FROM project WHERE id = 2",
            ],
            "2 is gone",
        ),
        (
            "a project's directory changed",
            &["UPDATE project SET directory = '/w/other' WHERE id = 1"],
            "project 1: directory was \"/w/a\" and is \"/w/other\"",
        ),
        (
            "an event was rewritten",
            &["UPDATE event SET data = '{}' WHERE id = 1"],
            "event 1: data was \"{\\\"name\\\":\\\"a\\\"}\" and is \"{}\"",
        ),
        (
            "a table is gone",
            &["DROP TABLE participant"],
            "lost its participant table",
        ),
        (
            "the schema went back",
            &["PRAGMA user_version = 9"],
            "schema is at 9, below 10",
        ),
    ];
    for (what, statements, words) in lost {
        let folder = tempfile::tempdir().unwrap();
        let file = ledger_file(folder.path(), 10, &ROWS);
        let before = read_ledger(&file).unwrap();
        let said = refusal(assert_kept(&before, &changed(&file, statements)));
        assert!(said.contains(words), "{what}: {said}");
    }
}

#[test]
fn says_the_row_that_is_gone_whole() {
    let folder = tempfile::tempdir().unwrap();
    let file = ledger_file(folder.path(), 10, &ROWS);
    let before = read_ledger(&file).unwrap();
    let after = changed(
        &file,
        &[
            "DELETE FROM event WHERE project_id = 2",
            "DELETE FROM participant WHERE project_id = 2",
            "DELETE FROM project WHERE id = 2",
        ],
    );
    // The tables are read in order of their names: the event goes first.
    let said = refusal(assert_kept(&before, &after));
    assert_eq!(
        said,
        "event 2 is gone: {\"id\":2,\"project_id\":2,\"at\":\"t2\",\"kind\":\"project.created\",\"data\":\"{\\\"name\\\":\\\"b\\\"}\"}"
    );
}

#[test]
fn takes_a_row_of_a_table_with_no_id_by_what_it_holds() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("links.db");
    let db = Connection::open(&file).unwrap();
    db.execute_batch(
        "CREATE TABLE task_need (task_id INTEGER NOT NULL, needs_id INTEGER NOT NULL);
         INSERT INTO task_need VALUES (1, 2), (3, 4);
         PRAGMA user_version = 4;",
    )
    .unwrap();
    drop(db);
    let before = read_ledger(&file).unwrap();
    assert_kept(&before, &before).unwrap();
    let after = changed(&file, &["DELETE FROM task_need WHERE task_id = 3"]);
    assert!(refusal(assert_kept(&before, &after)).contains("task_need undefined is gone"));
}

#[test]
fn is_not_sound_when_its_schema_is_below_the_older_daemons_a_reference_is_broken_or_it_holds_no_table(
) {
    let folder = tempfile::tempdir().unwrap();
    let file = ledger_file(folder.path(), 9, &ROWS);
    assert!(refusal(assert_sound(&read_ledger(&file).unwrap(), 10)).contains("below 10"));

    let folder = tempfile::tempdir().unwrap();
    let file = ledger_file(folder.path(), 10, &ROWS);
    let orphan = changed(
        &file,
        &[
            "PRAGMA foreign_keys = OFF",
            "INSERT INTO event VALUES (9, 77, 't', 'x', '{}')",
        ],
    );
    assert!(refusal(assert_sound(&orphan, 1)).contains("reference of the ledger"));

    let empty = Ledger {
        version: 10,
        integrity: vec!["ok".into()],
        references: Vec::new(),
        tables: BTreeMap::new(),
    };
    assert!(refusal(assert_sound(&empty, 1)).contains("holds no table"));
    let damaged = Ledger {
        integrity: vec!["*** in database main ***".into()],
        tables: BTreeMap::from([("a".to_string(), Vec::new())]),
        ..empty
    };
    assert!(refusal(assert_sound(&damaged, 1)).contains("integrity check"));
}

#[test]
fn has_a_project_for_each_directory_asked_for_and_says_which_has_none() {
    let folder = tempfile::tempdir().unwrap();
    let file = ledger_file(folder.path(), 10, &ROWS);
    let said = refusal(assert_projects(
        &read_ledger(&file).unwrap(),
        &[PathBuf::from("/w/a"), PathBuf::from("/w/missing")],
    ));
    assert!(said.contains("no project in /w/missing"), "{said}");
    // A ledger with no project at all has none for any.
    let none = Ledger {
        version: 1,
        integrity: Vec::new(),
        references: Vec::new(),
        tables: BTreeMap::new(),
    };
    assert!(
        refusal(assert_projects(&none, &[PathBuf::from("/w/a")])).contains("no project in /w/a")
    );
    assert_projects(&none, &[]).unwrap();
}

#[test]
fn reads_the_events_a_daemon_traced_from_its_trace_file_and_only_the_ledgers() {
    let lines = [
        r#"{"at":"t1","project":1,"kind":"project.created","data":{"name":"a"}}"#,
        r#"{"at":"t1","kind":"window.activity","project":1,"participant":"chief","state":"idle"}"#,
        "not json",
        "",
        "null",
        "[1,2]",
        r#"{"at":"t2","project":2,"kind":"project.created","data":{"name":"b"}}"#,
    ]
    .join("\n");
    let events = traced_events(&lines);
    assert_eq!(
        events
            .iter()
            .map(|event| (event["project"].clone(), event["kind"].clone()))
            .collect::<Vec<_>>(),
        [
            (json!(1), json!("project.created")),
            (json!(2), json!("project.created"))
        ]
    );
    let folder = tempfile::tempdir().unwrap();
    let ledger = read_ledger(&ledger_file(folder.path(), 10, &ROWS)).unwrap();
    assert_traced(&events, &ledger).unwrap();
    let lost = [
        json!({"at": "t3", "project": 1, "kind": "project.state", "data": {}}),
        json!({"at": "t1", "project": 1, "kind": "project.created", "data": {"name": "x"}}),
    ];
    for event in lost {
        let said = refusal(assert_traced(&[event], &ledger));
        assert!(said.contains("lost an event the daemon traced"), "{said}");
    }
}

#[test]
fn takes_an_event_by_its_json_as_it_was_written_with_the_keys_in_its_order() {
    let folder = tempfile::tempdir().unwrap();
    let ledger = read_ledger(&ledger_file(
        folder.path(),
        10,
        &[
            ROWS[0],
            "INSERT INTO event VALUES (1, 1, 't1', 'k', '{\"b\":1,\"a\":2}')",
        ],
    ))
    .unwrap();
    let written = traced_events(r#"{"at":"t1","project":1,"kind":"k","data":{"b":1,"a":2}}"#);
    assert_traced(&written, &ledger).unwrap();
    let reordered = traced_events(r#"{"at":"t1","project":1,"kind":"k","data":{"a":2,"b":1}}"#);
    assert!(refusal(assert_traced(&reordered, &ledger)).contains("lost an event"));
    // An event the ledger holds that is not JSON is the ledger's fault, and is said.
    fs::create_dir(folder.path().join("broken")).unwrap();
    let broken = read_ledger(&ledger_file(
        &folder.path().join("broken"),
        10,
        &[
            ROWS[0],
            "INSERT INTO event VALUES (1, 1, 't1', 'k', 'not json')",
        ],
    ))
    .unwrap();
    let said = refusal(assert_traced(&written, &broken));
    assert!(said.contains("holds no JSON"), "{said}");
}
