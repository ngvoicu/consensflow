//! A question's task waits while it is outstanding, and only a received
//! answer ends that, by whatever way it was received: all of a window's
//! questions, held to one rule.

use cf_ledger::{Claim, Read};

use crate::fixture::{gated_world, world, World};

/// A task given to zeus, its brief received, with two questions asked.
fn with_two_questions(w: &mut World, options: bool) -> (i64, i64, i64) {
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let ask = |w: &mut World, words: &str| {
        if options {
            w.ask_with_options("zeus", task).id
        } else {
            w.ask("zeus", task, words).id
        }
    };
    let first = ask(w, "Which format?");
    let second = ask(w, "And which version?");
    assert_eq!(w.state(task), "waiting");
    (task, first, second)
}

#[test]
fn one_answer_received_leaves_the_task_waiting_for_the_other_question_and_the_second_makes_it_work()
{
    let mut w = world();
    let (task, first, second) = with_two_questions(&mut w, false);
    let one = w.answer(first, "JSON");
    w.deliver(one.id);
    assert_eq!(
        w.state(task),
        "waiting",
        "one question still has no answer received"
    );
    let two = w.answer(second, "v2");
    assert_eq!(
        w.state(task),
        "waiting",
        "an answer on its way is not one received"
    );
    w.deliver(two.id);
    assert_eq!(w.state(task), "working");
}

#[test]
fn the_same_through_a_doors_acknowledgement() {
    let mut w = world();
    let (task, first, second) = with_two_questions(&mut w, true);
    let zeus = w.id("zeus");
    let one = w.choose(first, "red");
    let two = w.choose(second, "blue");
    for question in [first, second] {
        // The door takes the answer, hands it to its tool, and says so.
        assert!(matches!(
            w.ledger.claim_answer(question, zeus).expect("a claim"),
            Claim::Answered(_)
        ));
    }
    w.ledger.settle_claim(one.id, zeus, true).expect("received");
    assert_eq!(
        w.state(task),
        "waiting",
        "the second answer is claimed, not received"
    );
    assert_eq!(w.receipt(one.id), serde_json::json!({ "door": true }));
    w.ledger.settle_claim(two.id, zeus, true).expect("received");
    assert_eq!(w.state(task), "working");
}

#[test]
fn the_same_through_what_cf_served_whole() {
    let mut w = world();
    let (task, first, second) = with_two_questions(&mut w, false);
    let zeus = w.id("zeus");
    let one = w.answer(first, "JSON");
    let two = w.answer(second, "v2");
    w.ledger
        .receive_read(zeus, &[one.id], Read::Inbox)
        .expect("read");
    assert_eq!(w.state(task), "waiting");
    assert_eq!(
        w.receipt(one.id),
        serde_json::json!({ "read": "inbox" }),
        "how it was served is the proof"
    );
    w.ledger
        .receive_read(zeus, &[two.id], Read::Task)
        .expect("read");
    assert_eq!(w.state(task), "working");
    assert_eq!(w.receipt(two.id), serde_json::json!({ "read": "task" }));
}

#[test]
fn what_cf_served_to_anyone_else_or_that_is_no_answer_for_the_reader_is_not_received() {
    let mut w = world();
    let (task, first, _) = with_two_questions(&mut w, false);
    let chief = w.id("chief");
    let diana = w.id("diana");
    let one = w.answer(first, "JSON");
    w.ledger
        .receive_read(chief, &[one.id], Read::Task)
        .expect("a read by the chief");
    w.ledger
        .receive_read(diana, &[one.id], Read::Inbox)
        .expect("a read by another");
    assert_eq!(
        w.states(&[one.id]),
        ["queued"],
        "an answer is its recipient's to receive"
    );
    assert_eq!(w.state(task), "waiting");
}

#[test]
fn a_question_answered_while_it_waits_at_the_gate_obliges_while_the_answer_is_on_its_way() {
    let mut w = gated_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.ledger.approve_message(brief, "human").expect("passed on");
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    assert_eq!(w.states(&[question.id]), ["gated"]);
    // The chief answers it before the human passed it on: it goes no further.
    let answer = w.answer(question.id, "JSON");
    assert_eq!(w.states(&[question.id, answer.id]), ["cancelled", "gated"]);

    // A reconciliation now (a note arrives) still finds the question obliging.
    let note = w.note("chief", "zeus", task, "Mind the tests");
    w.ledger
        .approve_message(note.id, "human")
        .expect("passed on");
    w.deliver(note.id);
    assert_eq!(
        w.state(task),
        "waiting",
        "the question was withdrawn from the gate, not resolved"
    );
    w.ledger
        .approve_message(answer.id, "human")
        .expect("passed on");
    w.deliver(answer.id);
    assert_eq!(w.state(task), "working");
}

#[test]
fn an_answer_the_human_declines_gives_the_question_back_to_the_gate_and_the_task_still_waits() {
    let mut w = gated_world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.ledger.approve_message(brief, "human").expect("passed on");
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    let answer = w.answer(question.id, "JSON");
    w.ledger
        .decline_message(answer.id, "human")
        .expect("declined");
    assert_eq!(
        w.states(&[question.id, answer.id]),
        ["gated", "cancelled"],
        "nobody received an answer: the question is the human's to pass on again"
    );
    let note = w.note("chief", "zeus", task, "Mind the tests");
    w.ledger
        .approve_message(note.id, "human")
        .expect("passed on");
    w.deliver(note.id);
    assert_eq!(w.state(task), "waiting");

    // The chief answers it again, and the new answer is received.
    let again = w.answer(question.id, "YAML");
    w.ledger
        .approve_message(again.id, "human")
        .expect("passed on");
    w.deliver(again.id);
    assert_eq!(w.state(task), "working");
}

#[test]
fn an_answer_whose_delivery_failed_leaves_the_question_to_take_another() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?");
    let answer = w.answer(question.id, "JSON");
    w.begin(answer.id);
    w.ledger
        .fail_delivery(answer.id, "the window closed")
        .expect("given up");
    assert_eq!(w.state(task), "waiting");
    let again = w.answer(question.id, "JSON, then");
    w.deliver(again.id);
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_question_asked_while_its_task_is_paused_or_queued_behind_the_words_that_resume_it_is_born_with_its_door_shut(
) {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let zeus = w.id("zeus");
    assert_eq!(
        w.ledger
            .task_in_hand(zeus)
            .unwrap()
            .map(|thread| thread.task.number),
        Some(task)
    );

    // Paused: the turn that asks is an old one, and it is told so at its first poll.
    w.pause(task);
    assert_eq!(
        w.ledger
            .active_task(zeus, true)
            .unwrap()
            .map(|thread| thread.task.number),
        None,
        "no longer the task it is working on"
    );
    assert_eq!(
        w.ledger
            .task_in_hand(zeus)
            .unwrap()
            .map(|thread| thread.task.number),
        Some(task),
        "but the task it is on: the question is about it"
    );
    let paused = w.ask_with_options("zeus", task);
    assert_eq!(
        w.ledger.claim_answer(paused.id, zeus).unwrap(),
        Claim::Closed
    );

    // Queued behind its resume words: the same. And the question the pause
    // found shut stays shut once the task goes on: its turn is over for good.
    let words = w.resume(task, "Go on").message.expect("its words");
    assert_eq!(w.state(task), "queued");
    assert_eq!(
        w.ledger.claim_answer(paused.id, zeus).unwrap(),
        Claim::Closed,
        "born shut, not shut by the task's state alone"
    );
    let queued = w.ask_with_options("zeus", task);
    assert_eq!(w.state(task), "queued");
    assert_eq!(
        w.ledger.claim_answer(queued.id, zeus).unwrap(),
        Claim::Closed
    );
    // The answers come as messages, with the words, once, whichever door asked.
    let first = w.choose(paused.id, "red");
    let second = w.choose(queued.id, "blue");
    let begun = w.deliver(words.id);
    assert_eq!(
        crate::fixture::ids(&begun.carried),
        [first.id, second.id],
        "an answer to a question the pause shut goes in the paste of the words"
    );
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_question_asked_while_its_task_works_has_its_door_open() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let zeus = w.id("zeus");
    let open = w.ask_with_options("zeus", task);
    assert_eq!(
        w.ledger.claim_answer(open.id, zeus).unwrap(),
        Claim::Waiting
    );
}
