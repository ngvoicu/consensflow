//! What a requester is told while its task waits, and what is taken back when
//! the task moves on: the note that a task was taken back from a member, that
//! it waits for a free one, or is held for its member's quota is withdrawn if
//! its reader has not been given it by the time another member takes the
//! task, or the daemon resumes it; and the requester who was given the note
//! of a hold is told when the task goes on. Node never withdraws such a note
//! and tells nothing of a hold's end, so none of these is held to a Node trace.

use cf_engine::testing::Context;
use cf_ledger::MessageView;

use crate::fixtures::{exhausted, given, now, placed, soon, Tiers};

/// The words of a requester told that a task goes on.
const GOES_ON: &str = "T-1 goes on: its account has quota again.";

/// The notes ConsensFlow wrote the chief that say `needle`, oldest first.
fn notes_saying(context: &Context, tiers: &Tiers, needle: &str) -> Vec<MessageView> {
    let mut found: Vec<MessageView> = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .filter(|message| {
            message.kind == "note" && message.sender.is_none() && message.body.contains(needle)
        })
        .collect();
    found.reverse();
    found
}

/// What a message is now: its state, and why it was withdrawn if it was.
fn fate(context: &Context, message: &MessageView) -> (String, Option<String>) {
    let now = context.message(message.id);
    (now.state, now.reason)
}

/// T-1 working in the session of zeus, the one standard worker of a project
/// whose chief is in the middle of a turn: what is written for it waits.
fn held_by_a_lone_worker(context: &Context) -> Tiers<'_> {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some("zeus-amber-pine"))
    );
    tiers
}

/// zeus runs out of quota until `hours` from now, with nobody else to take
/// its task: it is held with its window, and its agent, stopped, says where it
/// was. The refusal is dated, as a harness's record dates it.
fn runs_out_for(context: &Context, hours: i64) -> String {
    let resets_at = soon(context, hours);
    context.adapter.quota(
        "zeus",
        Some(exhausted(Some(&now(context)), Some(&resets_at))),
    );
    context.pass().unwrap();
    context.adapter.answer("zeus", "Stopped at the lexer.");
    context.pass().unwrap();
    resets_at
}

/// The human logs the harness into another account a minute later: zeus is
/// back, and the daemon goes on with what it held.
fn switches_account(context: &Context, tiers: &Tiers) {
    context.advance(60_000);
    context.back_from_quota(tiers.project.id, "zeus").unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
}

#[test]
fn withdraws_the_note_that_says_a_task_was_taken_back_when_another_member_takes_it_before_its_reader_is_given_it(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some("zeus-amber-pine"))
    );

    // The chief is in the middle of a turn when zeus runs out of quota, its
    // reset far enough that diana, who is free, takes the task.
    context.adapter.busy("chief");
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&soon(&context, 2)))));
    context.pass().unwrap();
    let notes = notes_saying(&context, &tiers, "was taken back");
    assert_eq!(
        notes.len(),
        1,
        "the chief is told the task waits for another"
    );
    assert_eq!(notes[0].state, "queued", "its window has not been given it");

    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("queued", Some("diana-brisk-birch")),
        "diana took the task while the note was waiting"
    );
    assert_eq!(
        fate(&context, &notes[0]),
        (
            "cancelled".to_owned(),
            Some("T-1 was taken by @diana".to_owned())
        )
    );

    // The chief's turn ends: it is never told the task waits.
    context.adapter.answer("chief", "Done.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains("was taken back")),
        "never pasted"
    );
}

#[test]
fn leaves_the_note_that_says_a_task_was_taken_back_once_its_reader_has_been_given_it() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&soon(&context, 2)))));
    context.pass().unwrap();
    let notes = notes_saying(&context, &tiers, "was taken back");
    assert_eq!(notes.len(), 1);

    // The chief is idle: the note reaches it before diana has the task.
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        context.message(notes[0].id).state,
        "delivered",
        "the chief was given it"
    );
    assert_eq!(
        tiers.task(1).task.assignee.as_deref(),
        Some("diana-brisk-birch")
    );
    assert_eq!(fate(&context, &notes[0]).1, None, "and it stays as it was");
}

#[test]
fn withdraws_the_note_that_says_a_task_waits_for_a_free_member_when_one_takes_it() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    context.pass().unwrap();
    // The one standard worker is out of quota, and the chief is busy.
    let until = soon(&context, 5);
    let zeus = tiers.id("zeus");
    context
        .ledger
        .borrow_mut()
        .mark_out(zeus, &until, "out of quota")
        .unwrap();
    context.adapter.busy("chief");
    tiers.open();
    context.pass().unwrap();
    let notes = notes_saying(&context, &tiers, "waits for a free standard worker");
    assert_eq!(notes.len(), 1, "told once that nobody is free");
    assert_eq!(notes[0].state, "queued");

    // The human says zeus is back: it takes the task.
    context.back_from_quota(tiers.project.id, "zeus").unwrap();
    context.pass().unwrap();
    assert_eq!(
        tiers.task(1).task.assignee.as_deref(),
        Some("zeus-amber-pine")
    );
    assert_eq!(
        fate(&context, &notes[0]),
        (
            "cancelled".to_owned(),
            Some("T-1 was taken by @zeus".to_owned())
        )
    );
}

#[test]
fn withdraws_the_note_that_says_a_task_is_held_when_the_account_is_switched_and_says_nothing_after()
{
    let context = Context::new();
    let tiers = held_by_a_lone_worker(&context);
    context.adapter.busy("chief");
    // The weekly limit: the reset is days away.
    let resets_at = runs_out_for(&context, 120);
    let held = tiers.task(1).task;
    assert_eq!(
        (held.state.as_str(), held.held_until),
        ("paused", Some(resets_at.clone()))
    );
    let notes = notes_saying(&context, &tiers, "waits with");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].state, "queued");

    // The human logs the harness into another account; the daemon goes on.
    switches_account(&context, &tiers);
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(
        fate(&context, &notes[0]),
        ("cancelled".to_owned(), Some("T-1 resumed".to_owned()))
    );
    assert!(
        notes_saying(&context, &tiers, GOES_ON).is_empty(),
        "nobody was told it waits, so nobody is told it goes on"
    );

    context.adapter.answer("chief", "Done.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains("waits with") && !text.contains(GOES_ON)),
        "the chief is never told the task waits for five days, nor that it does not"
    );

    // What the note said: the time the harness gave is a time it expects.
    assert_eq!(
        notes[0].body,
        format!(
            "T-1 waits with @zeus-amber-pine: out of quota until {resets_at}, or sooner if its account has quota again; it goes on by itself."
        )
    );
}

#[test]
fn tells_a_requester_it_was_told_a_task_waits_that_the_task_goes_on_when_the_account_is_switched() {
    let context = Context::new();
    let tiers = held_by_a_lone_worker(&context);
    runs_out_for(&context, 120);
    // The chief is idle: it is given the hold note.
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let notes = notes_saying(&context, &tiers, "waits with");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].state, "delivered", "the chief believes five days");
    context.adapter.answer("chief", "Noted.");

    switches_account(&context, &tiers);
    assert_eq!(tiers.task(1).task.state, "working");
    assert_eq!(
        fate(&context, &notes[0]),
        ("delivered".to_owned(), None),
        "what the chief was given stays"
    );
    let told = notes_saying(&context, &tiers, GOES_ON);
    assert_eq!(
        told.iter()
            .map(|note| note.body.as_str())
            .collect::<Vec<_>>(),
        [GOES_ON],
        "and is corrected, once"
    );
    assert_eq!(told[0].task_number, Some(1));

    // The correction reaches the chief, once, and the task goes on no more times.
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(told[0].id).state, "delivered");
    assert_eq!(notes_saying(&context, &tiers, GOES_ON).len(), 1);
}

#[test]
fn tells_a_requester_it_was_told_a_task_waits_that_the_task_goes_on_when_the_reset_passes() {
    let context = Context::new();
    let tiers = held_by_a_lone_worker(&context);
    runs_out_for(&context, 3);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(
        notes_saying(&context, &tiers, "waits with")[0].state,
        "delivered"
    );

    // The reset passes; the window shows the refusal no more.
    context.advance(3 * 3_600_000 + 60_000);
    context.adapter.quota("zeus", None);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    let told: Vec<String> = notes_saying(&context, &tiers, GOES_ON)
        .into_iter()
        .map(|note| note.body)
        .collect();
    assert_eq!(told, [GOES_ON]);
}
