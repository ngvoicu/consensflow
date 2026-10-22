//! A failed turn that is no task's failure is told once (Calliope's review of
//! the failure notes, T-210): the chief's to the human, and a member's whose task
//! was not working to whoever gave the task. Once is by the message the window
//! was last given, read by id however many messages came since, and a message
//! still on its way is not given yet. An outage is told once however many turns
//! it fails: the same words again, with no turn of the window answered since, are
//! no news (the chief's decision of 2026-10-22, T-245). A refusal for quota is the
//! quota's to say. The words here are made up for the tests.

use crate::background::working;
use crate::fixtures::{after, exhausted, now, Tiers};
use cf_engine::testing::Context;
use cf_engine::ActivityState;
use cf_ledger::NewNote;

/// The chief's turn fails, its record saying `words` of why.
fn chief_fails(context: &Context, words: &str) {
    context.adapter.with("chief", |agent| {
        agent.failed = true;
        agent.failure = Some(words.to_owned());
    });
    context.adapter.answer("chief", "Looking at the board");
}

/// What the human was told of the chief's failed turns, oldest first.
fn chief_failures(tiers: &Tiers<'_>) -> Vec<String> {
    tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.starts_with("The chief's turn failed"))
        .collect()
}

/// A note of ConsensFlow's goes into `handle`'s window, which begins a turn on it.
fn given(context: &Context, tiers: &Tiers<'_>, handle: &str, body: &str) {
    context.note_from_consensflow(tiers.project.id, handle, body);
    context.pass().unwrap();
    context.pass().unwrap();
}

/// zeus's turn fails, its record saying why.
fn zeus_fails(context: &Context) {
    context.adapter.with("zeus", |agent| {
        agent.failed = true;
        agent.failure = Some("The model is busy, try another.".to_owned());
    });
    context.adapter.answer("zeus", "Waiting for the answer");
}

/// What zeus's requester was told of T-1's window's failed turns.
fn turn_failures(tiers: &Tiers<'_>) -> Vec<String> {
    tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("failed"))
        .collect()
}

#[test]
fn a_message_on_its_way_to_the_chief_does_not_make_its_failed_turn_news_again() {
    // The reason is the screen's: the message going in changes the screen, and
    // the failed turn is the same one until the window's record shows the message.
    let context = Context::new();
    let tiers = working(&context);
    context
        .host
        .set_snapshot(serde_json::json!({ "tail": ["Error: the agent stopped"] }));
    context.adapter.with("chief", |agent| agent.failed = true);
    context.adapter.answer("chief", "Looking at the board");
    context.pass().unwrap();
    assert_eq!(chief_failures(&tiers).len(), 1);
    // A message goes into the window, and its record does not show it yet (Devin's
    // wire log is read a while after); the screen shows it.
    context.adapter.with("chief", |agent| agent.arrive = false);
    context.note_from_consensflow(tiers.project.id, "chief", "Look at the board");
    context.host.set_snapshot(serde_json::json!({
        "tail": ["Error: the agent stopped", "> Look at the board"]
    }));
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(
        chief_failures(&tiers).len(),
        1,
        "{:?}",
        tiers.notes("human")
    );
}

#[test]
fn a_failure_told_is_found_however_many_notes_came_after_it() {
    // T-1 waits on the chief, its window's turn failed and was told: the note is
    // one of many to the chief by the next look, and the turn is not told again.
    let context = Context::new();
    let tiers = working(&context);
    let zeus = tiers.assignee(1);
    context.ask(tiers.project.id, &zeus, "chief", 1, "Which format?");
    zeus_fails(&context);
    context.pass().unwrap();
    for n in 0..60 {
        context.note_from_consensflow(tiers.project.id, "chief", &format!("Note {n}"));
    }
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(
        turn_failures(&tiers).len(),
        1,
        "{:?}",
        turn_failures(&tiers)
    );
}

#[test]
fn after_a_restart_a_failure_told_is_found_in_the_ledger() {
    let context = Context::new();
    let tiers = working(&context);
    chief_fails(&context, "API Error: 529 Overloaded");
    context.pass().unwrap();
    assert_eq!(chief_failures(&tiers).len(), 1);
    // The human's board fills with notes after it.
    for n in 0..60 {
        context.note_from_consensflow(tiers.project.id, "human", &format!("Note {n}"));
    }

    // The app quits and starts again; the chief's window comes back on its
    // conversation, whose turn failed as it did.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(tiers.project.id).unwrap();
    chief_fails(&context, "API Error: 529 Overloaded");
    for _ in 0..3 {
        after.pass().unwrap();
    }

    assert_eq!(
        chief_failures(&tiers).len(),
        1,
        "{:?}",
        tiers.notes("human")
    );
}

#[test]
fn a_chief_with_many_messages_waiting_has_its_new_failure_told() {
    let context = Context::new();
    let tiers = working(&context);
    chief_fails(&context, "API Error: 529 Overloaded");
    context.pass().unwrap();
    given(&context, &tiers, "chief", "Look at the board");
    chief_fails(&context, "API Error: 500 Internal server error");
    // Sixty messages wait for the chief behind its turn.
    for n in 0..60 {
        context.note_from_consensflow(tiers.project.id, "chief", &format!("Note {n}"));
    }
    context.pass().unwrap();

    let told = chief_failures(&tiers);
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(told[1].contains("500 Internal server error"), "{told:?}");
}

#[test]
fn an_outage_is_told_once_however_many_turns_of_the_chief_it_fails() {
    let context = Context::new();
    let tiers = working(&context);
    chief_fails(&context, "API Error: 529 Overloaded");
    context.pass().unwrap();
    // A wake-up's turn fails the same way, and another's.
    for wake in ["Wake up", "Wake up again"] {
        given(&context, &tiers, "chief", wake);
        chief_fails(&context, "API Error: 529 Overloaded");
        context.pass().unwrap();
        context.pass().unwrap();
    }

    assert_eq!(
        chief_failures(&tiers).len(),
        1,
        "{:?}",
        tiers.notes("human")
    );
}

#[test]
fn the_same_failure_after_an_answered_turn_is_told_again() {
    let context = Context::new();
    let tiers = working(&context);
    chief_fails(&context, "API Error: 529 Overloaded");
    context.pass().unwrap();
    given(&context, &tiers, "chief", "Wake up");
    context.adapter.answer("chief", "All quiet on the board");
    context.pass().unwrap();
    given(&context, &tiers, "chief", "Wake up again");
    chief_fails(&context, "API Error: 529 Overloaded");
    context.pass().unwrap();

    assert_eq!(
        chief_failures(&tiers).len(),
        2,
        "{:?}",
        tiers.notes("human")
    );
}

#[test]
fn a_failure_in_other_words_is_told_again() {
    let context = Context::new();
    let tiers = working(&context);
    chief_fails(&context, "API Error: 529 Overloaded");
    context.pass().unwrap();
    given(&context, &tiers, "chief", "Wake up");
    chief_fails(&context, "API Error: 500 Internal server error");
    context.pass().unwrap();

    assert_eq!(
        chief_failures(&tiers).len(),
        2,
        "{:?}",
        tiers.notes("human")
    );
}

#[test]
fn an_outage_of_a_members_window_is_told_its_requester_once() {
    let context = Context::new();
    let tiers = working(&context);
    let zeus = tiers.assignee(1);
    context.ask(tiers.project.id, &zeus, "chief", 1, "Which format?");
    zeus_fails(&context);
    context.pass().unwrap();
    // A note of T-1's reaches the window, whose next turn fails the same way.
    context
        .ledger
        .borrow_mut()
        .note(
            tiers.project.id,
            &NewNote {
                from: None,
                to: zeus.clone(),
                task: Some(1),
                body: "Mind the tests".to_owned(),
            },
        )
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    zeus_fails(&context);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(tiers.task(1).task.state, "waiting");
    assert_eq!(turn_failures(&tiers).len(), 1, "{:?}", tiers.notes("chief"));
}

#[test]
fn a_chiefs_turn_refused_for_quota_is_the_quotas_to_say_not_a_failure() {
    let context = Context::new();
    let tiers = working(&context);
    context.adapter.refuse(
        "chief",
        exhausted(Some(&now(&context)), Some(&after(&context, 3_600_000))),
    );
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        context.dispatcher.activity(tiers.id("chief")).state,
        ActivityState::Out
    );
    // Past the reset, the refusal is still the window's last word until its next turn.
    context.advance(3_600_000 + 60_000);
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(chief_failures(&tiers), Vec::<String>::new());
}
#[test]
fn an_outage_that_says_nothing_in_its_record_looks_at_the_screen_once_a_turn() {
    // The words are the screen's: a failed turn whose note is settled is not
    // looked at again, pass after pass, while it stays the window's last word.
    let context = Context::new();
    let tiers = working(&context);
    context
        .host
        .set_snapshot(serde_json::json!({ "tail": ["Error: the agent stopped"] }));
    let fails = || {
        context.adapter.with("chief", |agent| agent.failed = true);
        context.adapter.answer("chief", "Looking at the board");
    };
    fails();
    context.pass().unwrap();
    given(&context, &tiers, "chief", "Wake up");
    fails();
    let before = context.host.tails_asked();
    for _ in 0..5 {
        context.pass().unwrap();
    }

    assert_eq!(
        chief_failures(&tiers).len(),
        1,
        "{:?}",
        tiers.notes("human")
    );
    assert_eq!(
        context.host.tails_asked() - before,
        1,
        "the second turn is looked at once"
    );
}

#[test]
fn a_member_removed_while_its_window_is_looked_at_tells_nothing_of_its_failed_turn() {
    let context = Context::new();
    let tiers = working(&context);
    let zeus = tiers.assignee(1);
    context.ask(tiers.project.id, &zeus, "chief", 1, "Which format?");
    context
        .host
        .set_snapshot(serde_json::json!({ "tail": ["Error: the agent stopped"] }));
    context.adapter.with("zeus", |agent| agent.failed = true);
    context.adapter.answer("zeus", "Waiting for the answer");
    let answer = context
        .host
        .request_holds
        .hold(|args| args[0] == "pane.snapshot" && args[1].get("tail").is_some());
    let passing = context.begin_pass();
    context.settle();
    // The human takes zeus off the staff while its window is looked at.
    let removing = context.begin_remove_member(tiers.project.id, "zeus");
    context.settle();
    answer.open();
    context.settle();
    passing.take().unwrap().unwrap();
    context.finish(removing).unwrap();

    assert_eq!(turn_failures(&tiers), Vec::<String>::new());
}
