//! A window's turn ends with words while a question of its task is open (field find T-2197, a
//! chief on another board, 2026-11-24).
//!
//! A worker asked two questions. The chief answered the second, the worker went on, finished
//! and wrote its final report, and the task stayed "waiting" because the first question had no
//! answer: the report never reached the chief until `cf task finish`. The engine never takes
//! such a turn's words as the result itself (three reviews found three ways its guess went
//! wrong). It tells the requester once, with the words and the line that says how to take them
//! (`cf task finish`), and the task keeps waiting with its questions open. Finish takes the
//! words as the result, closes each question still open and names it in the result.

use cf_engine::testing::Context;
use cf_ledger::{MessageView, TURN_ON_OPEN_QUESTION};

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

/// The notes the engine sent the requester of T-1 of a turn that ended on words with a question
/// open.
fn notes(context: &Context, tiers: &Tiers<'_>) -> Vec<MessageView> {
    context
        .task(tiers.project.id, 1)
        .messages
        .into_iter()
        .filter(|message| message.kind == "note" && message.body.contains(TURN_ON_OPEN_QUESTION))
        .collect()
}

/// The line that ends a note naming `questions`.
fn line(tiers: &Tiers<'_>, questions: &str, names: &str) -> String {
    format!(
        "(ConsensFlow: @{}'s turn ended on these words with its question{questions} {names} still open. If they are its result: `cf task finish T-1`.)",
        tiers.assignee(1)
    )
}

/// The invariant: with a question open, no result is taken by the engine, the task waits and the
/// question stays open.
fn nothing_taken(context: &Context, tiers: &Tiers<'_>, open: &[i64]) {
    assert_eq!(results(context, tiers), Vec::<String>::new());
    assert_eq!(tiers.task(1).task.state, "waiting");
    for question in open {
        assert_ne!(
            context.message(*question).state,
            "cancelled",
            "m-{question}"
        );
    }
}

/// The field's order: the second question answered, the worker goes on and ends with its
/// report while the first has no answer. The chief is told once; Finish takes it.
#[test]
fn the_field_report_is_told_once_and_finish_takes_it_as_the_result() {
    let context = Context::new();
    let (tiers, first, second) = asked_two(&context);
    context.answer(tiers.project.id, second, "JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(notes(&context, &tiers).len(), 0, "no words yet");

    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    context.pass().unwrap();
    context.pass().unwrap();

    nothing_taken(&context, &tiers, &[first]);
    let told = notes(&context, &tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(
        told[0].body,
        format!("{DONE}\n\n{}", line(&tiers, "", &format!("m-{first}")))
    );

    let finished = context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("Finish takes a waiting task");
    assert_eq!(finished.task.state, "done");
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
    assert_eq!(context.message(first).state, "cancelled");
    assert_ne!(context.message(second).state, "cancelled");
    let words = refused(&context, &tiers, first);
    assert!(
        words.contains(&format!(
            "m-{first} is closed: T-1 gave its result with it unanswered. To send words to its window, reopen the task: `cf task reopen T-1"
        )),
        "{words}"
    );
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

/// Both questions unanswered; the chief's resume words open a turn that ends with its report:
/// the note names both.
#[test]
fn a_report_after_resume_words_is_told_naming_both_questions() {
    let context = Context::new();
    let (tiers, first, second) = asked_two(&context);
    context.pause_task(tiers.project.id, 1);
    context.pass().unwrap();
    context.resume_task(tiers.project.id, 1, "Use your judgment");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");

    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    context.pass().unwrap();
    context.pass().unwrap();

    nothing_taken(&context, &tiers, &[first, second]);
    let told = notes(&context, &tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0]
            .body
            .ends_with(&line(&tiers, "s", &format!("m-{first} and m-{second}"))),
        "{told:?}"
    );
    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
    let results = results(&context, &tiers);
    for question in [first, second] {
        assert!(
            results[0].contains(&format!("m-{question} was left unanswered and is closed")),
            "{results:?}"
        );
    }
}

/// A turn that asks a question after an earlier one was answered, and ends on it: the window
/// asked and stopped, and the chief has the question. No note, no result.
#[test]
fn a_turn_that_asks_again_after_an_answer_and_ends_on_it_is_not_told() {
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
    context.pass().unwrap();

    nothing_taken(&context, &tiers, &[second.id]);
    assert_eq!(notes(&context, &tiers), Vec::<MessageView>::new());
}

/// A second look at the same ended turn tells nothing more.
#[test]
fn a_second_look_at_the_same_turn_tells_nothing_more() {
    let context = Context::new();
    let (tiers, first, second) = asked_two(&context);
    context.answer(tiers.project.id, second, "JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", DONE);
    for _ in 0..6 {
        context.pass().unwrap();
    }
    assert_eq!(notes(&context, &tiers).len(), 1);
    nothing_taken(&context, &tiers, &[first]);
    context.advance(3 * 3_600_000);
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(notes(&context, &tiers).len(), 1);
}

/// The next turn after another paste is told again, once.
#[test]
fn the_next_turn_after_a_new_paste_is_told_once_more() {
    let context = Context::new();
    let (tiers, first, second) = asked_two(&context);
    context.answer(tiers.project.id, second, "JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", DONE);
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(notes(&context, &tiers).len(), 1);
    context
        .ledger
        .borrow_mut()
        .tell_queued(tiers.project.id, 1, "chief", "One more thing: add a test.")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "The test is added.");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(notes(&context, &tiers).len(), 2);
    nothing_taken(&context, &tiers, &[first]);
}

/// A question closed by Finish is never reminded, however long it has been.
#[test]
fn a_question_closed_by_finish_is_never_reminded() {
    let context = Context::new();
    let (tiers, _first, second) = asked_two(&context);
    context.answer(tiers.project.id, second, "JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");
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

/// A turn whose background jobs still run is not over: nothing is told until the jobs end and
/// the turn after them ends with words.
#[test]
fn a_turn_held_for_background_jobs_is_told_when_it_ends() {
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
    assert_eq!(notes(&context, &tiers).len(), 0, "held");
    nothing_taken(&context, &tiers, &[first]);

    context.adapter.answer("zeus", DONE);
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(notes(&context, &tiers).len(), 1);
    nothing_taken(&context, &tiers, &[first]);
}

/// The human keeps the window open past Finish: the question named closed is closed all the
/// same.
#[test]
fn a_question_closed_by_finish_is_closed_in_a_window_the_human_keeps_open() {
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
    for _ in 0..4 {
        context.pass().unwrap();
    }
    nothing_taken(&context, &tiers, &[first.id]);
    context.finish_task(project, 1, "chief").expect("finished");

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(context.message(first.id).state, "cancelled");
    refused(&context, &tiers, first.id);
}

/// A question from before holding tracking is closed by Finish too.
#[test]
fn a_question_from_before_tracking_is_closed_by_finish() {
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
    for _ in 0..4 {
        context.pass().unwrap();
    }
    nothing_taken(&context, &tiers, &[first]);
    assert_eq!(notes(&context, &tiers).len(), 1);
    context
        .finish_task(tiers.project.id, 1, "chief")
        .expect("finished");

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(context.message(first).state, "cancelled");
    refused(&context, &tiers, first);
}

/// A quick chief: the answer to the first question is written while the window is at work, the
/// window asks a second question in that same turn and ends it, and only then is the answer
/// pasted. It does not matter when the answer was written: the paste came after the question,
/// so the turn it begins is told.
#[test]
fn an_answer_written_before_the_next_question_still_makes_its_turn_told() {
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
    for _ in 0..4 {
        context.pass().unwrap();
    }

    nothing_taken(&context, &tiers, &[second.id]);
    assert_eq!(notes(&context, &tiers).len(), 1);
}

/// The turn that asked a third question ends while its answer, queued or held for the human,
/// has not gone in: no result by itself, before or after the answer's turn.
fn an_answer_still_on_its_way_is_never_a_result(gated: bool) {
    let context = Context::new();
    let (tiers, first, second) = asked_two(&context);
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
    // The paste of the earlier answer came after the question still open: that turn is told,
    // as the plain condition says, though a third answer is on its way.
    if !gated {
        assert_eq!(notes(&context, &tiers).len(), 1);
    }
    if gated {
        context.pass().unwrap();
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
    assert_eq!(context.message(answer.id).state, "delivered");
    nothing_taken(&context, &tiers, &[first]);

    context
        .adapter
        .answer("zeus", "Applied SQLite and finished");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    nothing_taken(&context, &tiers, &[first]);
    let told = notes(&context, &tiers);
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(told[1].body.starts_with("Applied SQLite and finished"));
}

#[test]
fn a_queued_answer_on_its_way_is_never_a_result() {
    an_answer_still_on_its_way_is_never_a_result(false);
}

#[test]
fn a_gated_answer_on_its_way_is_never_a_result() {
    an_answer_still_on_its_way_is_never_a_result(true);
}

/// A question of another task in the same window is not closed with Finish.
#[test]
fn a_question_of_another_task_is_not_closed_by_finish() {
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
    for _ in 0..4 {
        context.pass().unwrap();
    }
    context.finish_task(project, 1, "chief").expect("finished");

    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(context.message(first).state, "cancelled");
    assert_ne!(context.message(other.id).state, "cancelled");
}
