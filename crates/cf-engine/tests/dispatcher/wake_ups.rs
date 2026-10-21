//! The human's wake-up calls: a chief at rest is nudged on the project's
//! schedule by a note from the human, which goes through the usual queue. The
//! clock is the kit's (`Context::advance`). Not held to a Node recording: Node's
//! dispatcher never nudged a chief.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_engine::ActivityState;
use cf_harness::contract::{Held, Readiness, Waiting};
use cf_harness::records::Role;
use cf_ledger::{NewWakeUp, ProjectView, WakeUpUnit, DEFAULT_WAKE_UP_TEXT};
use serde_json::json;

use crate::fixtures::soon;

mod busy;
mod ending;

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// A project whose chief is at rest.
fn at_rest(context: &Context) -> ProjectView {
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        context
            .dispatcher
            .activity(context.id(project.id, "chief"))
            .state,
        ActivityState::Idle
    );
    project
}

/// The human sets the calls: `every` of `unit` apart, with the default words.
fn set(context: &Context, project: &ProjectView, on: bool, every: i64, unit: WakeUpUnit) {
    context
        .ledger
        .borrow_mut()
        .set_wake_up(
            project.id,
            &NewWakeUp {
                on,
                every,
                unit,
                text: None,
            },
        )
        .unwrap();
}

/// The wake-ups the chief's window has been given: the user items that hold the words.
fn given(context: &Context) -> usize {
    context
        .adapter
        .agent("chief")
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains(DEFAULT_WAKE_UP_TEXT))
        .count()
}

/// How many wake-ups the log says were queued.
fn queued(context: &Context, project: &ProjectView) -> usize {
    context
        .ledger
        .borrow()
        .events(project.id, 0, 5000)
        .unwrap()
        .iter()
        .filter(|event| event.kind == "project.wake_up")
        .count()
}

/// Something moves on the board and leaves nothing on its way: the chief gives
/// zeus a task and calls it off.
fn moved(context: &Context, project: &ProjectView) {
    let task = context.give(project.id, "zeus", "Write the parser").task;
    context
        .ledger
        .borrow_mut()
        .cancel_task(project.id, task.number, "chief")
        .unwrap();
}

/// Passes enough for a delivery to be handed over and seen in the record.
fn passes(context: &Context) {
    for _ in 0..3 {
        context.pass().unwrap();
    }
}

/// A wake-up goes, is answered, and the chief is at rest again.
fn woken(context: &Context, project: &ProjectView) {
    passes(context);
    context.adapter.answer("chief", "Nothing is left.");
    context.pass().unwrap();
    assert_eq!(
        context
            .dispatcher
            .activity(context.id(project.id, "chief"))
            .state,
        ActivityState::Idle
    );
}

#[test]
fn a_wake_up_goes_once_the_interval_has_passed_and_the_chief_is_at_rest_and_only_to_the_chief() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 5, WakeUpUnit::Minutes);
    context.advance(5 * MINUTE - 1);
    passes(&context);
    assert_eq!(given(&context), 0, "a millisecond short of the interval");
    context.advance(1);
    passes(&context);
    assert_eq!(given(&context), 1, "the interval has passed");
    let chief = context.id(project.id, "chief");
    let wake_up = context
        .inbox(chief)
        .into_iter()
        .find(|message| message.body == DEFAULT_WAKE_UP_TEXT)
        .expect("the wake-up");
    assert_eq!(
        (
            wake_up.kind.as_str(),
            wake_up.sender.as_deref(),
            wake_up.state.as_str()
        ),
        ("note", Some("human"), "delivered"),
        "a note from the human, through the usual queue"
    );
    let items = context.adapter.agent("chief").items;
    let item = items
        .iter()
        .find(|item| item.role == Role::User && item.text.contains(DEFAULT_WAKE_UP_TEXT))
        .unwrap();
    assert!(
        item.text.starts_with(&format!(
            "[ConsensFlow m-{} · note from @human]\n",
            wake_up.id
        )),
        "{}",
        item.text
    );
    assert!(
        context.inbox(context.id(project.id, "zeus")).is_empty(),
        "no member is sent one"
    );
}

#[test]
fn a_project_opened_with_the_calls_on_is_woken_when_their_interval_has_passed_with_no_save_after_it_opened(
) {
    let context = Context::new();
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [{ "agent": "zeus", "harness": "claude-code", "roles": ["worker"], "tier": "standard" }],
            "wakeUp": { "on": true, "every": 5, "unit": "minutes" },
        }))
        .unwrap()
        .expect("the project");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        context
            .dispatcher
            .activity(context.id(project.id, "chief"))
            .state,
        ActivityState::Idle
    );
    context.advance(5 * MINUTE - 1);
    passes(&context);
    assert_eq!(given(&context), 0, "a millisecond short of the interval");
    context.advance(1);
    passes(&context);
    assert_eq!(given(&context), 1, "the interval has passed");
}

#[test]
fn nothing_goes_while_the_project_is_suspended_and_the_count_restarts_at_the_resume() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 10, WakeUpUnit::Minutes);
    context.advance(9 * MINUTE);
    context.close_project(project.id).unwrap();
    context.advance(DAY);
    passes(&context);
    assert_eq!(given(&context), 0, "a suspended project is sent nothing");
    context.resume_project(project.id).unwrap();
    passes(&context);
    context.adapter.answer("chief", "Back.");
    passes(&context);
    context.advance(10 * MINUTE - 1);
    passes(&context);
    assert_eq!(
        given(&context),
        0,
        "ten minutes from the Resume, not from before it"
    );
    context.advance(1);
    passes(&context);
    assert_eq!(given(&context), 1);
}

#[test]
fn a_chief_in_a_turn_holds_the_wake_up_and_it_goes_once_the_turn_is_over_but_once() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    context.adapter.busy("chief");
    for _ in 0..5 {
        context.advance(HOUR);
        passes(&context);
    }
    assert_eq!(given(&context), 0, "nothing goes into a turn");
    assert_eq!(
        queued(&context, &project),
        0,
        "and none is queued to land stale after it"
    );
    context.adapter.answer("chief", "Done.");
    passes(&context);
    assert_eq!(given(&context), 1, "one, for all the intervals that passed");
}

#[test]
fn a_chief_waiting_on_a_question_holds_the_wake_up() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    context.adapter.with("chief", |agent| {
        agent.waiting = Some(Waiting {
            reason: Some("an answer from the human".to_owned()),
        });
    });
    context.advance(HOUR);
    passes(&context);
    assert_eq!(given(&context), 0);
    assert_eq!(queued(&context, &project), 0);
    context.adapter.with("chief", |agent| agent.waiting = None);
    passes(&context);
    assert_eq!(given(&context), 1, "it goes once the question is answered");
}

#[test]
fn a_chief_out_of_quota_holds_the_wake_up_until_it_is_back() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    let chief = context.id(project.id, "chief");
    let until = soon(&context, 5);
    context
        .ledger
        .borrow_mut()
        .mark_out(chief, &until, "out of quota")
        .unwrap();
    context.advance(HOUR);
    passes(&context);
    assert_eq!(context.dispatcher.activity(chief).state, ActivityState::Out);
    assert_eq!(given(&context), 0);
    assert_eq!(queued(&context, &project), 0);
    context.back_from_quota(project.id, "chief").unwrap();
    passes(&context);
    assert_eq!(given(&context), 1);
}

#[test]
fn text_the_human_has_not_sent_holds_the_wake_up_and_many_intervals_still_leave_one() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    let typed = Rc::new(Cell::new(true));
    let asked = Rc::clone(&typed);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        Ok(if asked.get() {
            Readiness::Held(Held::Unsent)
        } else {
            Readiness::Ready
        })
    }));
    let chief = context.id(project.id, "chief");
    for _ in 0..4 {
        context.advance(HOUR);
        passes(&context);
    }
    assert_eq!(given(&context), 0, "the human's words are in the window");
    assert_eq!(
        queued(&context, &project),
        1,
        "one waits, however many intervals passed"
    );
    assert!(
        context.dispatcher.holding(chief).unwrap(),
        "the board says why"
    );
    typed.set(false);
    passes(&context);
    assert_eq!(given(&context), 1, "it goes once the human's text is sent");
    assert_eq!(queued(&context, &project), 1);
    assert!(!context.dispatcher.holding(chief).unwrap());
}

#[test]
fn a_message_that_waits_for_the_humans_unsent_text_holds_the_wake_up_too_and_it_goes_after() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    let typed = Rc::new(Cell::new(true));
    let asked = Rc::clone(&typed);
    *context.adapter.ready.borrow_mut() = Some(Rc::new(move || {
        Ok(if asked.get() {
            Readiness::Held(Held::Unsent)
        } else {
            Readiness::Ready
        })
    }));
    // A word from the staff waits for the human's text: the chief will be told of
    // it, which is what a wake-up is for.
    context.note(project.id, "zeus", "chief", "The parser is done");
    passes(&context);
    let chief = context.id(project.id, "chief");
    assert!(context.dispatcher.holding(chief).unwrap());
    context.advance(HOUR);
    passes(&context);
    assert_eq!(
        queued(&context, &project),
        0,
        "no wake-up is queued behind it"
    );
    typed.set(false);
    passes(&context);
    context.adapter.answer("chief", "Noted.");
    passes(&context);
    assert_eq!(
        given(&context),
        1,
        "the wake-up goes once the chief is at rest and nothing waits"
    );
}

#[test]
fn a_wake_up_that_changed_nothing_waits_for_the_board_to_move_and_then_goes() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    context.advance(MINUTE);
    woken(&context, &project);
    for _ in 0..3 {
        context.advance(DAY);
        passes(&context);
    }
    assert_eq!(
        given(&context),
        1,
        "a quiet board: the same question is not asked again"
    );
    moved(&context, &project);
    passes(&context);
    context.advance(MINUTE);
    passes(&context);
    assert_eq!(
        given(&context),
        2,
        "something moved since the last: it goes"
    );
}

#[test]
fn the_human_typing_to_the_chief_is_news_too_and_a_note_the_staff_sends_is() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    context.advance(MINUTE);
    woken(&context, &project);
    context.advance(DAY);
    passes(&context);
    assert_eq!(given(&context), 1, "nothing moved");
    // The human types a line to the chief in its own window.
    let typed = context.adapter.item(Role::User, "Look at the docs next");
    context.adapter.with("chief", |agent| {
        agent.items.push(typed);
        agent.settled = false;
    });
    passes(&context);
    context.adapter.answer("chief", "On it.");
    passes(&context);
    context.advance(MINUTE);
    passes(&context);
    assert_eq!(given(&context), 2, "what the human typed is news");
    context.advance(DAY);
    passes(&context);
    context.adapter.answer("chief", "Nothing is left.");
    passes(&context);
    assert_eq!(given(&context), 2);
    context.note(project.id, "zeus", "chief", "The parser is done");
    passes(&context);
    context.adapter.answer("chief", "Noted.");
    passes(&context);
    context.advance(MINUTE);
    passes(&context);
    assert_eq!(given(&context), 3, "a message from the staff is news");
}

#[test]
fn the_first_wake_up_after_the_calls_are_turned_on_again_goes_on_a_quiet_board() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    context.advance(MINUTE);
    woken(&context, &project);
    context.advance(DAY);
    passes(&context);
    assert_eq!(given(&context), 1);
    set(&context, &project, false, 1, WakeUpUnit::Minutes);
    context.advance(DAY);
    passes(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    context.advance(MINUTE);
    passes(&context);
    assert_eq!(
        given(&context),
        2,
        "the first after an enable goes whether or not the board moved"
    );
}

#[test]
fn days_of_advance_send_one_wake_up_and_never_a_burst() {
    let context = Context::new();
    let project = at_rest(&context);
    set(&context, &project, true, 1, WakeUpUnit::Minutes);
    context.advance(3 * DAY);
    for _ in 0..6 {
        passes(&context);
        context.adapter.answer("chief", "Nothing is left.");
    }
    assert_eq!(given(&context), 1);
    assert_eq!(queued(&context, &project), 1);
}
