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
use crate::turn_end::pending;

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

/// The chief's late answer to `question`, and what refuses it.
fn refused(context: &Context, tiers: &Tiers<'_>, question: i64) -> String {
    let chief = tiers.id("chief");
    let refusal = context
        .ledger
        .borrow_mut()
        .answer(question, chief, Some(&serde_json::json!("Late")), None)
        .expect_err("a closed question takes no answer");
    format!("{refusal}")
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
    assert_eq!(
        context.message(first).state,
        "cancelled",
        "the question is closed"
    );
    assert_ne!(
        context.message(second).state,
        "cancelled",
        "the answered one is not closed"
    );
    // Its question can no longer be answered, and the refusal says what works on a done task.
    let words = refused(&context, &tiers, first);
    assert!(
        words.contains(&format!(
            "m-{first} is closed: T-1 gave its result with it unanswered. To send words to its window, reopen the task: `cf task reopen T-1"
        )),
        "{words}"
    );
    assert!(!words.contains("cf tell"), "{words}");
    assert!(!results[0].contains("cf tell"), "{results:?}");

    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .expect("accepted");
    let words = refused(&context, &tiers, first);
    assert!(
        words.contains("add a follow-up: `cf task add --after T-1"),
        "{words}"
    );
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

/// A quick chief: the answer to the first question is written while the window is at work, the
/// window asks a second question in that same turn and ends it, and only then is the answer
/// pasted. The answer was written before the second question existed, so it says nothing of
/// it: the turn it begins is no result, though it asked nothing, and the task keeps waiting
/// (T-2268's finding 8).
#[test]
fn an_answer_written_before_the_next_question_is_no_result_after_it() {
    let context = Context::new();
    let tiers = working(&context);
    let project = tiers.project.id;
    let worker = tiers.assignee(1);
    context.adapter.busy("zeus");
    let first = context.ask(project, &worker, "chief", 1, "Which grammar?");
    context.pass().unwrap();
    context.answer(project, first.id, "PEG");
    context.pass().unwrap();
    let second = context.ask(project, &worker, "chief", 1, "Which format?");
    context.pass().unwrap();
    context.adapter.answer("zeus", "I asked which format");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting", "the second is open");

    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_ne!(context.message(second.id).state, "cancelled");
}

/// A waiting task whose result is held for background jobs: when the jobs end and the held
/// result is taken, it is delivered and the open question is closed and named.
#[test]
fn a_result_held_for_background_jobs_closes_the_open_question_when_it_is_taken() {
    let context = Context::new();
    let (tiers, first, second) = asked_two(&context);
    context.answer(tiers.project.id, second, "JSON");
    context.pass().unwrap();
    context.pass().unwrap();

    context
        .adapter
        .answer_leaving("zeus", "The suite runs in the background", pending(1, 0));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(results(&context, &tiers), Vec::<String>::new(), "held");

    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    let results = results(&context, &tiers);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(results[0].contains(DONE), "{results:?}");
    assert!(
        results[0].contains(&format!("m-{first} was left unanswered and is closed")),
        "{results:?}"
    );
}

/// The human keeps the window open past its result: the question named closed is closed all
/// the same.
#[test]
fn a_question_closed_by_the_result_is_closed_in_a_window_the_human_keeps_open() {
    let context = Context::new();
    let tiers = working(&context);
    let project = tiers.project.id;
    let worker = tiers.assignee(1);
    context.open_window(project, &worker).unwrap();
    let first = context.ask(project, &worker, "chief", 1, "Which grammar?");
    let second = context.ask(project, &worker, "chief", 1, "Which format?");
    context.adapter.answer("zeus", "I asked two questions");
    context.pass().unwrap();
    context.pass().unwrap();
    context.answer(project, second.id, "JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(context.message(first.id).state, "cancelled");
    refused(&context, &tiers, first.id);
}

/// A question from before holding tracking is closed by the result too.
#[test]
fn a_question_from_before_tracking_is_closed_by_the_result() {
    let context = Context::new();
    let (tiers, first, second) = asked_two(&context);
    context
        .ledger
        .borrow_mut()
        .write_raw(&format!(
            "DELETE FROM question_binding WHERE question_id = {first}"
        ))
        .unwrap();
    context.answer(tiers.project.id, second, "JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(context.message(first).state, "cancelled");
    refused(&context, &tiers, first);
}

/// The turn that asked a third question ends while its answer, queued or held for the human,
/// has not gone in: the task waits for it and for the turn after it.
fn an_answer_still_on_its_way_holds_the_result(gated: bool) {
    let context = Context::new();
    let (tiers, _first, second) = asked_two(&context);
    let project = tiers.project.id;
    context.answer(project, second, "JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.busy("zeus");
    let third = context.ask(project, &tiers.assignee(1), "chief", 1, "Which dialect?");
    context.pass().unwrap();
    if gated {
        context.ledger.borrow_mut().set_gate(project, true).unwrap();
    }
    let answer = context.answer(project, third.id, "SQLite");
    context.pass().unwrap();

    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    if gated {
        context.pass().unwrap();
        assert_eq!(results(&context, &tiers), Vec::<String>::new());
        context
            .ledger
            .borrow_mut()
            .approve_message(answer.id, "human")
            .unwrap();
        context
            .ledger
            .borrow_mut()
            .set_gate(project, false)
            .unwrap();
    }
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(results(&context, &tiers), Vec::<String>::new());
    assert_eq!(context.message(answer.id).state, "delivered");

    context
        .adapter
        .answer("zeus", "Applied SQLite and finished");
    context.pass().unwrap();
    context.pass().unwrap();
    let results = results(&context, &tiers);
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(
        results[0].contains("Applied SQLite and finished"),
        "{results:?}"
    );
}

#[test]
fn a_queued_answer_on_its_way_holds_the_result() {
    an_answer_still_on_its_way_holds_the_result(false);
}

#[test]
fn a_gated_answer_on_its_way_holds_the_result() {
    an_answer_still_on_its_way_holds_the_result(true);
}

/// How the window that asked went, with nothing new reaching its successor.
#[derive(Clone, Copy)]
enum Gone {
    Close,
    Restart,
    Window,
}

/// A turn that asked before the project closed, or the app restarted, or the window went, is
/// no result afterwards.
fn an_old_turn_is_not_taken_after(gone: Gone) {
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let tiers = working(&context);
    let project = tiers.project.id;
    let worker = tiers.assignee(1);
    let old = context.ask(project, &worker, "chief", 1, "An old question");
    context.adapter.answer("zeus", "I asked");
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .write_raw(&format!(
            "DELETE FROM question_binding WHERE question_id = {}",
            old.id
        ))
        .unwrap();
    context
        .ledger
        .borrow_mut()
        .tell_queued(project, 1, "chief", "Start with the defaults.")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    context.ask(project, &worker, "chief", 1, "Can I change the defaults?");
    context
        .adapter
        .answer("zeus", "The cleanup ran on the old defaults.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");

    match gone {
        Gone::Close => {
            context.close_project(project).unwrap();
            context.resume_project(project).unwrap();
            for _ in 0..6 {
                context.pass().unwrap();
            }
        }
        Gone::Restart => {
            context.ledger.borrow_mut().suspend_for_restart().unwrap();
            let after = context.make();
            after.settle_after_restart().unwrap();
            after.resume_project(project).unwrap();
            for _ in 0..6 {
                after.pass().unwrap();
            }
        }
        Gone::Window => {
            context.exit("zeus");
            context.open_window(project, &worker).unwrap();
            for _ in 0..6 {
                context.pass().unwrap();
            }
        }
    }

    assert_eq!(results(&context, &tiers), Vec::<String>::new());
}

#[test]
fn a_turn_from_before_a_close_is_no_result() {
    an_old_turn_is_not_taken_after(Gone::Close);
}

#[test]
fn a_turn_from_before_a_restart_is_no_result() {
    an_old_turn_is_not_taken_after(Gone::Restart);
}

#[test]
fn a_turn_from_before_its_window_went_is_no_result() {
    an_old_turn_is_not_taken_after(Gone::Window);
}

/// A turn that ended before the project closed, and that no pass looked at, is not taken as
/// the result once the project is resumed.
#[test]
fn a_turn_that_ended_before_a_close_unlooked_at_is_no_result() {
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let (tiers, first, second) = asked_two(&context);
    let project = tiers.project.id;
    context
        .ledger
        .borrow_mut()
        .write_raw(&format!(
            "DELETE FROM question_binding WHERE question_id IN ({first}, {second})"
        ))
        .unwrap();
    context
        .ledger
        .borrow_mut()
        .tell_queued(project, 1, "chief", "Start with the defaults.")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", DONE);
    context.close_project(project).unwrap();
    context.resume_project(project).unwrap();
    for _ in 0..6 {
        context.pass().unwrap();
    }

    assert_eq!(results(&context, &tiers), Vec::<String>::new());
}

/// A question of another task in the same window is not closed with the result.
#[test]
fn a_question_of_another_task_is_not_closed_by_the_result() {
    let context = Context::new();
    let (tiers, first, second) = asked_two(&context);
    let project = tiers.project.id;
    let other_task = context
        .pool_task(project, "worker", "Another task for the same session")
        .task;
    let other = context.ask(project, &tiers.assignee(1), "chief", 1, "About T-2");
    let window = context.id(project, &tiers.assignee(1));
    context
        .ledger
        .borrow_mut()
        .write_raw(&format!(
            "UPDATE task SET assignee_id = {window}, state = 'paused' WHERE id = {t};
             DELETE FROM question_binding WHERE question_id = {q};
             UPDATE message SET task_id = {t} WHERE id = {q};",
            t = other_task.id,
            q = other.id
        ))
        .unwrap();
    context.answer(project, second, "JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(context.message(first).state, "cancelled");
    assert_ne!(context.message(other.id).state, "cancelled");
}
