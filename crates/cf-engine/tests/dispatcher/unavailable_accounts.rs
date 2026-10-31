//! A member whose account refused for what no wait cures (its billing, its login: a 402,
//! a 401, MUS-23) is unavailable, not out until a reset: its task is held with its window
//! and goes on at no time of its own; the requester and the human are told once; and only
//! the human's Try again now brings it back.

use cf_engine::testing::Context;
use std::sync::Arc;

use cf_harness::records::{Level, Quota};

use crate::fixtures::{after, assert_match, now, placed, Tiers};

const BECAUSE: &str = "Muse refused: the account has no allowance; set up billing at accountscenter.meta.com/muse_code";

fn unavailable(context: &Context) -> Quota {
    Quota::Unavailable {
        at: Some(now(context)),
        because: BECAUSE.to_owned(),
    }
}

/// The notes the human was sent, oldest first.
fn human_notes(tiers: &Tiers) -> Vec<String> {
    crate::fixtures::notes(tiers.context, tiers.context.id(tiers.project.id, "human"))
}

#[test]
fn holds_the_task_with_its_window_for_no_reset_and_tells_the_requester_and_the_human_once() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some("zeus-amber-pine"))
    );
    context
        .adapter
        .refuse("zeus-amber-pine", unavailable(&context));
    context.pass().unwrap();
    let held = tiers.task(1).task;
    assert_eq!(
        (held.state.as_str(), held.assignee.as_deref()),
        ("paused", Some("zeus-amber-pine")),
        "held with its window: diana is free, but only the human or the chief gives it to another"
    );
    assert!(
        held.held_until
            .as_deref()
            .is_some_and(|until| until.starts_with("9999-")),
        "no reset: it is due at no time of its own, {:?}",
        held.held_until
    );
    assert!(tiers
        .out_until("zeus")
        .is_some_and(|until| until.starts_with("9999-")));
    assert_match(
        tiers.notes("chief").last().unwrap(),
        r"^T-1 waits with @zeus-amber-pine: its account is unavailable \(Muse refused: the account has no allowance; set up billing at accountscenter\.meta\.com/muse_code\), and nothing resets that; it goes on when the human says the account works again\.$",
    );
    let told = human_notes(&tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_match(
        &told[0],
        r"^@zeus-amber-pine's account is unavailable: Muse refused: the account has no allowance; .* Nothing is sent to it until you say it works again: press Try again now on @zeus once it does\.$",
    );
    // Hours later, passes and a window that still shows the refusal change nothing, and
    // nobody is told again.
    for _ in 0..3 {
        context.advance(2 * 3_600_000);
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(human_notes(&tiers).len(), 1);
    assert_eq!(
        tiers
            .notes("chief")
            .iter()
            .filter(|note| note.starts_with("T-1 waits"))
            .count(),
        1
    );
    let inbox = context
        .ledger
        .borrow()
        .inbox(tiers.id("zeus-amber-pine"), 100)
        .unwrap();
    assert!(
        inbox
            .iter()
            .all(|message| !message.body.starts_with("Resumed:")),
        "no retry but at the human's word"
    );
    // Another task of the tier goes to the member that is available.
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let second = tiers.task(2).task;
    assert_eq!(second.state, "working");
    assert!(
        second
            .assignee
            .as_deref()
            .is_some_and(|who| who.starts_with("diana")),
        "{:?}",
        second.assignee
    );
}

#[test]
fn a_second_window_of_the_member_refused_a_minute_later_holds_its_task_and_the_human_is_not_told_again(
) {
    // Two windows of one member run into the same refused account: each task is held and its
    // requester told, and the human, who has one account to fix, hears it once.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open_body("One");
    tiers.open_body("Two");
    context.pass().unwrap();
    context.pass().unwrap();
    let (one, two) = (tiers.assignee(1), tiers.assignee(2));
    context.adapter.refuse(&one, unavailable(&context));
    context.pass().unwrap();
    context.advance(60_000);
    context.adapter.refuse(&two, unavailable(&context));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["paused", "paused"]
    );
    let waits: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains(" waits with @"))
        .collect();
    assert_eq!(waits.len(), 2, "{waits:?}");
    assert_eq!(human_notes(&tiers).len(), 1, "{:?}", human_notes(&tiers));
}

#[test]
fn the_human_saying_the_account_works_again_brings_the_member_back_and_its_task_goes_on() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .adapter
        .refuse("zeus-amber-pine", unavailable(&context));
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    context.advance(30 * 60_000);
    let back = context.back_from_quota(tiers.project.id, "zeus").unwrap();
    assert_eq!(back.out_until, None);
    // Its window reads at rest again: the account works.
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.quota = None;
        agent.failed = false;
    });
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some("zeus-amber-pine"))
    );
    // A refusal that came after its member was marked back is news again, told again.
    context.adapter.refuse(
        "zeus-amber-pine",
        Quota::Unavailable {
            at: Some(after(&context, 60_000)),
            because: BECAUSE.to_owned(),
        },
    );
    context.advance(2 * 60_000);
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    assert_eq!(human_notes(&tiers).len(), 2);
}

#[test]
fn an_allowance_the_harness_shows_again_brings_an_unavailable_member_back_a_still_full_one_does_not(
) {
    // Muse's `usage` says the account has an allowance (MUS-23): that is the harness's own word
    // that the refusal is over, and no human's is needed. A count that says used up is not.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .adapter
        .refuse("zeus-amber-pine", unavailable(&context));
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    let usage = |level, used: u64| Quota::Usage {
        level,
        used_percent: Some(used.into()),
        resets_at: None,
    };
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.failed = false;
        agent.quota = Some(Arc::new(usage(Level::Exhausted, 100)));
    });
    context.pass().unwrap();
    assert!(
        tiers.out_until("zeus").is_some(),
        "a count used up is no allowance"
    );
    context.adapter.with("zeus-amber-pine", |agent| {
        agent.quota = Some(Arc::new(usage(Level::Ok, 3)));
    });
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), None, "its allowance is back");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some("zeus-amber-pine")),
        "its task went on"
    );
}
