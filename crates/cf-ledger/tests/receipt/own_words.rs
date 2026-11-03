//! An answer in the human's own words ("Something else") to a question with options is an answer
//! like any other, up to the body limit (issue `long-own-words-answer-refused`, `911656f`): a
//! 6,000-character one was refused as `answer: empty pick` because a pick was capped at 1,200
//! characters (`MAX_TITLE * 10`) and one error named both an empty pick and a long one. The picks'
//! own rule is `questions.rs`'s; these go through `answer`, which also prints the answer under
//! the question's header and holds the printed text to the body limit.

use cf_ledger::model::MAX_BODY;
use serde_json::json;

use crate::fixture::{world, World};

/// A task given to zeus, its brief received, with a question that has options asked.
fn with_a_question(w: &mut World) -> (i64, i64) {
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    (task, w.ask_with_options("zeus", task).id)
}

/// What the answer prints before a pick: the question's header (`Colour`) and a colon.
const PRINTED_UNDER: &str = "Colour: ";

#[test]
fn a_6000_character_answer_in_the_humans_own_words_is_an_answer_like_any_other() {
    let mut w = world();
    let (task, question) = with_a_question(&mut w);
    // Words, spaces, a line break and a quote, as a person writes: not a label of the question.
    let mut own_words = format!("{}\"as it was\".\nEnd", "Why not both? ".repeat(425));
    own_words.push_str(&"!".repeat(6_000 - own_words.chars().count()));
    assert_eq!(own_words.chars().count(), 6_000);
    let chief = w.id("chief");
    let answer = w
        .ledger
        .answer(question, chief, None, Some(&json!([[own_words.clone()]])))
        .expect("the answer is taken");
    assert_eq!(answer.choices, json!([[own_words.clone()]]));
    assert_eq!(answer.body, format!("{PRINTED_UNDER}{own_words}"));
    w.deliver(answer.id);
    assert_eq!(w.state(task), "working");
}

/// The refusal of a pick that does not fit, with how long the longest could be.
fn too_long(room: usize) -> String {
    format!(
        "answer: pick too long (at most {room} characters here; the headers of the questions and the other picks count too)"
    )
}

#[test]
fn the_longest_pick_a_question_takes_is_the_body_limit_less_the_header_it_is_printed_under() {
    let longest = MAX_BODY - PRINTED_UNDER.len();
    let mut w = world();
    let (_, question) = with_a_question(&mut w);
    let chief = w.id("chief");
    let refused = w
        .ledger
        .answer(
            question,
            chief,
            None,
            Some(&json!([["x".repeat(longest + 1)]])),
        )
        .expect_err("one unit over the answer's limit");
    assert_eq!(refused.to_string(), too_long(longest));
    // The refusal left the question as it was: the same answer, one unit shorter, is taken.
    let answer = w
        .ledger
        .answer(question, chief, None, Some(&json!([["x".repeat(longest)]])))
        .expect("the longest answer");
    assert_eq!(answer.body.chars().count(), MAX_BODY);
}

/// The picks' limit is the answer's: the answer prints the header before the pick, so a pick of the
/// body limit's own length passed the picks' check and was refused by the body's, under another name
/// (`body must be text, not empty, at most 1000000 characters`), where the page says a pick too long
/// is `pick too long`. Found 2026-10-29 (T-969), mended the same day.
#[test]
fn a_pick_of_the_body_limit_is_refused_as_a_pick_too_long_and_not_as_a_body() {
    let mut w = world();
    let (_, question) = with_a_question(&mut w);
    let chief = w.id("chief");
    let refused = w
        .ledger
        .answer(
            question,
            chief,
            None,
            Some(&json!([["x".repeat(MAX_BODY)]])),
        )
        .expect_err("a pick as long as the body limit");
    assert_eq!(
        refused.to_string(),
        too_long(MAX_BODY - PRINTED_UNDER.len())
    );
    // In the text of an answer, which is read as picks line by line, the same.
    let refused = w
        .ledger
        .answer(question, chief, Some(&json!("x".repeat(MAX_BODY))), None)
        .expect_err("the same, by text");
    assert_eq!(
        refused.to_string(),
        too_long(MAX_BODY - PRINTED_UNDER.len())
    );
}
