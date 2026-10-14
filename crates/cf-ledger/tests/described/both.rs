//! What holds of the name and of the line alike: a project that is not there is
//! refused in the usual words, and what one project is called and says is never
//! another's.

use super::world::{events_of, home, project, refusal};

#[test]
fn a_project_the_ledger_does_not_have_is_refused_in_the_usual_words_and_nothing_is_logged() {
    let mut home = home();
    let app = project(&mut home.ledger, "app");
    let logged = home.told.borrow().len();
    let unknown = (Some("unknown-project"), "no project 99".to_owned());
    assert_eq!(refusal(home.ledger.rename_project(99, "harbour")), unknown);
    for description in [Some("Moves the cargo"), None, Some("  ")] {
        assert_eq!(
            refusal(home.ledger.describe_project(99, description)),
            unknown,
            "{description:?}"
        );
    }
    assert_eq!(home.ledger.project(99).expect("read"), None);
    assert_eq!(home.ledger.project(app.id).expect("read"), Some(app));
    assert_eq!(home.told.borrow().len(), logged);
}

#[test]
fn what_one_project_is_called_and_says_is_never_another_s() {
    let mut home = home();
    let (first, second) = (
        project(&mut home.ledger, "first"),
        project(&mut home.ledger, "second"),
    );
    home.ledger
        .rename_project(first.id, "one")
        .expect("renamed");
    home.ledger
        .describe_project(second.id, Some("The second"))
        .expect("described");
    let read = |id| home.ledger.project(id).expect("read").expect("there");
    assert_eq!(
        (read(first.id).name, read(first.id).description),
        ("one".to_owned(), None)
    );
    assert_eq!(
        (read(second.id).name, read(second.id).description),
        ("second".to_owned(), Some("The second".to_owned()))
    );
    assert_eq!(
        events_of(&home.ledger, first.id, "project.described").len(),
        0
    );
    assert_eq!(
        events_of(&home.ledger, second.id, "project.renamed").len(),
        0
    );
}
