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

#[test]
fn a_refusal_of_the_words_that_resumed_the_task_holds_it_again_without_telling_the_requester_again_and_tells_the_human_once(
) {
    // A reset nobody named is an hour away, and a limit that does not reset by
    // itself (a plan without the model, a spent balance) is refused again at
    // each: the chief heard "waits" and "goes on" every hour (the review of
    // the quota wait, T-88).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), None));
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "paused");
    let waits = |notes: &[String]| notes.iter().filter(|n| n.starts_with("T-1 waits")).count();
    let goes_on = |notes: &[String]| {
        notes
            .iter()
            .filter(|n| n.starts_with("T-1 goes on"))
            .count()
    };
    let may_need = |notes: &[String]| notes.iter().filter(|n| n.contains("may need you")).count();
    for round in 0..3 {
        context.advance(61 * 60_000);
        for _ in 0..3 {
            context.pass().unwrap();
        }
        assert_eq!(tiers.task(1).task.state, "working", "round {round}");
        let inbox = context
            .ledger
            .borrow()
            .inbox(tiers.id(&session), 100)
            .unwrap();
        assert_match(&inbox[0].body, RESUMED);
        // Refused again, the moment it goes on.
        context.advance(1_000);
        context
            .adapter
            .refuse(&session, exhausted(Some(&now(&context)), None));
        for _ in 0..3 {
            context.pass().unwrap();
        }
        let held = tiers.task(1).task;
        assert_eq!(
            (held.state.as_str(), held.assignee.as_deref()),
            ("paused", Some(session.as_str())),
            "round {round}"
        );
        let chief = tiers.notes("chief");
        assert_eq!(
            (waits(&chief), goes_on(&chief)),
            (1, 1),
            "round {round}: {chief:?}"
        );
        let human = tiers.notes("human");
        assert_eq!(may_need(&human), 1, "round {round}: {human:?}");
    }
    let human = tiers.notes("human");
    assert_match(
        human.iter().find(|n| n.contains("may need you")).unwrap(),
        &format!(
            r"^T-1 was refused for quota again as soon as it went on: @{session}'s account may need you \(a plan that includes its model, a balance, or another account\)\. It waits, and ConsensFlow tries it again at each reset; say @zeus is back once its account has quota\.$"
        ),
    );
}

#[test]
fn holds_the_work_of_a_second_window_whose_refusal_is_dated_before_its_member_was_marked_out() {
    // The second refusal, dated before the daemon marked the member out, read
    // as history: its task stayed working while its member was out (T-88).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open_body("One");
    tiers.open_body("Two");
    context.pass().unwrap();
    context.pass().unwrap();
    let (one, two) = (tiers.assignee(1), tiers.assignee(2));
    // Both were refused five seconds ago; the second's record shows it a pass
    // after the first's took the member out.
    let resets_at = after(&context, 48 * 3_600_000);
    let refused_at = after(&context, -5_000);
    context
        .adapter
        .refuse(&one, exhausted(Some(&refused_at), Some(&resets_at)));
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    context
        .adapter
        .refuse(&two, exhausted(Some(&refused_at), Some(&resets_at)));
    context.pass().unwrap();
    context.pass().unwrap();
    for number in [1, 2] {
        let held = tiers.task(number).task;
        assert_eq!(
            (held.state.as_str(), held.held_until.as_deref()),
            ("paused", Some(resets_at.as_str())),
            "T-{number}"
        );
    }
    assert_eq!(tiers.task(1).task.assignee.as_deref(), Some(one.as_str()));
    assert_eq!(tiers.task(2).task.assignee.as_deref(), Some(two.as_str()));
}
