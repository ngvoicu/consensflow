//! The turns the engine waits at the pane host and the adapters. An adapter's
//! methods are `async` functions, and an `await` of one is a turn in
//! JavaScript, with the one more of a `.catch(...)` where a call has it,
//! whether or not the call waited: the engine takes those turns itself, and
//! the `.catch` of a call to the pane host (whose fake answers a turn after
//! the call, as the test's `async` functions did). A note the test writes
//! straight to the ledger a number of turns after it began something falls
//! among what the engine does where that many turns have passed, so a wait of
//! one turn too many or too few puts it on the other side of what follows, as
//! Node's puts it.

use std::rc::Rc;

use cf_engine::runtime::{next_turn, Spawn};
use cf_engine::testing::{Context, Made};
use cf_engine::{SwitchTo, SwitchWhen};
use cf_ledger::NewNote;
use serde_json::json;

use crate::fixtures::assert_match;
use crate::traces::held_to;

const SUITES: &[&str] = &["the turns the engine waits at the pane host and the adapters"];

/// A note from `zeus`, which the engine never writes as, to the human, written
/// `turns` turns from now (`tickAfter`): one more turn each time the work
/// waits once.
fn tick_after(context: &Context, project: i64, turns: usize) {
    let ledger = Rc::clone(&context.ledger);
    context.executor.spawn(Box::pin(async move {
        for _ in 1..turns {
            next_turn().await;
        }
        ledger
            .borrow_mut()
            .note(
                project,
                &NewNote {
                    from: Some("zeus".to_owned()),
                    to: "human".to_owned(),
                    task: None,
                    body: "Tick".to_owned(),
                },
            )
            .unwrap();
    }));
}

#[test]
fn a_refused_kill_is_traced_after_the_hosts_answer_its_catch_and_the_await_of_that() {
    for turns in [1, 2] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.pass().unwrap();
        context.host.refuse_kills.set(true);
        let closing = context.begin_close_project(project.id);
        tick_after(&context, project.id, turns);
        context.settle();
        let refused = closing.take().unwrap().unwrap_err();
        assert_match(&refused.to_string(), "would not close");
        held_to(
            context.close(),
            SUITES,
            &format!("a note written {turns} turns after a Close whose kill is refused"),
        );
    }
}

#[test]
fn the_old_window_of_a_switch_chief_is_looked_at_for_the_methods_await_and_the_catch_and_await_of_that(
) {
    for turns in [2, 3] {
        let context = Context::made(Made {
            codex: true,
            ..Made::default()
        });
        let project = context.with_staff(&["zeus"]);
        context.pass().unwrap();
        let to = SwitchTo {
            harness: "codex".to_owned(),
            agent: "astraeus".to_owned(),
        };
        let switching = context.begin_switch_chief(project.id, to, SwitchWhen::Now, false);
        tick_after(&context, project.id, turns);
        context.settle();
        switching.take().unwrap().unwrap();
        held_to(
            context.close(),
            SUITES,
            &format!(
                "a note written {turns} turns after a Switch chief whose old window is looked at"
            ),
        );
    }
}

#[test]
fn a_launch_that_cannot_take_its_first_message_learns_it_after_starteds_await_and_its_catch() {
    for turns in [5, 6] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        *context.adapter.started.borrow_mut() =
            Some(Rc::new(|| Err("the server never answered".to_owned())));
        context.give(project.id, "zeus", "Parser");
        let passing = context.begin_pass();
        tick_after(&context, project.id, turns);
        context.settle();
        passing.take().unwrap().unwrap();
        held_to(
            context.close(),
            SUITES,
            &format!("a note written {turns} turns after a pass whose launch cannot take its first message"),
        );
    }
}

#[test]
fn a_message_the_harness_took_is_found_in_its_record_after_the_records_await_and_catch_and_the_reading_methods_own(
) {
    for turns in [2, 3] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.pass().unwrap();
        context.note(project.id, "human", "chief", "Taken");
        context.pass().unwrap();
        context.ledger.borrow_mut().suspend_for_restart().unwrap();
        let after = context.make();
        let restarting = after.begin_resume_after_restart();
        tick_after(&context, project.id, turns);
        context.settle();
        restarting.take().unwrap().unwrap();
        held_to(
            context.close(),
            SUITES,
            &format!(
                "a note written {turns} turns after a restart that finds a message in its harness's record"
            ),
        );
    }
}

#[test]
fn a_delivery_the_harness_refuses_is_settled_after_the_await_of_the_adapters_deliver() {
    for turns in [3, 4] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.pass().unwrap();
        context.adapter.with("chief", |agent| agent.admit = false);
        context.note(project.id, "human", "chief", "hello");
        let passing = context.begin_pass();
        tick_after(&context, project.id, turns);
        context.settle();
        passing.take().unwrap().unwrap();
        held_to(
            context.close(),
            SUITES,
            &format!(
                "a note written {turns} turns after a pass whose delivery the harness refuses"
            ),
        );
    }
}

#[test]
fn enter_is_pressed_again_after_the_snapshot_and_the_key_each_a_request_whose_catch_is_a_turn() {
    for turns in [4, 6, 7] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        context.adapter.with("chief", |agent| agent.arrive = false);
        context.host.set_snapshot(json!({ "outputQuietMs": 5000 }));
        context.note(project.id, "human", "chief", "hello");
        context.pass().unwrap();
        context.advance(11_000);
        let passing = context.begin_pass();
        tick_after(&context, project.id, turns);
        context.settle();
        passing.take().unwrap().unwrap();
        held_to(
            context.close(),
            SUITES,
            &format!("a note written {turns} turns after a pass that presses Enter again"),
        );
    }
}
