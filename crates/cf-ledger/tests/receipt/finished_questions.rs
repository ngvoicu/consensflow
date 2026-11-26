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
fn the_windows_own_done_leaves_its_open_question_to_be_answered_as_it_always_did() {
    // The window's `cf task done` is its own word, not the chief's: the question it asked
    // stays open until the task is accepted (`chief_open_work`'s tests hold it).
    let mut w = world();
    let (task, question) = asked(&mut w);

    w.ledger
        .record_result(w.project, task, "Done after all")
        .expect("a result");
    assert_ne!(w.message(question).state, "cancelled");
    assert_eq!(answering(&mut w, question), None, "the answer is taken");
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
fn answering_or_finishing_withdraws_an_unhanded_reminder_and_a_returned_handed_one() {
    use cf_ledger::{HandOver, NewNote};
    for finish in [false, true] {
        for handed in [false, true] {
            let mut w = world();
            let (task, question) = asked(&mut w);
            w.deliver(question);
            let reminder = w
                .ledger
                .remind_question(
                    w.project,
                    question,
                    None,
                    &NewNote {
                        from: None,
                        to: "chief".into(),
                        task: Some(task),
                        body: "A reminder".into(),
                    },
                )
                .unwrap()
                .unwrap();
            w.begin(reminder.id);
            if handed {
                assert_eq!(
                    w.ledger.admit(reminder.id, &[]).unwrap(),
                    HandOver::Admitted
                );
            }
            if finish {
                w.ledger
                    .record_finished(w.project, task, "Done", (0, 0))
                    .unwrap();
            } else {
                w.answer(question, "JSON");
            }
            assert_eq!(
                w.message(reminder.id).state,
                if handed { "delivering" } else { "cancelled" },
                "finish={finish}, handed={handed}"
            );
            if handed {
                w.ledger
                    .retry_delivery(reminder.id, "the paste was refused", false)
                    .unwrap();
                assert_eq!(
                    w.message(reminder.id).state,
                    "cancelled",
                    "a returned reminder must not go again"
                );
            } else {
                assert_eq!(
                    w.ledger.admit(reminder.id, &[]).unwrap(),
                    HandOver::Withdrawn
                );
            }
        }
    }
}

#[test]
fn a_pending_question_reminder_cannot_stack_even_after_the_ledger_is_reopened() {
    use cf_ledger::NewNote;
    let mut w = world();
    let (task, question) = asked(&mut w);
    w.deliver(question);
    let note = NewNote {
        from: None,
        to: "chief".into(),
        task: Some(task),
        body: "A reminder".into(),
    };
    let first = w
        .ledger
        .remind_question(w.project, question, None, &note)
        .unwrap()
        .unwrap();
    assert!(w
        .ledger
        .remind_question(w.project, question, None, &note)
        .unwrap()
        .is_none());
    w.edit(|_| {});
    let open = w.ledger.waiting_questions(w.project).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].reminder.as_ref().unwrap().id, first.id);
    assert!(
        w.ledger
            .remind_question(w.project, question, Some(first.id), &note)
            .unwrap()
            .is_none(),
        "a still queued reminder cannot be followed by another"
    );
}

#[test]
fn migration_keeps_an_old_ledgers_delivered_question_and_its_receipt_time() {
    let mut w = world();
    let (task, question) = asked(&mut w);
    w.deliver(question);
    let before = w.message(question);
    let receipt = before
        .delivered_at
        .clone()
        .expect("the question was received");
    w.edit(|db| {
        cf_ledger::testing::undo_migration_0048(db).expect("0048 undone");
        cf_ledger::testing::undo_migration_0047(db).unwrap();
        cf_ledger::testing::undo_migration_0046(db).expect("closed waits undone");
        cf_ledger::testing::undo_migration_0044(db).expect("holding bounds undone");
        cf_ledger::testing::undo_migration_0043(db).expect("question boundaries undone");
        cf_ledger::testing::undo_migration_0042(db).unwrap();
        db.execute_batch("PRAGMA user_version = 41").unwrap();
    });
    let after = w.message(question);
    assert_eq!(after.id, before.id);
    assert_eq!(after.state, before.state);
    assert_eq!(after.delivered_at, before.delivered_at);
    let open = w.ledger.waiting_questions(w.project).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, question);
    assert_eq!(open[0].task, Some(task));
    assert_eq!(open[0].received_at, receipt);
    assert!(open[0].reminder.is_none());
}
