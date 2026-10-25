//! Whether a window is opened again on its conversation or afresh is what the
//! window's harness says of the conversation, from its own record
//! ([`Adapter::resumable`]): only its no opens a window afresh, and where it
//! cannot tell the window is resumed, as it was before the harness was asked.
//! Found live on 2026-10-13: a Codex chief opened and idle, closed with Close and
//! brought back with Resume, was given `codex resume <thread>`, which found no
//! thread (Codex keeps one once it has had a turn) and ended the window at once,
//! and a chief's own exit closes its project. And the other way round: a turn the
//! human typed into a window is in its harness's record at once, and in
//! ConsensFlow's copy of the conversation at the next look, a second on, and a
//! conversation that holds things is not left behind for a record that says
//! nothing: the copy plays no part in the decision. None of these is held to a
//! Node trace: Node resumed every conversation it had.
//!
//! The fake's harness holds every conversation unless a test says it holds
//! nothing of one (`holds_nothing`) or cannot tell (`cannot_tell_a_resume`).
//!
//! [`Adapter::resumable`]: cf_harness::contract::Adapter::resumable

use cf_engine::testing::{Context, Made};
use cf_engine::SwitchWhen;
use cf_harness::records::Role;
use serde_json::{json, Value};

use crate::chiefs::{handoffs_of, to, with_codex};
use crate::fixtures::{assert_match, given, last_launch, launches_of, native_of};

/// A project whose chief is on `harness`, its window up, on an engine whose fake
/// answers for that harness.
fn chief_on(harness: &'static str) -> (Context, i64) {
    let context = Context::made(Made {
        harnesses: vec![harness],
        codex: false,
    });
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": harness, "agent": "apollo" },
            "staff": [],
        }))
        .unwrap()
        .unwrap();
    (context, project.id)
}

/// What the chief's window saves of a turn the human typed into it: the message
/// and the answer, in its harness's record and in no copy of it yet.
fn saves_a_turn(context: &Context) {
    let typed = context.adapter.item(Role::User, "Hello");
    let answer = context.adapter.item(Role::Assistant, "Hi");
    context.adapter.with("chief", |agent| {
        agent.items.extend([typed, answer]);
    });
}

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
fn a_chief_whose_harness_holds_its_conversation_is_resumed_on_it_though_no_look_has_copied_a_turn_of_it(
) {
    for harness in ["claude-code", "codex", "pi"] {
        let (context, project) = chief_on(harness);
        let chief = context.id(project, "chief");
        // No look came between: the copy holds nothing of the turn.
        saves_a_turn(&context);
        let before = native_of(&context, chief);

        // The human closes the project, or the app quits, right after the turn.
        context.close_project(project).unwrap();
        context.resume_project(project).unwrap();

        assert_eq!(
            last_launch(&context),
            ("chief".to_owned(), before.clone()),
            "{harness}"
        );
        assert_eq!(native_of(&context, chief), before, "{harness}");
        assert_eq!(context.project(project).state, "open", "{harness}");
    }
}

#[test]
fn a_conversation_the_human_switched_the_window_to_is_resumed_on_what_its_harness_holds_before_the_copy_follows(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    // The human types /resume in the window and picks an older conversation: the
    // harness holds it, and the window shows it; the copy of it is as new as the
    // conversation is to the ledger.
    context.adapter.switch_to("chief", "native-older");
    context.pass().unwrap();
    assert_eq!(native_of(&context, chief).as_deref(), Some("native-older"));

    context.close_project(project.id).unwrap();
    context.resume_project(project.id).unwrap();

    assert_eq!(
        last_launch(&context),
        ("chief".to_owned(), Some("native-older".to_owned()))
    );
    assert_eq!(native_of(&context, chief).as_deref(), Some("native-older"));
}

#[test]
fn a_chief_whose_harness_makes_its_sessions_before_any_turn_is_resumed_though_nothing_was_said_in_it(
) {
    let (context, project) = chief_on("opencode");
    let chief = context.id(project, "chief");
    let before = native_of(&context, chief);

    context.close_project(project).unwrap();
    context.resume_project(project).unwrap();

    assert_eq!(last_launch(&context), ("chief".to_owned(), before));
}

#[test]
fn a_chief_whose_harness_holds_nothing_of_its_conversation_is_opened_afresh_though_the_copy_of_it_holds_a_turn(
) {
    // A Codex chief: what Codex kept of its thread is gone (its rollout was cleaned away).
    let (context, project) = chief_on("codex");
    let chief = context.id(project, "chief");
    context.pass().unwrap();
    saves_a_turn(&context);
    context.pass().unwrap();
    context.adapter.holds_nothing("chief");
    let before = native_of(&context, chief).unwrap();

    context.close_project(project).unwrap();
    context.resume_project(project).unwrap();

    assert_eq!(
        last_launch(&context),
        ("chief".to_owned(), None),
        "a resume would find nothing, and the window would end"
    );
    assert_ne!(native_of(&context, chief).unwrap(), before);
    assert_eq!(
        earlier(&context, project),
        [(Some(before), true, 2)],
        "the one it left is over, with what it held"
    );
    assert_eq!(context.project(project).state, "open");
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
    // The harness's record of the conversation is not there until it has a turn (Codex's,
    // Claude Code's).
    context.adapter.holds_nothing("chief");
    let before = native_of(&context, chief).unwrap();

    context.close_project(project.id).unwrap();
    context.resume_project(project.id).unwrap();

    // The chief's launch, which the member's, whose task the Close paused, follows.
    assert_eq!(
        launches_of(&context, "chief").pop().unwrap()["resume"],
        Value::Null,
        "a conversation its harness holds nothing of has nothing to resume"
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
fn where_the_record_cannot_tell_a_conversation_is_resumed_whatever_its_copy_holds() {
    // Only the harness's no opens a window afresh: a conversation whose copy
    // holds nothing, and one whose copy holds a turn, are gone on with alike.
    for turn in [false, true] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        let chief = context.id(project.id, "chief");
        context.pass().unwrap();
        if turn {
            saves_a_turn(&context);
            context.pass().unwrap();
        }
        context.adapter.cannot_tell_a_resume("chief");
        let before = native_of(&context, chief);

        context.close_project(project.id).unwrap();
        context.resume_project(project.id).unwrap();

        assert_eq!(
            last_launch(&context),
            ("chief".to_owned(), before.clone()),
            "its copy holds a turn: {turn}"
        );
        assert_eq!(native_of(&context, chief), before, "{turn}");
        assert_eq!(context.project(project.id).state, "open", "{turn}");
    }
}

#[test]
fn a_member_whose_harness_holds_nothing_of_its_conversation_is_opened_afresh_and_given_its_brief() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let zeus = context.id(project.id, "zeus");
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let before = native_of(&context, zeus).unwrap();
    // The quit cut zeus's turn, and its harness kept nothing of the conversation.
    context.adapter.holds_nothing("zeus");
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();

    after.resume_project(project.id).unwrap();

    let launch = launches_of(&context, "zeus").pop().unwrap();
    assert_eq!(launch["resume"], Value::Null, "nothing for it to resume");
    assert_match(
        launch["message"].as_str().unwrap(),
        r"(?s)Parser.*ConsensFlow stopped while you were working on T-1",
    );
    assert_ne!(native_of(&context, zeus).unwrap(), before);
}

#[test]
fn what_waited_for_a_chief_whose_harness_holds_nothing_is_given_to_its_fresh_window_once() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.holds_nothing("chief");
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
fn a_message_the_window_never_took_is_given_to_the_chiefs_fresh_window_once() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    let before = native_of(&context, chief);
    // The window took the paste, but its record never showed it before the stop:
    // the message goes back to its queue, and the harness kept nothing of the thread.
    context.adapter.with("chief", |agent| agent.arrive = false);
    let lost = context.note(project.id, "zeus", "chief", "Lost");
    context.pass().unwrap();
    assert_eq!(context.message(lost.id).state, "delivering");
    context.adapter.holds_nothing("chief");
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
    // The window crashes, or the human closes the project, before the handoff shows:
    // the thread had no turn, and Codex kept nothing of it.
    context.codex.with("chief", |agent| agent.items.clear());
    context.codex.holds_nothing("chief");
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
fn a_fresh_chief_with_earlier_conversations_behind_it_is_given_a_handoff_though_its_own_held_nothing(
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
    context.codex.holds_nothing("chief");
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
