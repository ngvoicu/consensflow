//! A chief as a bare harness: what its own record says it did, with no board.

#![allow(clippy::unwrap_used)]

use std::collections::HashSet;

use cf_e2e::live::ask::Item;
use cf_evals::bare::{asking_turn_end, bare_metrics, report_value};

fn item(id: &str, role: &str, text: &str, complete: bool) -> Item {
    Item {
        id: id.to_owned(),
        role: role.to_owned(),
        text: text.to_owned(),
        complete,
    }
}

fn record() -> Vec<Item> {
    vec![
        item("u1", "user", "Add the page", true),
        item("a1", "assistant", "Reading?", false),
        item("t1", "tool", "ok", true),
        item(
            "a2",
            "assistant",
            "Keep the old document? Publish today?",
            true,
        ),
        item("u2", "user", "Keep it. Not yet.", true),
        item("a3", "assistant", "Done: the page is in place.", true),
    ]
}

#[test]
fn counts_what_a_bare_chief_asked_at_the_ends_of_its_turns_with_no_board() {
    let metrics = bare_metrics(
        &record(),
        vec!["site/legislatie.html".to_owned()],
        &HashSet::new(),
    );
    assert!(metrics.tasks.is_empty());
    assert_eq!((metrics.questions_on_board, metrics.chief_turns), (0, 3));
    assert_eq!(metrics.chief_last_words, "Done: the page is in place.");
    assert_eq!(
        (
            metrics.owner_questions.questions,
            metrics.owner_questions.turns_asking
        ),
        (2, 1)
    );
    assert_eq!(metrics.owner_messages, [12, 17], "what the owner typed");
    assert_eq!(metrics.files_changed, ["site/legislatie.html"]);
    assert_eq!(metrics.chief_edits, None);
}

#[test]
fn names_the_turn_end_the_owner_should_answer_only_when_it_asks_something() {
    let items = record();
    let nothing = HashSet::new();
    assert_eq!(
        asking_turn_end(&items[..4], &nothing).map(|end| end.id.as_str()),
        Some("a2")
    );
    assert!(
        asking_turn_end(&items, &nothing).is_none(),
        "its last turn asked nothing"
    );
    assert!(
        asking_turn_end(&items[..2], &nothing).is_none(),
        "a question mid-turn is not put to the owner"
    );
}

fn unmarked() -> Vec<Item> {
    vec![
        item("u1", "user", "Add the page", true),
        item(
            "a1",
            "assistant",
            "Keep the old document? Publish today?",
            false,
        ),
        item("u2", "user", "Keep it. Not yet.", true),
        item(
            "a2",
            "assistant",
            "Pages done. Waiting for your feedback.",
            false,
        ),
    ]
}

#[test]
fn takes_the_message_the_window_rested_on_as_the_end_of_that_turn_where_the_record_never_marks_one()
{
    // Devin's and OpenCode's records never say a turn is over.
    let items = unmarked();
    assert!(
        asking_turn_end(&items[..2], &HashSet::new()).is_none(),
        "nothing marked, nothing seen at rest"
    );
    let rested: HashSet<String> = ["a1".to_owned()].into();
    assert_eq!(
        asking_turn_end(&items[..2], &rested).map(|end| end.id.as_str()),
        Some("a1")
    );
    let both: HashSet<String> = ["a1".to_owned(), "a2".to_owned()].into();
    let metrics = bare_metrics(&items, Vec::new(), &both);
    assert_eq!(
        (
            metrics.owner_questions.questions,
            metrics.owner_questions.turns_asking
        ),
        (2, 1)
    );
}

#[test]
fn a_bare_report_keeps_the_numbers_a_bare_run_has_in_the_order_the_other_arms_reports_do() {
    let metrics = bare_metrics(&record(), vec!["site/a.html".to_owned()], &HashSet::new());
    let said = report_value(&metrics);
    let keys: Vec<&str> = said
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "chief",
            "tasks",
            "parallel",
            "advice",
            "reviews",
            "questionsOnBoard",
            "askRefused",
            "notesToHuman",
            "notesText",
            "longestResult",
            "ownerMessages",
            "taskCount",
            "chiefTurns",
            "chiefEdits",
            "chiefLastWords",
            "ownerQuestions",
            "filesChanged",
        ]
    );
    assert_eq!(said["chief"], "chief");
    assert_eq!(said["chiefEdits"], serde_json::Value::Null);
    assert_eq!(said["ownerQuestions"]["questions"], 2);
    assert_eq!(said["filesChanged"], serde_json::json!(["site/a.html"]));
}
