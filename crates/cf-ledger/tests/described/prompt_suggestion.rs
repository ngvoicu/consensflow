//! Whether the human leaves Claude Code's prompt suggestion on in the project's windows
//! (Project settings, "Prompt suggestion"): off for every project until it is ticked, switched
//! on and off, what each change logs and tells the trace, that the view has it only while it is
//! on, that a project that is not there is refused and changes nothing, that a project opened
//! with it has it as if said right after, and that a ledger at the version before gains it off
//! in every project with every row kept (T-1444).

use cf_ledger::{migrate, NewProject, ProjectView, MIGRATIONS};
use rusqlite::Connection;
use serde_json::json;

use super::migration::{populate, rows};
use super::world::{events_of, home, project, refusal, request, told_of, MINUTE, START};

fn key(view: &ProjectView) -> Option<serde_json::Value> {
    serde_json::to_value(view)
        .expect("the view as the page reads it")
        .get("promptSuggestion")
        .cloned()
}

#[test]
fn a_project_has_the_suggestion_off_and_the_view_says_so_only_while_it_is_on() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    assert!(!app.prompt_suggestion, "every project starts with it off");
    assert_eq!(
        key(&app),
        None,
        "a project with it off reads as it always did"
    );

    home.now.set(START + MINUTE);
    let on = home.ledger.set_prompt_suggestion(app.id, true).expect("on");
    assert!(on.prompt_suggestion);
    assert_eq!(key(&on), Some(json!(true)));
    assert_eq!(on.updated_at, "2026-10-13T12:01:00.000Z");
    assert_eq!(home.ledger.project(app.id).expect("read"), Some(on.clone()));
    let mut expected = app.clone();
    expected.prompt_suggestion = true;
    expected.updated_at = on.updated_at.clone();
    assert_eq!(on, expected, "the rest of the project stays");

    home.now.set(START + 2 * MINUTE);
    let off = home
        .ledger
        .set_prompt_suggestion(app.id, false)
        .expect("off");
    assert!(!off.prompt_suggestion);
    assert_eq!(key(&off), None, "the key is gone with it");

    let said = events_of(&home.ledger, app.id, "project.prompt_suggestion");
    assert_eq!(
        said,
        [
            json!({ "from": false, "to": true }),
            json!({ "from": true, "to": false }),
        ]
    );
    assert_eq!(
        told_of(&home, "project.prompt_suggestion"),
        [(app.id, said[0].clone()), (app.id, said[1].clone())],
        "the trace is told each one"
    );
}

#[test]
fn the_same_suggestion_word_again_logs_nothing_and_touches_nothing() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    home.now.set(START + MINUTE);
    let kept = home
        .ledger
        .set_prompt_suggestion(app.id, false)
        .expect("off");
    assert_eq!(kept, app, "it was off");
    home.ledger.set_prompt_suggestion(app.id, true).expect("on");
    home.now.set(START + 2 * MINUTE);
    let again = home
        .ledger
        .set_prompt_suggestion(app.id, true)
        .expect("on again");
    assert_eq!(again.updated_at, "2026-10-13T12:01:00.000Z");
    assert_eq!(told_of(&home, "project.prompt_suggestion").len(), 1);
}

#[test]
fn a_project_the_ledger_does_not_have_is_refused_and_one_projects_suggestion_is_never_another_s() {
    let mut home = home();
    let (first, second) = (
        project(&mut home.ledger, "first"),
        project(&mut home.ledger, "second"),
    );
    let logged = home.told.borrow().len();
    for on in [true, false] {
        assert_eq!(
            refusal(home.ledger.set_prompt_suggestion(99, on)),
            (Some("unknown-project"), "no project 99".to_owned())
        );
    }
    assert_eq!(home.told.borrow().len(), logged);
    home.ledger
        .set_prompt_suggestion(second.id, true)
        .expect("on");
    let read = |id| home.ledger.project(id).expect("read").expect("there");
    assert!(!read(first.id).prompt_suggestion);
    assert!(read(second.id).prompt_suggestion);
    assert!(events_of(&home.ledger, first.id, "project.prompt_suggestion").is_empty());
}

#[test]
fn a_request_in_json_says_the_suggestion_with_a_flag_and_refuses_anything_else_in_words() {
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
    assert!(!asked(json!({})).expect("none said").prompt_suggestion);
    assert!(
        asked(json!({ "promptSuggestion": true }))
            .expect("on")
            .prompt_suggestion
    );
    assert!(
        !asked(json!({ "promptSuggestion": false }))
            .expect("off")
            .prompt_suggestion
    );
    for value in [json!("true"), json!(1), json!(0), json!([]), json!(null)] {
        let refused = asked(json!({ "promptSuggestion": value })).expect_err("a refusal");
        assert_eq!(
            (refused.code(), refused.to_string()),
            (
                Some("invalid-prompt-suggestion"),
                "the prompt suggestion is on (true) or off (false)".to_owned()
            ),
            "{value}"
        );
    }
}

#[test]
fn a_project_opened_with_the_suggestion_ticked_has_it_when_first_seen_and_logs_it_as_if_said_right_after(
) {
    let mut home = home();
    let opened = home
        .ledger
        .create_project(&NewProject {
            prompt_suggestion: true,
            ..request("opened", "/work/opened")
        })
        .expect("a project");
    assert!(
        opened.prompt_suggestion,
        "the view the chief's launch is given"
    );
    let later = project(&mut home.ledger, "later");
    home.ledger
        .set_prompt_suggestion(later.id, true)
        .expect("on");
    let own = events_of(&home.ledger, opened.id, "project.prompt_suggestion");
    assert_eq!(own, [json!({ "from": false, "to": true })]);
    assert_eq!(
        own,
        events_of(&home.ledger, later.id, "project.prompt_suggestion")
    );
    let plain = project(&mut home.ledger, "plain");
    assert!(events_of(&home.ledger, plain.id, "project.prompt_suggestion").is_empty());
}

/// Migration 0036: every project that exists has the suggestion off (the windows of the
/// projects Gabriel has now are launched without it from their next start), and every other
/// value of every row is kept.
#[test]
fn a_ledger_at_the_version_before_gains_the_suggestion_off_in_every_project_and_keeps_every_row() {
    let at = MIGRATIONS
        .iter()
        .position(|sql| sql.contains("ADD COLUMN prompt_suggestion"))
        .expect("the migration that added the prompt suggestion (0036)");
    assert_eq!(at, 35, "migration 0036");
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
        "each project's row, and the suggestion off at its end"
    );
}
