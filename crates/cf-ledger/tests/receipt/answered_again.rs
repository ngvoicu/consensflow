//! An answer that did not stand leaves its task waiting for another, which
//! only the one asked can give: it is told so, in the words the decline of an
//! answer uses, when the answer fails to arrive and when a task is sent back
//! while the answer waited for the human, and not when nothing is owed.

use crate::fixture::{gated_world, world, World};
use crate::questions::{answered_again_and_received, answered_at_the_gate};

/// The notes a participant was sent, oldest first: the task each is about,
/// and what it says.
fn notes(w: &World, handle: &str) -> Vec<(Option<i64>, String)> {
    let mut sent: Vec<_> = w
        .ledger
        .inbox(w.id(handle), 100)
        .expect("an inbox")
        .into_iter()
        .filter(|message| message.kind == "note")
        .map(|message| (message.task_number, message.body))
        .collect();
    sent.reverse();
    sent
}

/// What an answerer is told to do.
fn again(question: i64) -> String {
    format!("Answer it again: cf answer m-{question} \"…\"")
}

#[test]
fn an_answer_whose_delivery_failed_for_a_question_withdrawn_at_the_gate_tells_the_chief_to_answer_it_again(
) {
    let mut w = gated_world();
    let (task, question, answer) = answered_at_the_gate(&mut w);
    w.ledger
        .approve_message(answer, "human")
        .expect("passed on");
    w.begin(answer);
    w.ledger
        .fail_delivery(answer, "the window closed")
        .expect("given up");
    assert_eq!(
        notes(&w, "chief"),
        [(
            Some(task),
            format!(
                "Your answer m-{answer} to m-{question} did not reach @zeus: the window closed. {}",
                again(question)
            )
        )],
        "the one asked, and the question, not the answer, is what it is told to answer"
    );

    // It does: the task goes on.
    answered_again_and_received(&mut w, question);
    assert_eq!(w.state(task), "working");
}

#[test]
fn an_answer_whose_delivery_failed_for_any_question_tells_the_one_asked_the_same() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?").id;
    let answer = w.answer(question, "JSON").id;
    w.begin(answer);
    w.ledger
        .fail_delivery(answer, "the window closed")
        .expect("given up");
    assert_eq!(
        notes(&w, "chief"),
        [(
            Some(task),
            format!(
                "Your answer m-{answer} to m-{question} did not reach @zeus: the window closed. {}",
                again(question)
            )
        )]
    );
}

#[test]
fn nobody_is_told_of_an_answer_that_failed_for_a_task_that_is_over_or_whose_asker_left() {
    // The task ended while the answer was on its way: nobody waits for another.
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?").id;
    let answer = w.answer(question, "JSON").id;
    w.begin(answer);
    w.ledger
        .record_result(w.project, task, "Parser, done")
        .expect("a result");
    w.ledger
        .fail_delivery(answer, "the window closed")
        .expect("given up");
    assert_eq!(notes(&w, "chief"), []);

    // The one asked left the project: the failure is still recorded, and nothing is sent.
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?").id;
    let answer = w.answer(question, "JSON").id;
    w.begin(answer);
    w.edit(|db| {
        db.execute(
            "UPDATE participant SET left_at = '2026-10-06T12:00:00.000Z' WHERE handle = 'chief'",
            [],
        )
        .expect("the chief left");
    });
    w.ledger
        .fail_delivery(answer, "the window closed")
        .expect("given up all the same");
    assert_eq!(w.states(&[answer]), ["failed"]);
}

#[test]
fn a_task_sent_back_while_an_answer_waited_for_the_human_tells_the_chief_to_answer_it_again() {
    let mut w = gated_world();
    let (task, question, answer) = answered_at_the_gate(&mut w);
    w.ledger
        .record_result(w.project, task, "Parser, as far as it goes")
        .expect("a result");
    // The human sends it back: what still waited at the gate is withdrawn.
    let words = w
        .ledger
        .reopen_task(w.project, task, "human", "Try again")
        .expect("sent back")
        .message
        .expect("its words");
    assert_eq!(w.states(&[question, answer]), ["cancelled", "cancelled"]);
    assert_eq!(
        notes(&w, "chief"),
        [(
            Some(task),
            format!(
                "T-{task} was sent back by @human, and m-{question} still waits for an answer: the one it had did not stand. {}",
                again(question)
            )
        )]
    );

    w.deliver(words.id);
    assert_eq!(w.state(task), "waiting", "it is owed the answer");
    answered_again_and_received(&mut w, question);
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_task_sent_back_after_it_failed_with_its_answer_cancelled_tells_the_chief_the_same() {
    let mut w = gated_world();
    let (task, question, answer) = answered_at_the_gate(&mut w);
    w.ledger
        .approve_message(answer, "human")
        .expect("passed on");
    w.ledger
        .fail_task(w.project, task, "its pane died")
        .expect("failed");
    assert_eq!(notes(&w, "chief"), [], "nothing is owed while it is over");
    w.ledger
        .reopen_task(w.project, task, "chief", "Try again")
        .expect("sent back");
    assert_eq!(
        notes(&w, "chief"),
        [(
            Some(task),
            format!(
                "T-{task} was sent back by @chief, and m-{question} still waits for an answer: the one it had did not stand. {}",
                again(question)
            )
        )]
    );
}

#[test]
fn a_task_sent_back_tells_nobody_of_a_question_that_never_had_an_answer_or_has_one_on_its_way() {
    // Never answered: nothing to answer again, and nobody ever saw it.
    let mut w = gated_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.ledger.approve_message(brief, "human").expect("passed on");
    w.deliver(brief);
    w.ask("zeus", task, "Which format?");
    w.ledger
        .fail_task(w.project, task, "its pane died")
        .expect("failed");
    w.ledger
        .reopen_task(w.project, task, "chief", "Try again")
        .expect("sent back");
    assert_eq!(notes(&w, "chief"), []);

    // Answered, and the answer waits to be pasted: it will be. One that was
    // withdrawn before it, and answered again, makes no difference.
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?").id;
    let first = w.answer(question, "JSON").id;
    w.ledger
        .cancel_message(first, "withdrawn by the chief")
        .expect("withdrawn");
    w.answer(question, "JSON, then");
    w.ledger
        .record_result(w.project, task, "Parser, as far as it goes")
        .expect("a result");
    w.ledger
        .reopen_task(w.project, task, "chief", "Try again")
        .expect("sent back");
    assert_eq!(notes(&w, "chief"), []);
}

#[test]
fn declining_an_answer_tells_the_chief_in_the_words_the_others_use() {
    let mut w = gated_world();
    let (task, question, answer) = answered_at_the_gate(&mut w);
    w.ledger.decline_message(answer, "human").expect("declined");
    assert_eq!(
        notes(&w, "chief"),
        [(
            Some(task),
            format!(
                "@human declined your answer to m-{question}. {}",
                again(question)
            )
        )]
    );
}
