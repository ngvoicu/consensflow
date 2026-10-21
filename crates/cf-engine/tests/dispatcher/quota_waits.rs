//! A quota wait never fails, and never gives the work away (dip-agentos,
//! 2026-10-17 to 19, ConsensFlow 3.0.4: T-17 and T-20 were told they wait out
//! of quota and go on by themselves, then ended failed, their work left in
//! their windows). While a member is out, its task stays in its window with
//! its work, whoever else of its tier is free: only the human or the chief
//! moves it. At the reset, or when the human says the member is back, the same
//! window is given the words that resume it. Held to no Node trace: Node's
//! dispatcher gave a task whose reset was far to another member of its tier.

use cf_engine::testing::Context;
use cf_harness::records::Role;

use crate::fixtures::{after, assert_match, exhausted, now, placed, Tiers};

/// The words that resume a task its member's quota held.
const RESUMED: &str = r"^Resumed: Go on where you stopped\.$";

/// The native session of a participant's current conversation.
fn current_session(context: &Context, participant: i64) -> Option<String> {
    context
        .ledger
        .borrow()
        .current_conversation(participant)
        .unwrap()
        .unwrap()
        .native_session
}

#[test]
fn keeps_the_task_with_its_window_though_another_of_its_tier_is_free_and_the_reset_hours_away_and_goes_on_there_when_the_human_says_the_member_is_back(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let zeus = tiers.id(&session);
    let native = current_session(&context, zeus);
    let resets_at = after(&context, 3 * 3_600_000);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    context.pass().unwrap();
    let held = tiers.task(1).task;
    assert_eq!(
        (
            held.state.as_str(),
            held.assignee.as_deref(),
            held.held_until.as_deref()
        ),
        ("paused", Some(session.as_str()), Some(resets_at.as_str())),
        "diana is free, but the work is in zeus's window"
    );
    assert!(
        !held.body.contains("Reassigned"),
        "never given to another member: {}",
        held.body
    );
    assert_eq!(
        tiers.notes("chief"),
        [format!(
            "T-1 waits with @{session}: out of quota until {resets_at}, or sooner if its account has quota again; it goes on by itself."
        )]
    );
    assert!(context.host.killed().is_empty(), "its window waits with it");

    context.back_from_quota(tiers.project.id, "zeus").unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some(session.as_str()))
    );
    let inbox = context.ledger.borrow().inbox(zeus, 100).unwrap();
    assert_match(&inbox[0].body, RESUMED);
    assert_eq!(current_session(&context, zeus), native, "the same window");
    context.adapter.answer(&session, "Parser done");
    context.pass().unwrap();
    let notes = tiers.notes("chief");
    assert_eq!(
        notes
            .iter()
            .filter(|note| note.starts_with("T-1 goes on"))
            .count(),
        1,
        "{notes:?}"
    );
    assert!(
        notes.iter().all(|note| !note.contains("failed")),
        "{notes:?}"
    );
    assert_ne!(tiers.task(1).task.state, "failed");
}

#[test]
fn a_first_message_its_harness_took_before_refusing_is_not_given_again_at_the_reset() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let zeus = tiers.id(&session);
    let brief = context.ledger.borrow().inbox(zeus, 100).unwrap()[0].clone();
    assert_eq!(brief.state, "delivering");
    // The look that first shows the brief in the record shows the refusal after it.
    let resets_at = after(&context, 20 * 60_000);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    let state_of = |id: i64| context.ledger.borrow().message(id).unwrap().unwrap().state;
    assert_eq!(
        (tiers.task(1).task.state, state_of(brief.id)),
        ("paused".to_owned(), "delivered".to_owned()),
        "the harness had it: it arrived"
    );
    context.advance(21 * 60_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some(session.as_str()))
    );
    let pasted = context
        .adapter
        .agent(&session)
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains("Write the parser"))
        .count();
    assert_eq!(pasted, 1, "the brief went into the window once");
}

#[test]
fn a_message_its_harness_refused_before_taking_it_goes_again_without_counting_an_attempt() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    let zeus = tiers.id(&session);
    let brief = context.ledger.borrow().inbox(zeus, 100).unwrap()[0].clone();
    assert_eq!((brief.state.as_str(), brief.attempts), ("delivering", 1));
    // The record never shows the brief: the harness refused before it took it.
    context.adapter.with(&session, |agent| {
        agent.items.retain(|item| item.role != Role::User);
    });
    context.adapter.refuse(
        &session,
        exhausted(Some(&now(&context)), Some(&after(&context, 20 * 60_000))),
    );
    context.pass().unwrap();
    let again = context.ledger.borrow().message(brief.id).unwrap().unwrap();
    assert_eq!(
        (again.state.as_str(), again.attempts),
        ("queued", 0),
        "quota is no failure of the window's: its attempt is given back"
    );
}
