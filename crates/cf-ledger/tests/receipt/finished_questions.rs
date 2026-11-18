//! A task finished while a question of its window is open leaves nobody waiting for the
//! answer (T-1119, from the review of T-1125): the chief's Finish takes the words a turn
//! ended on as the result, and the question those words were raised as (`turn_end::asks` of
//! the engine) is withdrawn with the move, so `cf answer` cannot reach it and the words are
//! not asked twice. A question with an answer that stands is its answerer's, and the chief's
//! own tell is not the window's question.

use cf_ledger::Claim;
use serde_json::json;

use crate::fixture::{world, World};

/// T-1 given to zeus, its brief received, and a question of its asked and given to the chief:
/// the task, the question.
fn asked(w: &mut World) -> (i64, i64) {
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask("zeus", task, "Which format?").id;
    assert_eq!(w.state(task), "waiting");
    (task, question)
}

/// What the chief is told on answering `question`: the code of the refusal, or none.
fn answering(w: &mut World, question: i64) -> Option<&'static str> {
    let chief = w.id("chief");
    w.ledger
        .answer(question, chief, Some(&json!("Tabs")), None)
        .err()
        .and_then(|refused| refused.code())
}

#[test]
fn a_waiting_task_finished_withdraws_its_open_question_and_the_chief_cannot_answer_it() {
    let mut w = world();
    let (task, question) = asked(&mut w);

    w.ledger
        .record_finished(w.project, task, "The words it ended on", (0, 0))
        .expect("finished");
    assert_eq!(w.state(task), "done");
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(
        w.message(question).reason.as_deref(),
        Some("T-1 was finished")
    );
    assert_eq!(answering(&mut w, question), Some("question-withdrawn"));
    // The chief's list of what waits for it has none.
    let open = w.ledger.chief_open_work(w.project).expect("the open work");
    assert!(open.questions.iter().all(|asked| asked.id != question));
}

#[test]
fn the_windows_own_done_withdraws_its_open_question() {
    let mut w = world();
    let (task, question) = asked(&mut w);

    w.ledger
        .record_result(w.project, task, "Done after all")
        .expect("a result");
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(answering(&mut w, question), Some("question-withdrawn"));
}

#[test]
fn a_failed_prior_answer_does_not_revive_a_finished_question_on_reopen() {
    let mut w = world();
    let (task, question) = asked(&mut w);
    let answer = w.answer(question, "JSON").id;
    w.ledger
        .cancel_message(answer, "withdrawn by the chief")
        .unwrap();
    w.ledger
        .record_result(w.project, task, "Done after all")
        .unwrap();
    assert_eq!(answering(&mut w, question), Some("question-withdrawn"));
    let words = w
        .ledger
        .reopen_task(w.project, task, "chief", "One more change")
        .unwrap()
        .message
        .unwrap();
    w.deliver(words.id);
    assert_eq!(w.state(task), "working");
    assert_eq!(answering(&mut w, question), Some("question-withdrawn"));
}

#[test]
fn opening_an_older_done_task_withdraws_its_unanswered_question() {
    let mut w = world();
    let (task, question) = asked(&mut w);
    w.ledger
        .record_result(w.project, task, "Done after all")
        .unwrap();
    w.edit(|db| {
        db.execute("UPDATE message SET state = 'delivered', reason = NULL, door_closed_at = NULL WHERE id = ?", [question]).unwrap();
    });
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(answering(&mut w, question), Some("question-withdrawn"));
}

#[test]
fn the_windows_own_done_closes_its_open_harness_door() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let task = created.task.number;
    w.deliver(created.message.unwrap().id);
    let question = w.ask_with_options("zeus", task).id;
    let zeus = w.id("zeus");
    w.ledger
        .record_result(w.project, task, "Done after all")
        .unwrap();
    assert_eq!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Closed
    );
}

#[test]
fn a_paused_task_finished_withdraws_the_question_it_was_paused_on() {
    let mut w = world();
    let (task, question) = asked(&mut w);
    w.pause(task);
    assert_eq!(w.state(task), "paused");

    w.ledger
        .record_finished(w.project, task, "The words it ended on", (0, 0))
        .expect("finished");
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(answering(&mut w, question), Some("question-withdrawn"));
}

#[test]
fn a_question_not_yet_given_to_the_chief_is_never_given() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    // The chief's queue is not read: the question is still queued.
    let question = w.ask("zeus", task, "Which format?").id;
    assert_eq!(w.message(question).state, "queued");

    w.ledger
        .record_finished(w.project, task, "The words it ended on", (0, 0))
        .expect("finished");
    assert_eq!(w.message(question).state, "cancelled");
    // What is next for the chief is the result, not the question.
    assert_ne!(
        w.next("chief"),
        Some(question),
        "the question is never given"
    );
}

#[test]
fn a_question_with_an_answer_that_stands_is_left_as_it_is() {
    let mut w = world();
    let (task, question) = asked(&mut w);
    let answer = w.answer(question, "Tabs");

    w.ledger
        .record_finished(w.project, task, "Done", (0, 0))
        .expect("finished");
    assert_ne!(w.message(question).state, "cancelled");
    assert_ne!(w.message(answer.id).state, "cancelled");
    // A second answer is refused as it always was.
    assert_eq!(answering(&mut w, question), Some("already-answered"));
}

#[test]
fn the_chiefs_tell_is_not_a_question_of_the_window_and_is_left_alone() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let session = w
        .ledger
        .task(w.project, task)
        .expect("a read")
        .expect("the task")
        .task
        .assignee
        .expect("its session");
    let told = w.tell(&session, task, "Use the new style").id;
    assert_eq!(w.state(task), "paused");

    w.ledger
        .record_finished(w.project, task, "Done", (0, 0))
        .expect("finished");
    assert_ne!(
        w.message(told).state,
        "cancelled",
        "the tell is the chief's"
    );
}

#[test]
fn a_question_open_at_a_harness_door_is_shut_out_of_waiting_by_the_finish() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let (task, brief) = (created.task.number, created.message.expect("a brief").id);
    w.deliver(brief);
    let question = w.ask_with_options("zeus", task).id;
    let zeus = w.id("zeus");
    assert_eq!(w.state(task), "waiting");

    w.ledger
        .record_finished(w.project, task, "The words it ended on", (0, 0))
        .expect("finished");
    assert_eq!(
        w.ledger.claim_answer(question, zeus).expect("a poll"),
        Claim::Closed,
        "the door is told at once, not when its poll gives up"
    );
}

#[test]
fn cancelling_with_a_delivered_question_closes_its_door_and_withdraws_it() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let task = created.task.number;
    w.deliver(created.message.unwrap().id);
    let question = w.ask_with_options("zeus", task).id;
    w.deliver(question);
    let zeus = w.id("zeus");
    assert_eq!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Waiting
    );

    w.ledger.cancel_task(w.project, task, "chief").unwrap();
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Closed
    );
    assert_eq!(answering(&mut w, question), Some("task-cancelled"));
}

#[test]
fn a_failed_task_closes_its_harness_door_while_it_waits_for_a_reopen() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let task = created.task.number;
    w.deliver(created.message.unwrap().id);
    let question = w.ask_with_options("zeus", task).id;
    w.deliver(question);
    let zeus = w.id("zeus");
    assert_eq!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Waiting
    );
    w.ledger
        .fail_task(w.project, task, "the window failed")
        .unwrap();
    assert_eq!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Closed
    );
    assert_eq!(answering(&mut w, question), Some("task-failed"));
    let reopened = w
        .ledger
        .reopen_task(w.project, task, "chief", "Try again")
        .unwrap();
    w.deliver(reopened.message.unwrap().id);
    assert_eq!(w.state(task), "waiting");
    let answer = w.choose(question, "red");
    assert_eq!(
        w.message(answer.id).state,
        "queued",
        "the same conversation still gets its answer"
    );
    assert_eq!(
        w.ledger.claim_answer(question, zeus).unwrap(),
        Claim::Closed
    );
}

#[test]
fn accepting_a_result_withdraws_the_unseen_question_before_the_chief_gets_it() {
    let mut w = world();
    let created = w.give("zeus", "Parser");
    let task = created.task.number;
    w.deliver(created.message.unwrap().id);
    let question = w.ask("zeus", task, "Which format?").id;
    assert_eq!(w.message(question).state, "queued");
    w.ledger.record_result(w.project, task, "Done").unwrap();
    // An older build could already have left the question queued on a done task.
    w.ledger
        .sqlite()
        .execute(
            "UPDATE message SET state = 'queued', reason = NULL WHERE id = ?",
            [question],
        )
        .unwrap();
    w.ledger.accept_task(w.project, task, "chief").unwrap();

    assert_eq!(w.message(question).state, "cancelled");
    assert_ne!(w.next("chief"), Some(question));
    assert_eq!(answering(&mut w, question), Some("task-accepted"));
}
