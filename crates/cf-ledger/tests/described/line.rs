//! The line under a project's name: set, changed and taken away, one line of at
//! most 200 characters, what it logs and tells the trace, and that the view has
//! it only while there is one.

use cf_ledger::ProjectView;
use serde_json::json;

use super::world::{events_of, home, project, refusal, told_of, MINUTE, START};

#[test]
fn a_description_is_set_changed_and_taken_away_and_the_view_has_it_only_while_there_is_one() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    let key = |view: &ProjectView| {
        serde_json::to_value(view)
            .expect("the view as the page reads it")
            .get("description")
            .cloned()
    };
    assert_eq!(app.description, None);
    assert_eq!(
        key(&app),
        None,
        "a project with none reads as it always did"
    );

    home.now.set(START + MINUTE);
    let set = home
        .ledger
        .describe_project(app.id, Some("Moves the cargo"))
        .expect("set");
    assert_eq!(set.description.as_deref(), Some("Moves the cargo"));
    assert_eq!(key(&set), Some(json!("Moves the cargo")));
    assert_eq!(set.updated_at, "2026-10-13T12:01:00.000Z");
    assert_eq!(
        home.ledger.project(app.id).expect("read"),
        Some(set.clone())
    );
    let mut expected = app.clone();
    expected.description = set.description.clone();
    expected.updated_at = set.updated_at.clone();
    assert_eq!(set, expected, "the name and the rest stay");

    let changed = home
        .ledger
        .describe_project(app.id, Some("  Moves the other cargo  "))
        .expect("changed");
    assert_eq!(
        changed.description.as_deref(),
        Some("Moves the other cargo"),
        "kept trimmed"
    );

    let said = events_of(&home.ledger, app.id, "project.described");
    assert_eq!(
        said,
        [
            json!({ "from": null, "to": "Moves the cargo" }),
            json!({ "from": "Moves the cargo", "to": "Moves the other cargo" }),
        ]
    );
    assert_eq!(
        told_of(&home, "project.described"),
        [(app.id, said[0].clone()), (app.id, said[1].clone())],
        "the trace is told each one"
    );

    // Taken away by none, by no text and by blank text, each from one there is.
    for away in [None, Some(""), Some("  \t ")] {
        home.ledger
            .describe_project(app.id, Some("Moves the cargo"))
            .expect("set again");
        let gone = home.ledger.describe_project(app.id, away).expect("away");
        assert_eq!(gone.description, None, "{away:?}");
        assert_eq!(key(&gone), None, "{away:?}: the key is gone with it");
        assert_eq!(
            events_of(&home.ledger, app.id, "project.described").last(),
            Some(&json!({ "from": "Moves the cargo", "to": null })),
            "{away:?}"
        );
    }
    assert_eq!(
        told_of(&home, "project.described").len(),
        events_of(&home.ledger, app.id, "project.described").len()
    );
}

#[test]
fn a_description_kept_or_taken_away_when_there_is_none_logs_nothing_and_touches_nothing() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    let before = told_of(&home, "project.described");
    home.now.set(START + MINUTE);
    for none in [None, Some(""), Some("   ")] {
        let view = home.ledger.describe_project(app.id, none).expect("none");
        assert_eq!(view, app, "{none:?}: it had none");
    }
    home.ledger
        .describe_project(app.id, Some("Moves the cargo"))
        .expect("set");
    home.now.set(START + 2 * MINUTE);
    // The same line again, with other spaces round it: the same line.
    let kept = home
        .ledger
        .describe_project(app.id, Some(" Moves the cargo "))
        .expect("kept");
    assert_eq!(kept.updated_at, "2026-10-13T12:01:00.000Z");
    assert_eq!(told_of(&home, "project.described").len(), before.len() + 1);
}

#[test]
fn a_description_is_one_line_of_at_most_two_hundred_characters_and_a_refused_one_changes_nothing() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    home.ledger
        .describe_project(app.id, Some("Moves the cargo"))
        .expect("set");
    let long = "a".repeat(200);
    for fits in [long.clone(), "😀".repeat(100), "é".repeat(200)] {
        let view = home
            .ledger
            .describe_project(app.id, Some(&fits))
            .expect("fits");
        assert_eq!(view.description.as_deref(), Some(fits.as_str()));
    }
    home.ledger
        .describe_project(app.id, Some("Moves the cargo"))
        .expect("back");
    let logged = home.told.borrow().len();
    let too_long =
        "a project's description is one line of plain text: it is at most 200 characters";
    let one_line = "a project's description is one line of plain text: it has no line breaks or other control characters";
    for (text, words) in [
        (format!("{long}a"), too_long),
        ("😀".repeat(101), too_long),
        ("two\nlines".to_owned(), one_line),
        ("two\r\nlines".to_owned(), one_line),
        ("two\u{2028}lines".to_owned(), one_line),
        ("tab\there".to_owned(), one_line),
    ] {
        assert_eq!(
            refusal(home.ledger.describe_project(app.id, Some(&text))),
            (Some("invalid-description"), words.to_owned()),
            "{text:?}"
        );
    }
    let still = home.ledger.project(app.id).expect("read").expect("there");
    assert_eq!(still.description.as_deref(), Some("Moves the cargo"));
    assert_eq!(home.told.borrow().len(), logged, "a refusal logs nothing");
}
