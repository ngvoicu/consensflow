//! Whether the human has the project's members run with Claude Code's own advisor tool
//! (Project settings, "Advisor tool"): off for every project until it is ticked, switched on
//! and off, what each change logs and tells the trace, that the view has it only while it is
//! on, that a project that is not there is refused and changes nothing, that a project opened
//! with it has it as if said right after, and that a ledger at the version before gains it off
//! in every project with every row kept (alpha-5 A5-20).

use cf_ledger::{migrate, NewProject, ProjectView, MIGRATIONS};
use rusqlite::Connection;
use serde_json::json;

use super::migration::{populate, rows};
use super::world::{events_of, home, project, refusal, request, told_of, MINUTE, START};

fn key(view: &ProjectView) -> Option<serde_json::Value> {
    serde_json::to_value(view)
        .expect("the view as the page reads it")
        .get("advisorTool")
        .cloned()
}

#[test]
fn a_project_has_the_tool_off_and_the_view_says_so_only_while_it_is_on() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    assert!(!app.advisor_tool, "every project starts with it off");
    assert_eq!(
        key(&app),
        None,
        "a project with it off reads as it always did"
    );

    home.now.set(START + MINUTE);
    let on = home.ledger.set_advisor_tool(app.id, true).expect("on");
    assert!(on.advisor_tool);
    assert_eq!(key(&on), Some(json!(true)));
    assert_eq!(on.updated_at, "2026-10-13T12:01:00.000Z");
    assert_eq!(home.ledger.project(app.id).expect("read"), Some(on.clone()));
    let mut expected = app.clone();
    expected.advisor_tool = true;
    expected.updated_at = on.updated_at.clone();
    assert_eq!(on, expected, "the rest of the project stays");

    home.now.set(START + 2 * MINUTE);
    let off = home.ledger.set_advisor_tool(app.id, false).expect("off");
    assert!(!off.advisor_tool);
    assert_eq!(key(&off), None, "the key is gone with it");

    let said = events_of(&home.ledger, app.id, "project.advisor_tool");
    assert_eq!(
        said,
        [
            json!({ "from": false, "to": true }),
            json!({ "from": true, "to": false }),
        ]
    );
    assert_eq!(
        told_of(&home, "project.advisor_tool"),
        [(app.id, said[0].clone()), (app.id, said[1].clone())],
        "the trace is told each one"
    );
}

#[test]
fn the_same_word_again_logs_nothing_and_touches_nothing() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    home.now.set(START + MINUTE);
    let kept = home.ledger.set_advisor_tool(app.id, false).expect("off");
    assert_eq!(kept, app, "it was off");
    home.ledger.set_advisor_tool(app.id, true).expect("on");
    home.now.set(START + 2 * MINUTE);
    let again = home
        .ledger
        .set_advisor_tool(app.id, true)
        .expect("on again");
    assert_eq!(again.updated_at, "2026-10-13T12:01:00.000Z");
    assert_eq!(told_of(&home, "project.advisor_tool").len(), 1);
}

#[test]
fn a_project_the_ledger_does_not_have_is_refused_and_one_projects_word_is_never_another_s() {
    let mut home = home();
    let (first, second) = (
        project(&mut home.ledger, "first"),
        project(&mut home.ledger, "second"),
    );
    let logged = home.told.borrow().len();
    for on in [true, false] {
        assert_eq!(
            refusal(home.ledger.set_advisor_tool(99, on)),
            (Some("unknown-project"), "no project 99".to_owned())
        );
    }
    assert_eq!(home.told.borrow().len(), logged);
    home.ledger.set_advisor_tool(second.id, true).expect("on");
    let read = |id| home.ledger.project(id).expect("read").expect("there");
    assert!(!read(first.id).advisor_tool);
    assert!(read(second.id).advisor_tool);
    assert!(events_of(&home.ledger, first.id, "project.advisor_tool").is_empty());
}

#[test]
fn a_request_in_json_says_it_with_a_flag_and_refuses_anything_else_in_words() {
    let asked = |said: serde_json::Value| {
        let mut asked = json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "model": "claude-opus-5" },
            "staff": [],
        });
        asked
            .as_object_mut()
            .expect("an object")
            .extend(said.as_object().cloned().expect("an object"));
        NewProject::from_json(&asked)
    };
    assert!(!asked(json!({})).expect("none said").advisor_tool);
    assert!(
        asked(json!({ "advisorTool": true }))
            .expect("on")
            .advisor_tool
    );
    assert!(
        !asked(json!({ "advisorTool": false }))
            .expect("off")
            .advisor_tool
    );
    for value in [json!("true"), json!(1), json!(0), json!([]), json!(null)] {
        let refused = asked(json!({ "advisorTool": value })).expect_err("a refusal");
        assert_eq!(
            (refused.code(), refused.to_string()),
            (
                Some("invalid-advisor-tool"),
                "the advisor tool is on (true) or off (false)".to_owned()
            ),
            "{value}"
        );
    }
}

#[test]
fn a_project_opened_with_it_has_it_when_first_seen_and_logs_it_as_if_said_right_after() {
    let mut home = home();
    let opened = home
        .ledger
        .create_project(&NewProject {
            advisor_tool: true,
            ..request("opened", "/work/opened")
        })
        .expect("a project");
    assert!(opened.advisor_tool, "the view the chief's launch is given");
    let later = project(&mut home.ledger, "later");
    home.ledger.set_advisor_tool(later.id, true).expect("on");
    let own = events_of(&home.ledger, opened.id, "project.advisor_tool");
    assert_eq!(own, [json!({ "from": false, "to": true })]);
    assert_eq!(
        own,
        events_of(&home.ledger, later.id, "project.advisor_tool")
    );
    let plain = project(&mut home.ledger, "plain");
    assert!(events_of(&home.ledger, plain.id, "project.advisor_tool").is_empty());
}

/// Migration 0035: every project that exists has the tool off (the members of the projects
/// Gabriel has now run without it from their next start), and every other value of every row
/// is kept.
#[test]
fn a_ledger_at_the_version_before_gains_the_tool_off_in_every_project_and_keeps_every_row() {
    let at = MIGRATIONS
        .iter()
        .position(|sql| sql.contains("ADD COLUMN advisor_tool"))
        .expect("the migration that added the advisor tool (0035)");
    assert_eq!(at, 34, "migration 0035");
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
            .map(|row| format!("{row}|Integer(0)"))
            .collect::<Vec<_>>(),
        "each project's row, and the tool off at its end"
    );
}
