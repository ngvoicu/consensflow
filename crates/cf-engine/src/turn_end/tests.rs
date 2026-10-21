use std::sync::Arc;

use super::*;

fn item(text: &str, commentary: bool) -> Item {
    Item {
        id: Arc::from("i"),
        role: Role::Assistant,
        text: Arc::from(text),
        complete: true,
        at: None,
        commentary,
        compaction: false,
    }
}

fn pending(jobs: usize, agents: usize, retired: usize) -> Pending {
    Pending {
        agents,
        jobs,
        retired,
    }
}

#[test]
fn the_words_of_an_answer_are_its_items_trimmed_and_none_says_so() {
    let (a, b, progress) = (
        item(" First ", false),
        item("Second", false),
        item("x", true),
    );
    assert_eq!(words_of(&[&a, &progress, &b]), "First\n\nSecond");
    let empty = item("  ", false);
    assert_eq!(words_of(&[&empty]), WITHOUT_ANSWER);
    assert_eq!(words_of(&[]), WITHOUT_ANSWER);
}

#[test]
fn what_a_turn_left_running_is_said_in_a_few_words() {
    assert_eq!(running(pending(1, 0, 0)), "1 background job");
    assert_eq!(running(pending(2, 0, 0)), "2 background jobs");
    assert_eq!(running(pending(0, 1, 0)), "1 background agent or workflow");
    assert_eq!(
        running(pending(2, 3, 0)),
        "2 background jobs and 3 background agents or workflows"
    );
}

#[test]
fn a_result_says_what_was_left_only_where_there_is_something_to_say() {
    // An answer with nothing pending and nothing retired is its words alone.
    assert_eq!(leftovers("zeus", pending(0, 0, 0), None), "");
    assert_eq!(with_leftovers("Done".to_owned(), ""), "Done");
    // Finished at someone's word while jobs ran: none of them is ended.
    let said = leftovers("zeus", pending(1, 0, 0), Some("chief"));
    assert_eq!(
        said,
        "Finished at @chief's word while @zeus's harness still counted 1 background job running, which finishing ends none of: it may run on until its window closes, or past it where the host cannot tell what is the window's."
    );
    assert_eq!(
        leftovers("zeus", pending(0, 0, 0), Some("human")),
        "Finished at the human's word: what @zeus said last is the result."
    );
    // What an earlier window left that no word says ended is never said to be gone.
    let said = leftovers("zeus", pending(0, 0, 2), None);
    assert!(
        said.starts_with("no word came that the 2 background jobs an earlier window of @zeus's conversation started had ended"),
        "{said}"
    );
    assert!(!said.contains("nothing remains"));
    assert_eq!(
        with_leftovers("Done".to_owned(), "x"),
        "Done\n\n(ConsensFlow: x)"
    );
}
