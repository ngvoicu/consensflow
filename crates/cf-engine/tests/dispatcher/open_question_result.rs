//! A window's result is delivered while a question of its task is still open (field find
//! T-2197, a chief on another board, 2026-11-24).
//!
//! A worker asked two questions. The chief answered the second, the worker went on, finished
//! and wrote its final report, and the task stayed "waiting" because the first question had no
//! answer: the report never reached the chief until `cf task finish`. Now a turn that ends
//! with its result, begun by a message that came after every open question was asked, is the
//! task's result whatever is still open: it is delivered, the task leaves "waiting", and each
//! question still open is closed, named in the result, and refused an answer with the way on
//! (`cf tell`). A turn that itself asked a question and ended on it is still a wait.

use cf_engine::testing::Context;

use crate::background::{results, working};
use crate::fixtures::Tiers;

const DONE: &str = "Both are done: committed as fe76a3c";

/// Two questions from zeus about T-1, its turn ended on them.
fn asked_two<'a>(context: &'a Context) -> (Tiers<'a>, i64, i64) {
    let tiers = working(context);
    let project = tiers.project.id;
    let first = context.ask(project, &tiers.assignee(1), "chief", 1, "Which grammar?");
    let second = context.ask(project, &tiers.assignee(1), "chief", 1, "Which format?");
    context.adapter.answer("zeus", "I asked two questions");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");
    (tiers, first.id, second.id)
}

/// The field's order: the second question answered, the worker goes on and ends with its
/// result while the first has no answer.
#[test]
fn a_result_is_delivered_when_the_first_of_two_questions_is_still_open() {
    let context = Context::new();
    let (tiers, first, second) = asked_two(&context);
    context.answer(tiers.project.id, second, "JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting", "the first is open");

    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    let results = results(&context, &tiers);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(results[0].starts_with(DONE), "{results:?}");
    assert!(
        results[0].contains(&format!("m-{first} was left unanswered and is closed")),
        "{results:?}"
    );
    assert!(
        !results[0].contains(&format!("m-{second} was left")),
        "the answered one is not named: {results:?}"
    );
    assert_eq!(context.message(first).state, "cancelled", "the question is closed");
    assert_ne!(context.message(second).state, "cancelled", "the answered one is not closed");
    // Its question can no longer be answered, and the refusal says how to tell the worker.
    let chief = tiers.id("chief");
    let refused = context
        .ledger
        .borrow_mut()
        .answer(first, chief, Some(&serde_json::json!("Late")), None)
        .expect_err("a closed question takes no answer");
    let words = format!("{refused:?}");
    assert!(words.contains("cf tell T-1"), "{words}");
}

/// Both questions unanswered; the chief's tell opens a turn that ends with the result.
#[test]
fn a_result_after_resume_words_closes_both_open_questions_and_names_them() {
    let context = Context::new();
    let (tiers, first, second) = asked_two(&context);
    // The chief pauses the task and resumes it with words: its window gets them.
    context.pause_task(tiers.project.id, 1);
    context.pass().unwrap();
    context.resume_task(tiers.project.id, 1, "Use your judgment");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");

    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    let results = results(&context, &tiers);
    assert_eq!(results.len(), 1, "{results:?}");
    for question in [first, second] {
        assert!(
            results[0].contains(&format!("m-{question} was left unanswered and is closed")),
            "{results:?}"
        );
    }
}

/// A turn that asks a question after an earlier one was answered, and ends on it, is a wait:
/// no result, and the task keeps waiting.
#[test]
fn a_turn_that_asks_again_after_an_answer_and_ends_on_it_is_still_a_wait() {
    let context = Context::new();
    let tiers = working(&context);
    let project = tiers.project.id;
    let first = context.ask(project, &tiers.assignee(1), "chief", 1, "Which grammar?");
    context.adapter.answer("zeus", "I asked");
    context.pass().unwrap();
    context.pass().unwrap();
    context.answer(project, first.id, "PEG");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");

    let second = context.ask(project, &tiers.assignee(1), "chief", 1, "Which format?");
    context.adapter.answer("zeus", "I asked again");
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_ne!(context.message(second.id).state, "cancelled");
}

/// No reminder of a question closed by the result goes out, however long it has been.
#[test]
fn a_question_closed_by_the_result_is_never_reminded() {
    let context = Context::new();
    let (tiers, _first, second) = asked_two(&context);
    context.answer(tiers.project.id, second, "JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");

    context.advance(3 * 3_600_000);
    context.pass().unwrap();
    context.pass().unwrap();
    let reminders = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .filter(|message| message.body.contains("has waited"))
        .count();
    assert_eq!(reminders, 0);
}
