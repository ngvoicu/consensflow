//! `cf tell T-3 "…"` stops nothing (TAN-02). A tell to a task at work or waiting is a note kept for
//! its window, as a tell to a queued task is: nothing is pressed, no state moves, no stop is
//! asked, and the words go in when the window's turn ends, before anything is taken as the
//! task's result. The member owes no answer. What a tell at work needs beyond the route: a tell
//! given up fails nothing; a result that ended the turn before the words went in is not lost;
//! a tell held at the window is said, and Finish withdraws the words it waits for.
//! None of these tests is held to a Node recording: Node had no such tell.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_harness::contract::{Held, Readiness};
use cf_ledger::MessageView;
use std::cell::Cell;
use std::rc::Rc;

use crate::background::{results, working};
use crate::fixtures::{given, Tiers};

/// A write-up that is a report, not a line about where the worker stands: over 500 characters.
const WRITE_UP: &str = "# T-1: the parser is done\n\nThe grammar is v1, the lexer handles the five token kinds, and the checks pass: the parser suite is green in full, the fuzz run found nothing in 200,000 inputs, and the bench is within 3% of the last run. The build folder was removed and the tree is clean.\n\nOpen points: the error messages should name the line. Head 1a2b3c4d5e6f on origin/wt/parser.";

/// A line a worker ends its turn on, under 500 characters, which is its result all the same.
const DONE: &str = "Done, head abc123";

/// The chief tells T-1's window `body`, a moment after the window began its turn.
fn tell(context: &Context, tiers: &Tiers<'_>, body: &str) -> MessageView {
    context.advance(1_000);
    context
        .ledger
        .borrow_mut()
        .tell_queued(tiers.project.id, 1, "chief", body)
        .expect("the tell is kept for the window")
}

/// zeus at work on T-1, its turn running, and the chief's tell kept for it.
fn told_at_work<'a>(context: &'a Context, body: &str) -> (Tiers<'a>, MessageView) {
    let tiers = working(context);
    context.adapter.busy("zeus");
    let told = tell(context, &tiers, body);
    (tiers, told)
}

/// The notes the chief was given.
fn chief_notes(tiers: &Tiers<'_>) -> Vec<String> {
    tiers.notes("chief")
}

#[test]
fn a_tell_to_a_working_task_presses_no_key_moves_no_state_and_asks_no_stop() {
    let context = Context::new();
    let (tiers, told) = told_at_work(&context, "Use the v2 grammar");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working", "no state moved");
    assert_eq!(context.message(told.id).state, "queued", "kept, not pasted");
    assert_eq!(
        context.host.inputs(),
        Vec::<serde_json::Value>::new(),
        "no key pressed: no Escape, no Enter"
    );
    let asked = tiers
        .task(1)
        .messages
        .iter()
        .filter(|message| message.urgent)
        .count();
    assert_eq!(asked, 0, "no urgent question, so no stop owed");
    assert_eq!(
        given(&context, &tiers.assignee(1)).len(),
        1,
        "only the brief"
    );
}

#[test]
fn the_words_of_a_tell_at_work_go_in_at_the_first_rest_before_anything_is_the_result_and_the_turn_after_them_is_the_tasks(
) {
    let context = Context::new();
    let (tiers, told) = told_at_work(&context, "Use the v2 grammar");
    let session = tiers.assignee(1);
    context.adapter.answer("zeus", "Parser, a first draft");
    context.pass().unwrap();
    context.pass().unwrap();
    let words = given(&context, &session);
    assert_eq!(words.len(), 2, "{words:?}");
    assert!(
        words[1].starts_with(&format!(
            "[ConsensFlow m-{} · T-1 · note from @chief]\nUse the v2 grammar",
            told.id
        )),
        "{words:?}"
    );
    assert_eq!(context.message(told.id).state, "delivered");
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "the first draft is no result"
    );
    assert_eq!(results(&context, &tiers), Vec::<String>::new());

    context.adapter.answer("zeus", "Parser, on the v2 grammar");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "done");
    let last = results(&context, &tiers).pop().expect("a result");
    assert!(last.ends_with("Parser, on the v2 grammar"), "{last}");
}

#[test]
fn a_report_of_any_length_that_ended_the_turn_before_the_tell_went_in_is_in_the_result_in_front_of_the_last_words(
) {
    for report in [DONE, WRITE_UP] {
        let context = Context::new();
        let (tiers, told) = told_at_work(&context, "Use the v2 grammar");
        context.adapter.answer("zeus", report);
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(context.message(told.id).state, "delivered");
        context.adapter.answer("zeus", "Noted.");
        for _ in 0..3 {
            context.pass().unwrap();
        }
        assert_eq!(tiers.task(1).task.state, "done");
        assert_eq!(
            results(&context, &tiers),
            [format!(
                "Written before the note m-{} went in:\n\n{report}\n\nWhat it said last:\n\nNoted.",
                told.id
            )],
            "the report, then the last words"
        );
    }
}

#[test]
fn a_tell_whose_delivery_is_given_up_fails_nothing_on_a_working_task_and_releases_its_result() {
    let context = Context::new();
    let (tiers, told) = told_at_work(&context, "Use the v2 grammar");
    context.adapter.answer("zeus", DONE);
    context.adapter.with("zeus", |agent| agent.admit = false);
    for _ in 0..5 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(told.id).state, "failed");
    let thread = tiers.task(1);
    assert_ne!(thread.task.state, "failed", "the work is not thrown away");
    assert_eq!(thread.task.state, "done", "the result is released");
    assert_eq!(results(&context, &tiers), [DONE]);
    let said: Vec<String> = chief_notes(&tiers)
        .into_iter()
        .filter(|note| note.contains(&format!("m-{}", told.id)))
        .collect();
    assert_eq!(
        said.len(),
        1,
        "the chief is told once, with its words: {said:?}"
    );
    assert!(said[0].contains("Use the v2 grammar"), "{said:?}");
}

/// What holds a tell at the window, imposed after the window's turn ended.
enum Hold {
    Unsent,
    Dialog,
    Gate,
}

#[test]
fn a_tell_held_at_its_window_is_said_on_the_board_and_finish_withdraws_the_words_and_says_so() {
    for (name, hold) in [
        ("unsent", Hold::Unsent),
        ("dialog", Hold::Dialog),
        ("the human's approval", Hold::Gate),
    ] {
        let context = Context::new();
        let tiers = working(&context);
        let session = tiers.assignee(1);
        context.adapter.busy("zeus");
        let asked = Rc::new(Cell::new(true));
        match hold {
            Hold::Unsent => {
                let asked = Rc::clone(&asked);
                *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
                    Ok(if asked.get() {
                        Readiness::Held(Held::Unsent)
                    } else {
                        Readiness::Ready
                    })
                }));
            }
            Hold::Dialog => {
                let asked = Rc::clone(&asked);
                *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
                    Ok(if asked.get() {
                        Readiness::Held(Held::ForHuman("a dialog of its harness is open".into()))
                    } else {
                        Readiness::Ready
                    })
                }));
            }
            Hold::Gate => {
                context
                    .ledger
                    .borrow_mut()
                    .set_gate(tiers.project.id, true)
                    .unwrap();
            }
        }
        let told = tell(&context, &tiers, "Use the v2 grammar");
        context.adapter.answer("zeus", DONE);
        for _ in 0..3 {
            context.pass().unwrap();
        }
        assert_ne!(
            context.message(told.id).state,
            "delivered",
            "held by {name}"
        );
        assert_eq!(tiers.task(1).task.state, "working", "no result while held");
        assert_eq!(given(&context, &session).len(), 1, "nothing pasted");

        let finished = context
            .finish_task(tiers.project.id, 1, "chief")
            .expect("finish takes the result as it stands");
        assert_eq!(finished.task.state, "done");
        assert_eq!(
            finished.withdrawn.iter().map(|m| m.id).collect::<Vec<_>>(),
            [told.id],
            "the reply says which words were withdrawn"
        );
        assert_eq!(context.message(told.id).state, "cancelled");
        // The human's approval holds the result itself as well.
        if !name.contains("approval") {
            let result = results(&context, &tiers);
            assert!(
                result.len() == 1 && result[0].starts_with(DONE),
                "{name}: {result:?}"
            );
        }
    }
}

#[test]
fn a_tell_held_by_unsent_text_is_said_on_the_board() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.busy("zeus");
    let typed = Rc::new(Cell::new(true));
    let asked = Rc::clone(&typed);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        Ok(if asked.get() {
            Readiness::Held(Held::Unsent)
        } else {
            Readiness::Ready
        })
    }));
    let told = tell(&context, &tiers, "Use the v2 grammar");
    context.adapter.answer("zeus", DONE);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(
        context
            .dispatcher
            .holding(tiers.id(&tiers.assignee(1)))
            .unwrap(),
        "the board says what holds the words"
    );
    typed.set(false);
    context.pass().unwrap();
    assert_ne!(
        context.message(told.id).state,
        "queued",
        "goes in once the text is sent"
    );
    assert!(!context
        .dispatcher
        .holding(tiers.id(&tiers.assignee(1)))
        .unwrap());
}

#[test]
fn a_tell_to_a_waiting_task_goes_in_at_rest_and_the_members_question_still_waits() {
    let context = Context::new();
    let tiers = working(&context);
    context.ask(
        tiers.project.id,
        &tiers.assignee(1),
        "chief",
        1,
        "Which grammar, v1 or v2?",
    );
    context.adapter.busy("zeus");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");
    let told = tell(&context, &tiers, "Use the v2 grammar");
    context.adapter.answer("zeus", "Waiting for your answer.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(told.id).state, "delivered", "in at rest");
    assert_eq!(
        tiers.task(1).task.state,
        "waiting",
        "the question still waits"
    );
    assert_eq!(
        results(&context, &tiers),
        Vec::<String>::new(),
        "nothing is a result"
    );
}

#[test]
fn a_tell_to_a_paused_task_is_kept_and_goes_in_with_the_resume() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.answer("zeus", "Parser, a first draft");
    context.pause_task(tiers.project.id, 1);
    let told = tell(&context, &tiers, "Use the v2 grammar");
    assert_eq!(
        context.message(told.id).state,
        "queued",
        "kept for the resume"
    );
    context.pass().unwrap();
    assert_eq!(
        context.message(told.id).state,
        "queued",
        "a paused task takes nothing"
    );
    context.resume_task(tiers.project.id, 1, "Carry on");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(told.id).state, "delivered");
    let words = given(&context, &tiers.assignee(1));
    let last = words.last().expect("a paste");
    assert!(last.contains("Resumed: Carry on"), "{last}");
    assert!(
        last.contains("Use the v2 grammar"),
        "with the resume: {last}"
    );
}

#[test]
fn a_tell_that_crosses_the_result_is_decided_by_the_ledgers_order() {
    // The tell first: the result waits for the words.
    let context = Context::new();
    let (tiers, told) = told_at_work(&context, "Use the v2 grammar");
    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.state,
        "working",
        "the result waits for the words"
    );
    assert!(results(&context, &tiers).is_empty());
    assert!(
        matches!(
            context.message(told.id).state.as_str(),
            "delivering" | "delivered"
        ),
        "in at rest"
    );

    // The result first: the tell is refused.
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.answer("zeus", DONE);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    let refused =
        context
            .ledger
            .borrow_mut()
            .tell_queued(tiers.project.id, 1, "chief", "Use the v2 grammar");
    assert!(refused.is_err(), "no window to tell: {refused:?}");
}

#[test]
fn a_tell_over_sixteen_thousand_characters_is_read_in_parts_as_any_message_is() {
    let context = Context::new();
    let long = format!("{}END", "word ".repeat(4_000));
    let (tiers, told) = told_at_work(&context, &long);
    context.adapter.answer("zeus", "Parser, a first draft");
    context.pass().unwrap();
    context.pass().unwrap();
    let words = given(&context, &tiers.assignee(1));
    let pasted = words.last().expect("a paste");
    assert!(pasted.contains(&marker_of(told.id)), "{pasted}");
    assert!(
        pasted.contains(&format!("read all of it with: cf inbox read m-{}", told.id)),
        "{}",
        &pasted[pasted.len().saturating_sub(200)..]
    );
    assert!(!pasted.contains("END"), "the end is read in parts");
}

#[test]
fn a_window_that_closes_with_a_tell_waiting_tells_the_teller_once_and_fails_nothing() {
    let context = Context::new();
    let (tiers, told) = told_at_work(&context, "Use the v2 grammar");
    context.exit("zeus");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let thread = tiers.task(1);
    assert_eq!(
        thread.task.state, "paused",
        "the task is paused, not failed"
    );
    assert_ne!(context.message(told.id).state, "failed");
    let said: Vec<String> = chief_notes(&tiers)
        .into_iter()
        .filter(|note| note.contains(&format!("m-{}", told.id)))
        .collect();
    assert_eq!(said.len(), 1, "the teller is told once: {said:?}");
}
