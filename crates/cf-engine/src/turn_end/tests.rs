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
        synthetic: false,
        compaction: false,
        detail: None,
    }
}

fn pending(jobs: usize, agents: usize, retired: usize) -> Pending {
    Pending {
        agents,
        jobs,
        retired,
        noticed: 0,
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
    assert_eq!(leftovers("zeus", pending(0, 0, 0), None, true), "");
    assert_eq!(with_leftovers("Done".to_owned(), ""), "Done");
    // Finished at someone's word while jobs ran: none of them is ended.
    let said = leftovers("zeus", pending(1, 0, 0), Some("chief"), true);
    assert_eq!(
        said,
        "Finished at @chief's word while @zeus's harness still counted 1 background job running, which finishing ends none of: it may run on until its window closes, or past it where the host cannot tell what is the window's."
    );
    assert_eq!(
        leftovers("zeus", pending(0, 0, 0), Some("human"), true),
        "Finished at the human's word: what @zeus said last is the result."
    );
    // What an earlier window left that no word says ended is never said to be gone.
    let said = leftovers("zeus", pending(0, 0, 2), None, true);
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

#[test]
fn the_last_words_a_result_takes_say_so_above_them_and_no_other_words_do() {
    // T-1311: a run the never-empty rule took (T-262) says it is the turn's last words.
    let (said, empty) = (item("Let me check the tests", false), item("", false));
    let answers = [
        Item {
            role: Role::User,
            ..item("Go", false)
        },
        said.clone(),
        Item {
            role: Role::Tool,
            ..item("ok", false)
        },
        empty,
    ];
    let answered = crate::role_change::answers_to_the_task(&answers, &[]).expect("an answer");
    assert_eq!(
        words_of(&answered.final_run),
        format!("{LAST_WORDS}\n\nLet me check the tests")
    );
    // The same items, as words after the last tool call: no line above them.
    assert_eq!(words_of(&[&said]), "Let me check the tests");
    assert_eq!(words_of(&answered.after_the_last_tool), WITHOUT_ANSWER);
}

#[test]
fn the_long_runs_oracle_judges_a_result_by_the_engines_own_words() {
    // cf-evals depends on no crate of the product, so it keeps the engine's lines in its own
    // words (`long_run::invariants::order`): held to them here.
    let oracle = include_str!("../../../cf-evals/src/long_run/invariants/order.rs");
    for line in [LAST_WORDS, WITHOUT_ANSWER] {
        assert!(oracle.contains(&format!("{line:?}")), "{line}");
    }
}
