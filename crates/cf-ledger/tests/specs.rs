//! What a project says of specs in the ledger: migration 0015 gives the project
//! three columns that no row needs a value in, the human's word is kept as given
//! (checked, and logged where it changes), and read together with what the human
//! set for every project it says whether the chief plans by specs, where, and by
//! which variant of Specmint. The folder and the variant are refused before
//! anything is written, and what one project says is never another's.

#![allow(clippy::expect_used)]

use cf_ledger::{
    migrate, open_ledger, Ledger, NewChief, NewProject, Options, ProjectView, SpecsChange,
    MIGRATIONS,
};
use cf_proto::specs::{SpecsSetting, Variant};
use rusqlite::Connection;
use serde_json::{json, Value};

const AT: &str = "2026-10-13T10:00:00.000Z";

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger")
}

fn project(ledger: &mut Ledger, name: &str) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: format!("/work/{name}"),
            name: name.into(),
            chief: NewChief {
                harness: "claude-code".into(),
                agent: Some("apollo".into()),
            },
            staff: Vec::new(),
            gate: false,
        })
        .expect("a project")
}

fn said(request: Value) -> SpecsChange {
    SpecsChange::from_json(&request).expect("a request the ledger takes")
}

fn events_of(ledger: &Ledger, project: i64, kind: &str) -> Vec<Value> {
    ledger
        .events(project, 0, 500)
        .expect("the events")
        .into_iter()
        .filter(|event| event.kind == kind)
        .map(|event| event.data)
        .collect()
}

#[test]
fn a_project_that_never_said_anything_of_specs_reads_as_it_always_did() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let app = project(&mut ledger, "app");
    assert_eq!(app.specs, None);
    let written = serde_json::to_value(&app).expect("the view as the page reads it");
    assert!(
        written.get("specs").is_none(),
        "a project that never said it has no `specs` key: {written}"
    );
    assert_eq!(app.specs_setting(false), None);
    assert_eq!(
        app.specs_setting(true),
        Some(SpecsSetting {
            folder: ".specs".into(),
            variant: Variant::Core
        }),
        "following the app's default, with the folder and the variant that are none"
    );
}

#[test]
fn a_project_s_word_is_kept_viewed_and_logged_as_it_changes_and_never_when_it_does_not() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let app = project(&mut ledger, "app");

    let set = ledger
        .set_specs(
            app.id,
            &said(json!({ "on": true, "folder": "./docs//specs/", "variant": "tdd-html" })),
        )
        .expect("set");
    assert_eq!(
        serde_json::to_value(&set).expect("the view")["specs"],
        json!({ "on": true, "folder": "docs/specs", "variant": "tdd-html" }),
        "the folder is kept as the project's own, in slashes"
    );
    let read_back = ledger.project(app.id).expect("read").expect("the project");
    assert_eq!(read_back.specs, set.specs);
    assert_eq!(
        events_of(&ledger, app.id, "project.specs"),
        [json!({
            "from": { "on": null, "folder": null, "variant": null },
            "to": { "on": true, "folder": "docs/specs", "variant": "tdd-html" },
        })]
    );

    // The same word again changes nothing, and logs nothing.
    ledger
        .set_specs(
            app.id,
            &said(json!({ "on": true, "folder": "docs/specs", "variant": "tdd-html" })),
        )
        .expect("set again");
    assert_eq!(events_of(&ledger, app.id, "project.specs").len(), 1);

    // A part left out is not said: the project goes back to its defaults.
    let cleared = ledger
        .set_specs(app.id, &SpecsChange::default())
        .expect("cleared");
    assert_eq!(cleared.specs, None, "it says nothing again");
    assert_eq!(
        events_of(&ledger, app.id, "project.specs")[1],
        json!({
            "from": { "on": true, "folder": "docs/specs", "variant": "tdd-html" },
            "to": { "on": null, "folder": null, "variant": null },
        })
    );

    // Off is a word of its own, not the lack of one.
    let off = ledger
        .set_specs(app.id, &said(json!({ "on": false })))
        .expect("off");
    assert_eq!(off.specs.as_ref().and_then(|specs| specs.on), Some(false));
}

#[test]
fn the_project_s_word_and_the_app_s_default_resolve_in_every_combination() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let app = project(&mut ledger, "app");
    // (the project's own word, the app's default, whether the chief is given specs)
    let table = [
        (None, false, false),
        (None, true, true),
        (Some(true), false, true),
        (Some(true), true, true),
        (Some(false), false, false),
        (Some(false), true, false),
    ];
    for (own, default, given) in table {
        let change = SpecsChange {
            on: own,
            folder: Some("plans".into()),
            variant: Some(Variant::Tdd),
        };
        let view = ledger.set_specs(app.id, &change).expect("set");
        assert_eq!(
            view.specs_setting(default),
            given.then(|| SpecsSetting {
                folder: "plans".into(),
                variant: Variant::Tdd
            }),
            "the project says {own:?}, the app's default is {default}"
        );
    }
}

#[test]
fn what_one_project_says_is_never_another_s() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let (first, second) = (
        project(&mut ledger, "first"),
        project(&mut ledger, "second"),
    );
    ledger
        .set_specs(
            first.id,
            &said(json!({ "on": true, "variant": "core-html" })),
        )
        .expect("set the first");
    ledger
        .set_specs(second.id, &said(json!({ "on": false, "folder": "notes" })))
        .expect("set the second");
    let read = |id| ledger.project(id).expect("read").expect("the project");
    assert_eq!(
        read(first.id).specs_setting(false),
        Some(SpecsSetting {
            folder: ".specs".into(),
            variant: Variant::CoreHtml
        })
    );
    assert_eq!(read(second.id).specs_setting(true), None);
    assert_eq!(events_of(&ledger, first.id, "project.specs").len(), 1);
    assert_eq!(events_of(&ledger, second.id, "project.specs").len(), 1);
}

#[test]
fn a_project_the_ledger_does_not_have_is_refused_and_nothing_is_logged() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let refused = ledger
        .set_specs(77, &said(json!({ "on": true })))
        .expect_err("no such project");
    assert_eq!(
        (refused.code(), refused.to_string()),
        (Some("unknown-project"), "no project 77".to_owned())
    );
}

#[test]
fn a_folder_that_leaves_the_project_and_a_variant_that_is_none_of_the_four_are_refused_before_a_write(
) {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let app = project(&mut ledger, "app");
    ledger
        .set_specs(app.id, &said(json!({ "on": true, "folder": "plans" })))
        .expect("set");
    for (request, code) in [
        (json!({ "folder": "../elsewhere" }), "invalid-specs-folder"),
        (json!({ "folder": "/etc/specs" }), "invalid-specs-folder"),
        (json!({ "folder": "C:\\specs" }), "invalid-specs-folder"),
        (json!({ "folder": "a/../../b" }), "invalid-specs-folder"),
        (
            json!({ "variant": "specmint-core" }),
            "invalid-specs-variant",
        ),
        (json!({ "variant": "all" }), "invalid-specs-variant"),
        (json!({ "on": "yes" }), "invalid-specs"),
    ] {
        let refused = SpecsChange::from_json(&request).expect_err("a request it refuses");
        assert_eq!(refused.code(), Some(code), "{request}");
    }
    let still = ledger.project(app.id).expect("read").expect("the project");
    assert_eq!(
        still.specs_setting(false).map(|setting| setting.folder),
        Some("plans".to_owned()),
        "what was said before stays"
    );
    assert_eq!(events_of(&ledger, app.id, "project.specs").len(), 1);
}

/// The rows of `sql`, each as the text its columns quote to.
fn rows(db: &Connection, sql: &str) -> Vec<String> {
    let mut statement = db.prepare(sql).expect("a query");
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            (0..columns)
                .map(|at| {
                    row.get::<_, rusqlite::types::Value>(at)
                        .map(|value| format!("{value:?}"))
                })
                .collect::<rusqlite::Result<Vec<_>>>()
                .map(|cells| cells.join("|"))
        })
        .expect("the rows")
        .map(|row| row.expect("a row"))
        .collect()
}

#[test]
fn a_ledger_of_the_version_before_gains_the_three_columns_empty_and_loses_nothing() {
    let db = Connection::open_in_memory().expect("a database");
    db.execute_batch("PRAGMA foreign_keys = ON").expect("keys");
    migrate(&db, &MIGRATIONS[..13]).expect("the version before");
    db.execute_batch(&format!(
        "INSERT INTO project (id, directory, name, state, gate, created_at, updated_at) VALUES
           (1, '/work/app', 'app', 'suspended', 0, '{AT}', '{AT}'),
           (2, '/work/docs', 'docs', 'open', 1, '{AT}', '{AT}');
         INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, created_at, designer) VALUES
           (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, '{AT}', 0),
           (2, 1, 'chief', 'chief', '[]', 'apollo', 'claude-code', NULL, NULL, '{AT}', 0);
         INSERT INTO event (id, project_id, at, kind, data) VALUES
           (1, 1, '{AT}', 'project.created', '{{}}');"
    ))
    .expect("a populated ledger");
    let others = |db: &Connection| {
        ["participant", "event", "task", "message"]
            .map(|table| rows(db, &format!("SELECT * FROM {table} ORDER BY rowid")))
    };
    let before = (
        rows(
            &db,
            "SELECT id, directory, name, state, gate, created_at, updated_at FROM project ORDER BY rowid",
        ),
        others(&db),
    );

    migrate(&db, &MIGRATIONS).expect("migrated");
    migrate(&db, &MIGRATIONS).expect("and again");

    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the version");
    assert_eq!(version, i64::try_from(MIGRATIONS.len()).expect("a count"));
    let columns: Vec<String> = db
        .prepare("SELECT name FROM pragma_table_info('project') ORDER BY cid")
        .expect("the columns")
        .query_map([], |row| row.get(0))
        .expect("the columns")
        .map(|name| name.expect("a name"))
        .collect();
    assert_eq!(
        columns,
        [
            "id",
            "directory",
            "name",
            "state",
            "gate",
            "created_at",
            "updated_at",
            "specs",
            "specs_folder",
            "specs_variant"
        ],
        "the three come last, and every column is where it was"
    );
    assert_eq!(
        rows(
            &db,
            "SELECT id, directory, name, state, gate, created_at, updated_at FROM project ORDER BY rowid"
        ),
        before.0
    );
    assert_eq!(others(&db), before.1, "no other table moved");
    assert_eq!(
        rows(
            &db,
            "SELECT specs, specs_folder, specs_variant FROM project ORDER BY rowid"
        ),
        ["Null|Null|Null", "Null|Null|Null"],
        "nobody said anything of specs"
    );
    let refuses = |set: &str| {
        db.execute_batch(&format!("UPDATE project SET {set} WHERE id = 1"))
            .expect_err("a word the table refuses")
            .to_string()
    };
    assert!(refuses("specs = 2").contains("project_specs_check"));
    assert!(refuses("specs_variant = 'full'").contains("project_specs_variant_check"));
    db.execute_batch(
        "UPDATE project SET specs = 0, specs_folder = 'plans', specs_variant = 'tdd-html' WHERE id = 1",
    )
    .expect("a word the table takes");
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("the check");
    assert_eq!(integrity, "ok");
    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .expect("the check")
            .exists([])
            .expect("the check"),
        "every reference holds"
    );
}
