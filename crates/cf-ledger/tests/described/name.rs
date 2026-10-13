//! A project's name: a rename, which has the rules a name has when the project
//! is opened, what it logs and tells the trace, and what a refused one leaves.

use serde_json::json;

use super::world::{
    events_of, home, project, refusal, request, told_of, MINUTE, NAME_REFUSED, START,
};

#[test]
fn a_project_is_renamed_and_nothing_but_its_name_and_its_time_changes() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    let other = project(&mut home.ledger, "docs");
    home.now.set(START + 5 * MINUTE);

    let renamed = home
        .ledger
        .rename_project(app.id, "harbour")
        .expect("renamed");

    let mut expected = app.clone();
    expected.name = "harbour".to_owned();
    expected.updated_at = "2026-10-13T12:05:00.000Z".to_owned();
    assert_eq!(
        renamed, expected,
        "its folder, state, staff and the rest stay"
    );
    assert_eq!(renamed.directory, "/work/app", "the folder never changes");
    assert_eq!(
        home.ledger.project(app.id).expect("read"),
        Some(renamed.clone())
    );
    let listed: Vec<_> = home.ledger.projects().expect("listed");
    assert_eq!(
        listed
            .iter()
            .map(|project| project.name.as_str())
            .collect::<Vec<_>>(),
        ["harbour", "docs"]
    );
    assert_eq!(listed[1], other, "another project is not touched");
}

#[test]
fn a_rename_logs_what_the_name_was_and_became_tells_the_trace_and_a_name_kept_logs_nothing() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    home.now.set(START + MINUTE);
    home.ledger
        .rename_project(app.id, "harbour")
        .expect("renamed");
    let said = json!({ "from": "app", "to": "harbour" });
    assert_eq!(
        events_of(&home.ledger, app.id, "project.renamed"),
        std::slice::from_ref(&said)
    );
    assert_eq!(
        told_of(&home, "project.renamed"),
        [(app.id, said)],
        "the trace (the daemon's events.jsonl) is told as it is of every change"
    );

    // The name it already has: nothing is renamed, so nothing is logged or touched.
    home.now.set(START + 2 * MINUTE);
    let kept = home.ledger.rename_project(app.id, "harbour").expect("kept");
    assert_eq!(kept.updated_at, "2026-10-13T12:01:00.000Z");
    assert_eq!(events_of(&home.ledger, app.id, "project.renamed").len(), 1);
    assert_eq!(told_of(&home, "project.renamed").len(), 1);

    home.ledger.rename_project(app.id, "app").expect("back");
    assert_eq!(
        events_of(&home.ledger, app.id, "project.renamed")[1],
        json!({ "from": "harbour", "to": "app" })
    );
}

#[test]
fn a_name_is_checked_as_a_new_project_s_is_and_kept_as_given() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    let hundred = "x".repeat(100);
    for (at, (name, fits)) in [
        ("a".to_owned(), true),
        (hundred.clone(), true),
        // Two units each in JavaScript: fifty of them are a hundred.
        ("😀".repeat(50), true),
        ("é".repeat(100), true),
        ("  padded  ".to_owned(), true),
        (String::new(), false),
        (" ".to_owned(), false),
        ("\t\r\n ".to_owned(), false),
        (format!("{hundred}x"), false),
        ("😀".repeat(51), false),
    ]
    .into_iter()
    .enumerate()
    {
        let created = home
            .ledger
            .create_project(&request(&name, &format!("/work/{at}")));
        let renamed = home.ledger.rename_project(app.id, &name);
        match (created, renamed, fits) {
            (Ok(_), Ok(view), true) => assert_eq!(view.name, name, "kept as given"),
            (created @ Err(_), renamed @ Err(_), false) => {
                assert_eq!(refusal(renamed), refusal(created), "{name:?}");
            }
            (created, renamed, _) => panic!(
                "{name:?}: opened {:?}, renamed {:?}",
                created.map(|view| view.name),
                renamed.map(|view| view.name)
            ),
        }
    }
    assert_eq!(
        refusal(home.ledger.rename_project(app.id, "")),
        (Some("invalid-text"), NAME_REFUSED.to_owned())
    );
}

#[test]
fn a_name_that_is_refused_is_refused_before_the_project_is_looked_for_and_writes_nothing() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    let logged = home.told.borrow().len();
    for name in ["", "   ", &"x".repeat(101)] {
        for id in [app.id, 99] {
            assert_eq!(
                refusal(home.ledger.rename_project(id, name)),
                (Some("invalid-text"), NAME_REFUSED.to_owned()),
                "project {id}, {name:.5?}"
            );
        }
    }
    assert_eq!(home.ledger.project(app.id).expect("read"), Some(app));
    assert_eq!(home.told.borrow().len(), logged, "nothing was logged");
}
