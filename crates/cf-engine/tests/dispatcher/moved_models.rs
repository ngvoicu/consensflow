//! A member whose harness moved its window's model on its own (MUS-24, T-816): held, as an
//! account that refused for its billing is, and released by the human's word for that member
//! alone, never by a teammate's turn, an allowance or another window's answer, or the
//! engine would paste into a window on a model nobody chose. A model that only drifts in the
//! harness's records is told once and never held on. A move dated before the window opened is
//! history.

use cf_engine::testing::Context;
use cf_harness::records::{Level, ModelMoved, Quota};
use std::sync::Arc;

use crate::fixtures::{after, given, now, placed, Tiers};

/// What Claude Code's own `fallback` record says, at `at`.
fn fell_back(at: &str) -> ModelMoved {
    ModelMoved {
        from: "claude-fable-5-1".to_owned(),
        to: "claude-opus-4-8".to_owned(),
        because: "Claude Code fell back to another model (its own `fallback` record)".to_owned(),
        at: Some(at.to_owned()),
    }
}

/// The notes the human was sent, oldest first.
fn human_notes(tiers: &Tiers) -> Vec<String> {
    crate::fixtures::notes(tiers.context, tiers.context.id(tiers.project.id, "human"))
}

/// zeus and diana, each at work on a task, on a harness whose windows are on one account;
/// zeus's window then reads a move of the model.
fn zeus_moved(context: &Context) -> (Tiers<'_>, String, String) {
    context.adapter.one_account.set(true);
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    tiers.open_body("One");
    tiers.open_body("Two");
    context.pass().unwrap();
    context.pass().unwrap();
    let (zeus, diana) = (tiers.assignee(1), tiers.assignee(2));
    assert!(
        zeus.starts_with("zeus") && diana.starts_with("diana"),
        "{zeus} {diana}"
    );
    context.advance(60_000);
    context
        .adapter
        .refuse(&zeus, fell_back(&now(context)).as_quota());
    context.pass().unwrap();
    context.pass().unwrap();
    (tiers, zeus, diana)
}

#[test]
fn a_teammates_turn_and_the_humans_word_for_another_member_release_no_member_held_for_a_move() {
    let context = Context::new();
    let (tiers, zeus, diana) = zeus_moved(&context);
    let held = |tiers: &Tiers| tiers.out_until("zeus").is_some();
    assert!(held(&tiers), "zeus is held for the move");
    assert_eq!(tiers.task(1).task.state, "paused");
    // diana finishes a turn on the same account: no word for zeus.
    context.advance(60_000);
    context.adapter.writes(&diana, "Going on.", &now(&context));
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(held(&tiers), "a teammate's turn is no word for zeus");
    assert_eq!(tiers.task(1).task.state, "paused");
    assert!(
        given(&context, &zeus)
            .iter()
            .all(|said| !said.contains("Resumed:")),
        "nothing was pasted into the moved window: {:?}",
        given(&context, &zeus)
    );
    // The human's Try again now on diana is for diana alone.
    context.back_from_quota(tiers.project.id, "diana").unwrap();
    context.pass().unwrap();
    assert!(held(&tiers), "Try again now on diana is not on zeus");
    assert_eq!(tiers.task(1).task.state, "paused");
    // The sentinel names no time and says why it is held.
    assert!(tiers
        .out_until("zeus")
        .is_some_and(|until| until.starts_with("9999-")));
}

#[test]
fn the_humans_word_for_that_member_brings_it_back_and_its_task_goes_on() {
    let context = Context::new();
    let (tiers, zeus, _) = zeus_moved(&context);
    context.back_from_quota(tiers.project.id, "zeus").unwrap();
    // Its window reads at rest again: the human put the model back, or accepted it.
    context.adapter.with(&zeus, |agent| {
        agent.quota = None;
        agent.failed = false;
    });
    context.pass().unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), None);
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some(zeus.as_str()))
    );
    assert!(
        given(&context, &zeus)
            .iter()
            .any(|said| said.contains("Resumed:")),
        "{:?}",
        given(&context, &zeus)
    );
}

#[test]
fn a_human_who_accepts_the_model_and_presses_try_again_goes_on_though_the_record_still_reads_the_move(
) {
    // In production the harness's record goes on reading the move until the model is put back or
    // the window is relaunched: the hold must not come back at the next look, the words go in once,
    // and the requester, told it waits, is told it goes on (T-816, Calliope's question).
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let zeus = tiers.assignee(1);
    context.advance(60_000);
    context
        .adapter
        .quota(&zeus, Some(fell_back(&now(&context)).as_quota()));
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(tiers.out_until("zeus").is_some());
    assert_eq!(tiers.task(1).task.state, "paused");
    // Its turn stopped when the task was held: the window rests, still reading the move.
    context.adapter.with(&zeus, |agent| agent.settled = true);
    context.back_from_quota(tiers.project.id, "zeus").unwrap();
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(
        tiers.out_until("zeus"),
        None,
        "the same record does not hold it again"
    );
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some(zeus.as_str()))
    );
    let resumed = || {
        given(&context, &zeus)
            .iter()
            .filter(|said| said.contains("Resumed:"))
            .count()
    };
    assert_eq!(resumed(), 1);
    // Its window answers, past the time a refusal is the same wait: the requester is told.
    context.advance(6 * 60_000);
    context.adapter.writes(&zeus, "Going on.", &now(&context));
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let goes_on: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.starts_with("T-1 goes on"))
        .collect();
    assert_eq!(
        goes_on,
        ["T-1 goes on: the human said its model may go on."],
        "{:?}",
        tiers.notes("chief")
    );
    assert_eq!(tiers.out_until("zeus"), None);
    assert_eq!(resumed(), 1, "once");
}

#[test]
fn another_window_of_the_member_that_answers_and_an_allowance_release_no_hold_for_a_move() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open_body("One");
    tiers.open_body("Two");
    context.pass().unwrap();
    context.pass().unwrap();
    let (one, two) = (tiers.assignee(1), tiers.assignee(2));
    context.advance(60_000);
    context
        .adapter
        .refuse(&one, fell_back(&now(&context)).as_quota());
    context.pass().unwrap();
    assert!(tiers.out_until("zeus").is_some());
    // The member's other window gets a turn through a minute later.
    context.advance(60_000);
    context.adapter.writes(&two, "Going on.", &now(&context));
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        tiers.out_until("zeus").is_some(),
        "another window's answer is no word for the move"
    );
    assert_eq!(tiers.task(1).task.state, "paused");
    // And a count that is not used up, which brings an unavailable account back, does not.
    context.adapter.with(&one, |agent| {
        agent.failed = false;
        agent.quota = Some(Arc::new(Quota::Usage {
            level: Level::Ok,
            used_percent: Some(3.into()),
            resets_at: None,
        }));
    });
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        tiers.out_until("zeus").is_some(),
        "an allowance is no word for the move"
    );
    assert_eq!(tiers.task(1).task.state, "paused");
}

#[test]
fn the_requester_and_the_human_are_told_why_once_in_words_of_the_move_not_of_a_billing() {
    let context = Context::new();
    let (tiers, zeus, _) = zeus_moved(&context);
    let waits: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains(" waits with @"))
        .collect();
    assert_eq!(waits.len(), 1, "{waits:?}");
    assert!(
        waits[0].starts_with(&format!("T-1 waits with @{zeus}: its model moved from claude-fable-5-1 to claude-opus-4-8 on its own")),
        "{}",
        waits[0]
    );
    assert!(!waits[0].contains("account"), "{}", waits[0]);
    let told = human_notes(&tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("a model nobody chose"), "{}", told[0]);
    assert!(told[0].contains("Try again now"), "{}", told[0]);
    assert!(!told[0].contains("account is unavailable"), "{}", told[0]);
    for _ in 0..3 {
        context.advance(3_600_000);
        context.pass().unwrap();
    }
    assert_eq!(human_notes(&tiers).len(), 1);
}

#[test]
fn a_move_dated_before_the_window_opened_holds_nothing_and_one_after_it_does() {
    // A Resume of a project whose transcript holds an old change: the window opens now, and what
    // its harness's record says was written before is history.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let zeus = tiers.assignee(1);
    let old = after(&context, -3_600_000);
    context
        .adapter
        .quota(&zeus, Some(fell_back(&old).as_quota()));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), None, "an old move holds nothing");
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some(zeus.as_str()))
    );
    // One written after the window opened holds.
    context.advance(60_000);
    context
        .adapter
        .quota(&zeus, Some(fell_back(&now(&context)).as_quota()));
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(tiers.out_until("zeus").is_some());
    assert_eq!(tiers.task(1).task.state, "paused");
}

#[test]
fn a_model_that_only_drifts_is_told_to_the_human_once_and_holds_nothing() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let zeus = tiers.assignee(1);
    context.advance(60_000);
    let at = now(&context);
    context
        .adapter
        .drifts(&zeus, "claude-opus-5", "claude-opus-5-5", Some(&at));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.out_until("zeus"), None, "a drift holds no member");
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some(zeus.as_str()))
    );
    let told = human_notes(&tiers);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains("claude-opus-5") && told[0].contains("claude-opus-5-5"),
        "{}",
        told[0]
    );
    assert!(told[0].contains("Nothing is held"), "{}", told[0]);
    // The same drift read at every look is told once; another is told again.
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(human_notes(&tiers).len(), 1);
    context.advance(60_000);
    context.adapter.drifts(
        &zeus,
        "claude-opus-5-5",
        "claude-opus-5",
        Some(&now(&context)),
    );
    context.pass().unwrap();
    assert_eq!(human_notes(&tiers).len(), 2);
}

#[test]
fn a_drift_dated_before_the_window_opened_is_not_told() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let zeus = tiers.assignee(1);
    let old = after(&context, -3_600_000);
    context
        .adapter
        .drifts(&zeus, "claude-opus-5", "claude-opus-5-5", Some(&old));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(human_notes(&tiers), Vec::<String>::new());
}

#[test]
fn a_task_that_waits_for_a_free_member_is_told_the_model_moved_not_a_billing_refused() {
    // The reason a held member is not free is told in the words of what held it, also after a
    // restart of the daemon, which keeps the hold in the ledger and not the kind of refusal.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open_body("One");
    context.pass().unwrap();
    context.pass().unwrap();
    let zeus = tiers.assignee(1);
    context.advance(60_000);
    context
        .adapter
        .refuse(&zeus, fell_back(&now(&context)).as_quota());
    context.pass().unwrap();
    tiers.open_body("Two");
    context.pass().unwrap();
    context.pass().unwrap();
    let waits: Vec<String> = tiers
        .notes("chief")
        .into_iter()
        .filter(|note| note.starts_with("T-2 waits for a free"))
        .collect();
    assert_eq!(waits.len(), 1, "{:?}", tiers.notes("chief"));
    assert!(
        waits[0].contains("@zeus's model moved on its own (it waits for the human's word)"),
        "{}",
        waits[0]
    );
    assert!(!waits[0].contains("billing"), "{}", waits[0]);
}

/// zeus at work on T-1, its window refused with `held`, dated a minute after it began, and
/// the task held; then its window goes (a Close, an exit; a daemon restart ends every window
/// the same way), the human presses Try again now, and the window comes back on its
/// conversation: its record reads what the one before it ended in (T-841's P1-A).
fn reopened_after_try_again(
    context: &Context,
    held: impl Fn(String) -> Quota,
    failed: bool,
) -> (Tiers<'_>, String) {
    context.adapter.resumes_records.set(true);
    context.adapter.rereads_failures.set(true);
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let zeus = tiers.assignee(1);
    context.advance(60_000);
    if failed {
        context.adapter.refuse(&zeus, held(now(context)));
    } else {
        context.adapter.quota(&zeus, Some(held(now(context))));
    }
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    assert!(tiers.out_until("zeus").is_some());
    context.advance(60_000);
    context.exit("zeus");
    context.pass().unwrap();
    context.advance(60_000);
    context.back_from_quota(tiers.project.id, "zeus").unwrap();
    // The words that resume it go into the reopened window; its turn begins, and the record it
    // reads is a turn that got through, not the refusal's (the fake does not write it by itself).
    for _ in 0..12 {
        context.pass().unwrap();
        if given(context, &zeus)
            .iter()
            .any(|said| said.contains("Resumed:"))
        {
            break;
        }
    }
    context.adapter.with(&zeus, |agent| {
        agent.failed = false;
        agent.quota = None;
    });
    context.advance(1_000);
    context.adapter.writes(&zeus, "Going on.", &now(context));
    for _ in 0..4 {
        context.pass().unwrap();
    }
    (tiers, zeus)
}

/// The words that resumed T-1 and the notes that say it failed, to be counted.
fn went_on(context: &Context, tiers: &Tiers<'_>, zeus: &str) {
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some(zeus)),
        "the task goes on, it is not failed by the refusal that held it"
    );
    let resumed = given(context, zeus)
        .iter()
        .filter(|said| said.contains("Resumed:"))
        .count();
    assert_eq!(resumed, 1, "{:?}", given(context, zeus));
    assert_eq!(
        tiers.out_until("zeus"),
        None,
        "not held again by the old record"
    );
    assert!(
        tiers
            .notes("chief")
            .iter()
            .all(|note| !note.contains("failed")),
        "{:?}",
        tiers.notes("chief")
    );
}

#[test]
fn a_task_held_for_an_account_goes_on_at_try_again_when_its_window_was_reopened_on_the_old_refusal()
{
    let context = Context::new();
    let (tiers, zeus) = reopened_after_try_again(
        &context,
        |at| Quota::Unavailable {
            at: Some(at),
            because: BECAUSE.to_owned(),
        },
        true,
    );
    went_on(&context, &tiers, &zeus);
}

#[test]
fn a_task_held_for_a_moved_model_goes_on_at_try_again_when_its_window_was_reopened_on_the_old_failed_turn(
) {
    // Muse's shape: the 402 fails the turn, and the reconfigure follows it.
    let context = Context::new();
    let (tiers, zeus) = reopened_after_try_again(&context, |at| fell_back(&at).as_quota(), true);
    went_on(&context, &tiers, &zeus);
}

#[test]
fn a_task_held_for_a_moved_model_goes_on_at_try_again_when_its_window_was_reopened_on_the_old_move()
{
    // Claude's `fallback` and Codex's reroute: no failed turn, the window rests on the move.
    let context = Context::new();
    let (tiers, zeus) = reopened_after_try_again(&context, |at| fell_back(&at).as_quota(), false);
    went_on(&context, &tiers, &zeus);
}

const BECAUSE: &str = "its billing was refused";
