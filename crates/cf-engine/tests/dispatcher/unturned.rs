//! A conversation that never had a turn is not resumed. A harness keeps what it
//! can resume only once something was said in a conversation (Codex writes a
//! thread's record at its first turn), and found live on 2026-10-13: a Codex
//! chief opened and idle, closed with Close and brought back with Resume, was
//! given `codex resume <thread>`, which found no thread and ended the window at
//! once, and a chief's own exit closes its project. A window opened again on
//! such a conversation opens afresh, in a conversation of its own, as a first
//! launch does, with what waited for it; one that had a turn (something was
//! delivered into it since it began, or ConsensFlow's copy of it holds an item)
//! is resumed as it always was. None of these is held to a Node trace: Node
//! resumed every conversation it had.

use cf_engine::testing::Context;
use cf_engine::SwitchWhen;
use serde_json::Value;

use crate::chiefs::{handoffs_of, to, with_codex};
use crate::fixtures::{assert_match, given, last_launch, native_of};

/// The conversations the chief had before its current one, oldest first: each
/// with its native session, whether it ended, and how many items its copy holds.
fn earlier(context: &Context, project: i64) -> Vec<(Option<String>, bool, usize)> {
    context
        .ledger
        .borrow()
        .chief_history(project)
        .unwrap()
        .into_iter()
        .map(|earlier| {
            (
                earlier.conversation.native_session,
                earlier.conversation.ended_at.is_some(),
                earlier.items.len(),
            )
        })
        .collect()
}

#[test]
fn a_chief_that_never_had_a_turn_is_opened_afresh_by_the_resume_after_a_close_and_the_project_stays_open(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    // A brief is delivered to a member's window, and nothing to the chief's.
    context.pool_task(project.id, "worker", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let before = native_of(&context, chief).unwrap();

    context.close_project(project.id).unwrap();
    context.resume_project(project.id).unwrap();

    assert_eq!(
        last_launch(&context),
        ("chief".to_owned(), None),
        "a conversation with no turn has nothing to resume"
    );
    assert_ne!(context.dispatcher.pane(chief), None, "its window is open");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.project(project.id).state, "open");
    let now = native_of(&context, chief).unwrap();
    assert_ne!(now, before, "a conversation of its own");
    assert_eq!(
        earlier(&context, project.id),
        [(Some(before), true, 0)],
        "the one it left is over, and held nothing"
    );
}

#[test]
fn a_chief_whose_window_wrote_something_is_resumed_on_its_conversation() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    // The human typed into the chief's window and it answered: nothing was delivered
    // into it, and the copy of it holds what it wrote.
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    let before = native_of(&context, chief);

    context.close_project(project.id).unwrap();
    context.resume_project(project.id).unwrap();

    assert_eq!(last_launch(&context), ("chief".to_owned(), before.clone()));
    assert_eq!(native_of(&context, chief), before);
}

#[test]
fn a_chief_that_was_delivered_a_message_is_resumed_though_the_copy_of_its_window_never_held_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    let note = context.note(project.id, "zeus", "chief", "Taken");
    context.pass().unwrap();
    // The harness took it, and its own record has it; the daemon stopped before its
    // next look copied the window, so the copy does not, and a start confirms it.
    assert_eq!(context.message(note.id).state, "delivering");
    let copied = context
        .ledger
        .borrow()
        .copied_item_with(chief, &format!("m-{}", note.id))
        .unwrap();
    assert_eq!(copied, None);
    let before = native_of(&context, chief);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    assert_eq!(context.message(note.id).state, "delivered");

    after.resume_project(project.id).unwrap();

    assert_eq!(last_launch(&context), ("chief".to_owned(), before));
}

#[test]
fn a_message_that_was_not_delivered_makes_no_turn() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    let before = native_of(&context, chief);
    // The window took the paste, but its record never showed it before the stop:
    // the message goes back to its queue, and nothing was delivered.
    context.adapter.with("chief", |agent| agent.arrive = false);
    let lost = context.note(project.id, "zeus", "chief", "Lost");
    context.pass().unwrap();
    assert_eq!(context.message(lost.id).state, "delivering");
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    assert_eq!(context.message(lost.id).state, "queued");

    after.resume_project(project.id).unwrap();

    assert_eq!(last_launch(&context), ("chief".to_owned(), None));
    assert_ne!(native_of(&context, chief), before);
    // It is given to the window that opened afresh, once.
    for _ in 0..3 {
        after.pass().unwrap();
    }
    let delivered = context.message(lost.id);
    assert_eq!(
        (delivered.state.as_str(), delivered.attempts),
        ("delivered", 1)
    );
    let times = given(&context, "chief")
        .iter()
        .filter(|text| text.contains("Lost"))
        .count();
    assert_eq!(times, 1);
}

#[test]
fn what_waited_for_a_chief_that_never_had_a_turn_is_given_to_its_fresh_window_once() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    let waiting = context.note(project.id, "zeus", "chief", "A result");

    context.resume_project(project.id).unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(last_launch(&context), ("chief".to_owned(), None));
    let delivered = context.message(waiting.id);
    assert_eq!(
        (delivered.state.as_str(), delivered.attempts),
        ("delivered", 1)
    );
    let times = given(&context, "chief")
        .iter()
        .filter(|text| text.contains("A result"))
        .count();
    assert_eq!(times, 1, "given to the window that is open, and no other");
}

#[test]
fn a_handoff_the_new_chief_never_showed_is_given_again_in_a_conversation_of_its_own_and_once() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    // The window crashes, or the human closes the project, before the handoff shows.
    context.codex.with("chief", |agent| agent.items.clear());
    let handoff = handoffs_of(&context, project.id).remove(0);
    let first = native_of(&context, chief).unwrap();
    context.exit("chief");
    assert_eq!(context.project(project.id).state, "suspended");

    context.resume_project(project.id).unwrap();

    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_eq!(
        launch["resume"],
        Value::Null,
        "nothing of it was ever shown"
    );
    assert_match(
        launch["message"].as_str().unwrap(),
        r"^\[ConsensFlow m-\d+ · note from ConsensFlow\]\nYou are the chief now\.",
    );
    assert_ne!(native_of(&context, chief).unwrap(), first);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(handoff.id).state, "delivered");
    assert_eq!(
        handoffs_of(&context, project.id).len(),
        1,
        "the same handoff, and no other"
    );
    let times = context
        .codex
        .agent("chief")
        .items
        .iter()
        .filter(|item| item.text.contains("You are the chief now"))
        .count();
    assert_eq!(times, 1, "shown once");
}

#[test]
fn a_fresh_chief_with_earlier_conversations_behind_it_is_given_a_handoff_though_its_own_never_had_a_turn(
) {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    // The new chief's window never showed its handoff, and nothing waits for it now.
    context.codex.with("chief", |agent| agent.items.clear());
    let first = handoffs_of(&context, project.id).remove(0);
    context
        .ledger
        .borrow_mut()
        .cancel_message(first.id, "the human withdrew it")
        .unwrap();
    context.exit("chief");
    assert_eq!(context.project(project.id).state, "suspended");

    context.resume_project(project.id).unwrap();

    // The chief starts fresh with a conversation behind it that has things in it:
    // it is handed over, as any chief that starts fresh is.
    let handoffs = handoffs_of(&context, project.id);
    assert_eq!(
        handoffs
            .iter()
            .map(|m| m.state.as_str())
            .collect::<Vec<_>>(),
        ["delivering", "cancelled"]
    );
    let launch = context.codex.prepared().last().cloned().unwrap();
    assert_eq!(launch["resume"], Value::Null);
    assert_match(
        launch["message"].as_str().unwrap(),
        &format!(
            r"^\[ConsensFlow m-{} · note from ConsensFlow\]",
            handoffs[0].id
        ),
    );
}

#[test]
fn a_conversation_the_human_switched_the_window_to_that_never_had_a_turn_is_not_resumed_either() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    // The first conversation had a turn: a message was delivered into it.
    let one = context.note(project.id, "zeus", "chief", "One");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(one.id).state, "delivered");
    // Then the human types /new: the window shows another conversation, with nothing in it.
    context.advance(1_000);
    context.adapter.switch_to("chief", "native-new");
    context.pass().unwrap();
    assert_eq!(native_of(&context, chief).as_deref(), Some("native-new"));

    context.close_project(project.id).unwrap();
    context.resume_project(project.id).unwrap();

    assert_eq!(
        last_launch(&context),
        ("chief".to_owned(), None),
        "what was delivered into the one before it is no turn of this one"
    );
    assert_ne!(native_of(&context, chief).as_deref(), Some("native-new"));
}
