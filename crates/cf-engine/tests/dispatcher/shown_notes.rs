//! The notes of ConsensFlow's own that ask the chief no decision are shown, not typed (RS-25,
//! Gabriel on 2026-11-11: "many useless messages sent to the chief we act"). A task held in
//! the background by work that ends by itself is the most of them (T-1410: 10 of the chief's
//! last 20 notes): its task's line says it, `cf task get` holds the note, and the chief's
//! window is given nothing of it. A note is typed after an hour in one stretch and every
//! three hours after that, however many times the worker holds again. An exchange starts a
//! new stretch; a hold that nothing but Finish ends is typed as it was.

use cf_engine::testing::Context;
use cf_engine::SwitchWhen;
use cf_harness::records::{End, Source};
use cf_ledger::NewNote;

use crate::chiefs::{to, with_codex};
use crate::fixtures::{after, exhausted, given, now, Tiers};
use crate::turn_end::{monitor, pending};

/// The notification Claude Code begins a turn with when a job of its ends.
const ENDED: &str =
    "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>";

/// `STILL_WORKING_REMINDED_AFTER_MS`: an hour.
const AN_HOUR: i64 = 60 * 60_000;
const FORTY_MINUTES: i64 = 40 * 60_000;

/// A project of zeus and diana, and T-1 given to zeus by name, at work in its window.
fn zeus_at_work(context: &Context) -> Tiers<'_> {
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    context.give(tiers.project.id, "zeus", "Record the run");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    tiers
}

/// The notes sent to the chief to be typed into its window (not shown), oldest first.
fn typed(context: &Context, tiers: &Tiers<'_>) -> Vec<String> {
    let mut notes: Vec<_> = context
        .ledger
        .borrow()
        .inbox(tiers.id("chief"), 100)
        .unwrap()
        .into_iter()
        .filter(|message| {
            message.kind == "note"
                && message.sender.is_none()
                && message.receipt.get("shown").is_none()
        })
        .map(|message| message.body)
        .collect();
    notes.reverse();
    notes
}

/// Reminder pastes the chief's window has actually been given.
fn pasted_reminders(context: &Context) -> Vec<String> {
    given(context, "chief")
        .into_iter()
        .filter(|paste| paste.contains("has been working in the background for"))
        .collect()
}

/// What T-1's line says beside its state.
fn aside(tiers: &Tiers<'_>) -> Option<String> {
    tiers.task(1).task.aside
}

/// Zeus ends its turn with `jobs` commands running in the background, and a pass looks.
fn held(context: &Context, words: &str, jobs: usize) {
    context
        .adapter
        .answer_leaving("zeus", words, pending(jobs, 0));
    context.pass().unwrap();
}

/// A background job ends, wakes its worker, and the worker starts another.
fn held_again(context: &Context, jobs: usize) {
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    held(context, "Still queued", jobs);
}

/// An ended socket monitor holds a task for which Finish is required.
fn held_for_finish(context: &Context) {
    context.adapter.answer_leaving_monitors(
        "zeus",
        "The feed closed",
        pending(1, 1),
        vec![monitor("feed", Source::Socket, Some(End::Closed))],
    );
    context.pass().unwrap();
}

/// Deliver a note at the current test time and let the chief finish its turn.
fn chief_reads(context: &Context) {
    for _ in 0..4 {
        context.pass().unwrap();
    }
    context.adapter.answer("chief", "Noted");
    context.pass().unwrap();
}

#[test]
fn a_worker_awake_with_background_work_is_reminded_at_the_first_hour() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The wait starts", 1);
    context.advance(FORTY_MINUTES);
    context.adapter.wake("zeus", ENDED);
    context
        .adapter
        .with("zeus", |agent| agent.pending = pending(2, 0));
    context.pass().unwrap();
    context.advance(AN_HOUR - FORTY_MINUTES);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers), ["T-1 has been working in the background for 60 minutes, holding again 0 times (2 commands). Its result comes when that ends; `cf task finish T-1` takes what it said last now."]);
}

#[test]
fn an_exchange_in_an_awake_turn_ends_the_old_stretch_before_it_holds_again() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The wait starts", 1);
    context.advance(FORTY_MINUTES);
    context.adapter.wake("zeus", ENDED);
    context
        .adapter
        .with("zeus", |agent| agent.pending = pending(2, 0));
    let assignee = tiers.assignee(1);
    context
        .ledger
        .borrow_mut()
        .note(
            tiers.project.id,
            &NewNote {
                from: Some(assignee),
                to: "chief".into(),
                task: Some(1),
                body: "A finding".into(),
            },
        )
        .unwrap();
    context.pass().unwrap();
    context.advance(AN_HOUR - FORTY_MINUTES);
    context.pass().unwrap();
    assert!(typed(&context, &tiers).is_empty(), "the old stretch ended");
    held(&context, "The next wait starts", 2);
    context.advance(AN_HOUR);
    context.pass().unwrap();
    assert_eq!(
        typed(&context, &tiers).len(),
        1,
        "the new stretch is an hour old"
    );
}

#[test]
fn an_awake_worker_with_nothing_pending_is_not_said_to_have_a_background_hold() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The wait starts", 1);
    context.advance(FORTY_MINUTES);
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context.advance(AN_HOUR - FORTY_MINUTES);
    context.pass().unwrap();
    assert!(typed(&context, &tiers).is_empty());
    context.advance(10 * 60_000);
    context
        .adapter
        .with("zeus", |agent| agent.pending = pending(1, 0));
    context.pass().unwrap();
    let notes = typed(&context, &tiers);
    assert_eq!(notes.len(), 1, "the background hold is overdue: {notes:?}");
    assert!(notes[0].contains("70 minutes, holding again 0 times (1 command)"));
}

#[test]
fn a_busy_chief_is_not_queued_repeated_reminders_before_the_first_is_delivered() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    context.adapter.busy("chief");
    held(&context, "The wait starts", 1);
    context.advance(AN_HOUR);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers).len(), 1);
    context.advance(6 * AN_HOUR);
    context.pass().unwrap();
    assert_eq!(
        typed(&context, &tiers).len(),
        1,
        "one reminder waits for the chief"
    );
    assert!(pasted_reminders(&context).is_empty());
    context.adapter.with("chief", |agent| agent.settled = true);
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(pasted_reminders(&context).len(), 1);
    context.adapter.answer("chief", "Noted");
    context.pass().unwrap();
    context.advance(3 * AN_HOUR - 1_000);
    context.pass().unwrap();
    assert_eq!(
        typed(&context, &tiers).len(),
        1,
        "three hours after delivery"
    );
    context.advance(1_000);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers).len(), 2);
}

#[test]
fn a_finish_hold_does_not_stack_reminders_for_a_busy_chief() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held_for_finish(&context);
    chief_reads(&context);
    context.adapter.busy("chief");
    context.advance(AN_HOUR);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers).len(), 2);
    context.advance(6 * AN_HOUR);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers).len(), 2, "one reminder waits");
    context.adapter.with("chief", |agent| agent.settled = true);
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(pasted_reminders(&context).len(), 1);
    context.adapter.answer("chief", "Noted");
    context.pass().unwrap();
    context.advance(3 * AN_HOUR);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers).len(), 3);
}

#[test]
fn a_hold_keeps_its_stretch_when_it_needs_finish_then_can_end_again() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "Watching", 1);
    context.advance(FORTY_MINUTES);
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    held_for_finish(&context);
    assert_eq!(typed(&context, &tiers).len(), 1, "Finish is told at once");
    context.advance(10 * 60_000);
    held_again(&context, 2);
    context.advance(10 * 60_000);
    context.pass().unwrap();
    let notes = typed(&context, &tiers);
    assert_eq!(notes.len(), 2, "the first hour still counts: {notes:?}");
    assert!(
        notes[1].contains("60 minutes, holding again 2 times (2 commands)"),
        "{notes:?}"
    );
}

#[test]
fn a_hold_that_first_needs_finish_keeps_its_stretch_when_it_can_end() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held_for_finish(&context);
    assert_eq!(typed(&context, &tiers).len(), 1, "Finish is told at once");
    context.advance(FORTY_MINUTES);
    held_again(&context, 1);
    context.advance(AN_HOUR - FORTY_MINUTES);
    context.pass().unwrap();
    let notes = typed(&context, &tiers);
    assert_eq!(notes.len(), 2, "the first hour still counts: {notes:?}");
    assert!(
        notes[1].contains("60 minutes, holding again 1 time (1 command)"),
        "{notes:?}"
    );
}

#[test]
fn a_hold_that_keeps_needing_finish_is_reminded_without_promising_a_result() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held_for_finish(&context);
    assert_eq!(typed(&context, &tiers).len(), 1, "Finish is told at once");
    chief_reads(&context);
    context.advance(AN_HOUR);
    context.pass().unwrap();
    let notes = typed(&context, &tiers);
    assert_eq!(notes.len(), 2, "remind after an hour: {notes:?}");
    assert!(
        notes[1].contains("60 minutes, holding again 0 times"),
        "{notes:?}"
    );
    assert!(
        notes[1].contains("result will not come by itself"),
        "{notes:?}"
    );
    chief_reads(&context);
    context.advance(3 * AN_HOUR);
    context.pass().unwrap();
    assert_eq!(
        typed(&context, &tiers).len(),
        3,
        "remind after three more hours"
    );
}

#[test]
fn a_reminder_before_a_finish_hold_can_end_is_carried_into_the_shown_hold() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held_for_finish(&context);
    chief_reads(&context);
    context.advance(AN_HOUR);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers).len(), 2);
    chief_reads(&context);
    context.advance(20 * 60_000);
    held_again(&context, 1);
    context.advance(AN_HOUR);
    context.pass().unwrap();
    assert_eq!(
        typed(&context, &tiers).len(),
        2,
        "no new hour for a shown hold"
    );
    context.advance(100 * 60_000);
    context.pass().unwrap();
    let notes = typed(&context, &tiers);
    assert_eq!(notes.len(), 3, "three hours after the reminder: {notes:?}");
    assert!(
        notes[2].contains("4 h 0 min, holding again 1 time (1 command)"),
        "{notes:?}"
    );
}

#[test]
fn an_exchange_during_a_finish_hold_starts_a_new_stretch() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held_for_finish(&context);
    context.advance(FORTY_MINUTES);
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    let assignee = tiers.assignee(1);
    context
        .ledger
        .borrow_mut()
        .note(
            tiers.project.id,
            &NewNote {
                from: Some(assignee),
                to: "chief".into(),
                task: Some(1),
                body: "A finding for the chief".into(),
            },
        )
        .unwrap();
    held_for_finish(&context);
    context.advance(AN_HOUR - FORTY_MINUTES);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers).len(), 1, "the earlier hour is over");
    context.advance(FORTY_MINUTES);
    context.pass().unwrap();
    let notes = typed(&context, &tiers);
    assert_eq!(notes.len(), 2, "the new hour is over: {notes:?}");
    assert!(
        notes[1].contains("60 minutes, holding again 0 times"),
        "{notes:?}"
    );
}

#[test]
fn a_worker_who_holds_again_after_forty_minutes_is_typed_at_the_first_hour() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The wait starts", 1);
    context.advance(FORTY_MINUTES);
    held_again(&context, 2);
    assert!(typed(&context, &tiers).is_empty());
    context.advance(AN_HOUR - FORTY_MINUTES - 1_000);
    context.pass().unwrap();
    assert!(
        typed(&context, &tiers).is_empty(),
        "not before the first hour"
    );
    context.advance(1_000);
    context.pass().unwrap();
    assert_eq!(
        typed(&context, &tiers),
        ["T-1 has been working in the background for 60 minutes, holding again 1 time (2 commands). Its result comes when that ends; `cf task finish T-1` takes what it said last now."]
    );
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let pastes = pasted_reminders(&context);
    assert_eq!(pastes.len(), 1, "{pastes:?}");
    assert!(pastes[0].contains(&typed(&context, &tiers)[0]));
}

#[test]
fn a_worker_who_keeps_holding_every_forty_minutes_is_typed_again_at_four_hours() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The wait starts", 1);
    context.advance(FORTY_MINUTES);
    held_again(&context, 1);
    context.advance(AN_HOUR - FORTY_MINUTES);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers).len(), 1);
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(pasted_reminders(&context).len(), 1);
    context.adapter.answer("chief", "Noted.");
    context.pass().unwrap();
    // The worker wakes at 80, 120, 160, 200 and 240 minutes.
    context.advance(FORTY_MINUTES / 2);
    for repeat in 2..=6 {
        held_again(&context, repeat);
        if repeat < 6 {
            assert_eq!(typed(&context, &tiers).len(), 1, "repeat {repeat}");
            context.advance(FORTY_MINUTES);
        }
    }
    assert_eq!(
        typed(&context, &tiers),
        [
            "T-1 has been working in the background for 60 minutes, holding again 1 time (1 command). Its result comes when that ends; `cf task finish T-1` takes what it said last now.",
            "T-1 has been working in the background for 4 h 0 min, holding again 6 times (6 commands). Its result comes when that ends; `cf task finish T-1` takes what it said last now."
        ]
    );
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(
        typed(&context, &tiers).len(),
        2,
        "no duplicate at four hours"
    );
    let pastes = pasted_reminders(&context);
    assert_eq!(pastes.len(), 2, "{pastes:?}");
    assert!(pastes[1].contains(&typed(&context, &tiers)[1]));
}

#[test]
fn a_worker_who_sends_its_own_note_starts_a_new_stretch_at_its_next_hold() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The wait starts", 1);
    context.advance(FORTY_MINUTES);
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    let assignee = tiers.assignee(1);
    context
        .ledger
        .borrow_mut()
        .note(
            tiers.project.id,
            &NewNote {
                from: Some(assignee),
                to: "chief".into(),
                task: Some(1),
                body: "A finding for the chief".into(),
            },
        )
        .unwrap();
    held(&context, "The next wait starts", 2);
    context.advance(AN_HOUR - FORTY_MINUTES);
    context.pass().unwrap();
    assert!(typed(&context, &tiers).is_empty(), "the old hour is over");
    context.advance(FORTY_MINUTES);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers), ["T-1 has been working in the background for 60 minutes, holding again 0 times (2 commands). Its result comes when that ends; `cf task finish T-1` takes what it said last now."]);
}

#[test]
fn a_worker_who_asks_and_is_answered_starts_a_new_stretch() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The wait starts", 1);
    context.advance(FORTY_MINUTES);
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context
        .adapter
        .answer("zeus", "Should I continue? I'm waiting for your answer.");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "waiting");
    let question = tiers
        .task(1)
        .messages
        .into_iter()
        .find(|m| m.kind == "question")
        .unwrap();
    context.answer(tiers.project.id, question.id, "Continue");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    held(&context, "The next wait starts", 1);
    context.advance(AN_HOUR - FORTY_MINUTES);
    context.pass().unwrap();
    assert!(typed(&context, &tiers).is_empty(), "the old hour is over");
    context.advance(FORTY_MINUTES);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers), ["T-1 has been working in the background for 60 minutes, holding again 0 times (1 command). Its result comes when that ends; `cf task finish T-1` takes what it said last now."]);
}

#[test]
fn a_worker_given_a_question_while_held_starts_a_new_stretch_when_it_holds_again() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The wait starts", 1);
    context.advance(FORTY_MINUTES);
    let question = context.ask(
        tiers.project.id,
        "chief",
        &tiers.assignee(1),
        1,
        "Check another host",
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(question.id).state, "delivered");
    held(&context, "The other wait starts", 1);
    context.advance(AN_HOUR - FORTY_MINUTES);
    context.pass().unwrap();
    assert!(typed(&context, &tiers).is_empty(), "the old hour is over");
    context.advance(FORTY_MINUTES);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers), ["T-1 has been working in the background for 60 minutes, holding again 0 times (1 command). Its result comes when that ends; `cf task finish T-1` takes what it said last now."]);
}

#[test]
fn a_worker_whose_result_is_followed_by_more_work_starts_a_new_stretch() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The wait starts", 1);
    context.advance(FORTY_MINUTES);
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context.adapter.answer("zeus", "First result");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 1, "chief", "Do more")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    held(&context, "The next wait starts", 1);
    context.advance(AN_HOUR - FORTY_MINUTES);
    context.pass().unwrap();
    assert!(typed(&context, &tiers).is_empty(), "the old hour is over");
    context.advance(FORTY_MINUTES);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers), ["T-1 has been working in the background for 60 minutes, holding again 0 times (1 command). Its result comes when that ends; `cf task finish T-1` takes what it said last now."]);
}

#[test]
fn a_hold_that_ends_by_itself_is_shown_on_its_task_and_not_typed() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The Windows run starts", 1);

    assert_eq!(typed(&context, &tiers), Vec::<String>::new());
    assert_eq!(
        aside(&tiers).as_deref(),
        Some("in the background: 1 command")
    );
    // `cf task get` holds what the note said, as it did.
    let thread = tiers.task(1);
    let note = thread
        .messages
        .iter()
        .find(|message| message.kind == "note")
        .expect("the note is in the task's thread");
    assert_eq!(
        note.body,
        "T-1 is still working in the background (1 command); its result comes when it ends. `cf task finish T-1` takes what it said last now.\n\nWhat it said:\n\nThe Windows run starts"
    );
    assert_eq!(note.state, "read");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains("in the background")),
        "{:?}",
        given(&context, "chief")
    );
}

#[test]
fn a_turn_of_the_workers_own_that_leaves_it_held_again_says_what_runs_now_and_writes_no_note() {
    // poker-lab's T-442: a Monitor wakes its worker at each event; a note a turn would be a
    // note a tick, typed or not.
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The Windows run starts", 1);
    assert_eq!(
        aside(&tiers).as_deref(),
        Some("in the background: 1 command")
    );

    for (round, jobs) in [2, 3, 1].into_iter().enumerate() {
        context.adapter.wake("zeus", ENDED);
        context.pass().unwrap();
        held(&context, &format!("Round {round}"), jobs);
    }
    assert_eq!(
        aside(&tiers).as_deref(),
        Some("in the background: 1 command")
    );
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    held(&context, "The Linux runs start", 2);
    assert_eq!(
        aside(&tiers).as_deref(),
        Some("in the background: 2 commands")
    );
    assert_eq!(typed(&context, &tiers), Vec::<String>::new());
    let notes: Vec<_> = tiers
        .task(1)
        .messages
        .into_iter()
        .filter(|message| message.kind == "note")
        .collect();
    assert_eq!(
        notes.len(),
        1,
        "one note for the hold, as before: {notes:?}"
    );

    // The hold ends with a question: the task waits, and the line says no more of the hold.
    context.adapter.wake("zeus", ENDED);
    context.pass().unwrap();
    context.adapter.answer(
        "zeus",
        "Both runs failed on the same test. Waiting for your answer: should I skip it or fix it?",
    );
    for _ in 0..2 {
        context.pass().unwrap();
    }
    assert_eq!(
        tiers.task(1).task.state,
        "waiting",
        "it waits for the chief's answer"
    );
    assert_eq!(aside(&tiers), None, "{:?}", tiers.task(1).task);
}

#[test]
fn a_single_long_hold_is_typed_at_one_hour_and_again_at_four_hours() {
    let context = Context::new();
    let tiers = zeus_at_work(&context);
    held(&context, "The soak test starts", 1);

    context.advance(AN_HOUR - 1_000);
    context.pass().unwrap();
    assert_eq!(
        typed(&context, &tiers),
        Vec::<String>::new(),
        "not before the hour"
    );

    context.advance(1_000);
    context.pass().unwrap();
    let reminder = "T-1 has been working in the background for 60 minutes, holding again 0 times (1 command). Its result comes when that ends; `cf task finish T-1` takes what it said last now.";
    assert_eq!(typed(&context, &tiers), [reminder]);
    chief_reads(&context);
    assert_eq!(
        aside(&tiers).as_deref(),
        Some("in the background: 1 command"),
        "the line still says it"
    );

    for _ in 0..2 {
        context.advance(AN_HOUR);
        context.pass().unwrap();
    }
    assert_eq!(typed(&context, &tiers), [reminder]);
    context.advance(AN_HOUR - 1_000);
    context.pass().unwrap();
    assert_eq!(typed(&context, &tiers).len(), 1);
    context.advance(1_000);
    context.pass().unwrap();
    assert_eq!(
        typed(&context, &tiers),
        [reminder, "T-1 has been working in the background for 4 h 0 min, holding again 0 times (1 command). Its result comes when that ends; `cf task finish T-1` takes what it said last now."]
    );
}

#[test]
fn a_restart_ends_a_background_hold_and_writes_no_note_of_it_again() {
    // T-1500's finding 3 feared that a restart, which empties the engine's memory of a hold,
    // writes a fresh shown note whose time the hour then counts from. No window survives a
    // restart (`cf-daemon`'s start), and the hold ends with its window: on the Resume the
    // turn is read from its conversation and its result typed to the chief, and no note of
    // the hold is written again.
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let tiers = zeus_at_work(&context);
    held(&context, "The soak test starts", 1);
    let notes = |tiers: &Tiers<'_>| {
        tiers
            .task(1)
            .messages
            .into_iter()
            .filter(|message| message.kind == "note")
            .count()
    };
    assert_eq!(notes(&tiers), 1);

    context.advance(AN_HOUR - 10 * 60_000);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(tiers.project.id).unwrap();
    for _ in 0..4 {
        after.pass().unwrap();
    }
    let thread = tiers.task(1);
    assert_eq!(thread.task.state, "done", "{:#?}", thread.messages);
    assert_eq!(
        notes(&tiers),
        1,
        "the hold is told once: {:#?}",
        thread.messages
    );
    assert_eq!(aside(&tiers), None, "its line says no hold");
    let result = thread
        .messages
        .iter()
        .find(|message| message.kind == "result")
        .expect("its result");
    assert_eq!(result.recipient, "chief");
    assert_eq!(result.body, "The soak test starts");
}

#[test]
fn a_member_out_of_quota_with_seven_tasks_types_each_task_s_hold_once_in_a_paste_of_its_own() {
    // RS-25: the first note of a hold is typed, once for each task, one paste a note.
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    for n in 1..=7 {
        tiers.open_body(&format!("Part {n}"));
    }
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!((1..=7).all(|n| tiers.task(n).task.state == "working"));
    context.adapter.busy("chief");
    let before = given(&context, "chief").len();
    let resets_at = after(&context, 120 * 3_600_000);
    for n in 1..=7 {
        context.adapter.refuse(
            &tiers.assignee(n),
            exhausted(Some(&now(&context)), Some(&resets_at)),
        );
    }
    context.pass().unwrap();
    let states: Vec<String> = (1..=7).map(|n| tiers.task(n).task.state).collect();
    assert!(states.iter().all(|state| state == "paused"), "{states:?}");
    for _ in 0..12 {
        context.adapter.answer("chief", "Done.");
        context.pass().unwrap();
    }
    let pasted: Vec<String> = given(&context, "chief").split_off(before);
    assert_eq!(pasted.len(), 7, "a paste a task: {pasted:?}");
    for n in 1..=7 {
        let hold = format!(
            "T-{n} waits with @{}: out of quota until {resets_at}",
            tiers.assignee(n)
        );
        let with = pasted.iter().filter(|paste| paste.contains(&hold)).count();
        assert_eq!(with, 1, "T-{n}'s hold is typed once: {pasted:?}");
        // Each task's line says its hold, typed as it was.
        assert_eq!(
            tiers.task(n).task.aside,
            Some(format!(
                "waits with @{}: out of quota until {resets_at}",
                tiers.assignee(n)
            ))
        );
    }
    for paste in &pasted {
        assert_eq!(paste.matches(" waits with @").count(), 1, "{paste}");
    }
}

/// T-1 for zeus, at work and then refused for quota until five days from now, so its hold is
/// typed to the chief: the hold's words.
fn held_for_quota(context: &Context, tiers: &Tiers<'_>) -> String {
    tiers.open_body("Part 1");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "working");
    let zeus = tiers.assignee(1);
    let resets_at = after(context, 120 * 3_600_000);
    context
        .adapter
        .refuse(&zeus, exhausted(Some(&now(context)), Some(&resets_at)));
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(tiers.task(1).task.state, "paused");
    let hold = format!("waits with @{zeus}: out of quota until {resets_at}");
    assert!(
        given(context, "chief")
            .iter()
            .any(|text| text.contains(&format!("T-1 {hold}"))),
        "the old chief was typed the hold: {:?}",
        given(context, "chief")
    );
    hold
}

/// The first message of the chief Codex's astraeus takes over with.
fn handoff_on_switch(context: &Context, tiers: &Tiers<'_>) -> String {
    context
        .switch_chief(
            tiers.project.id,
            to("codex", "astraeus"),
            SwitchWhen::Now,
            false,
        )
        .unwrap();
    let prepared = context.codex.prepared().last().cloned().unwrap();
    prepared["message"].as_str().unwrap().to_owned()
}

#[test]
fn a_chief_switched_while_a_task_holds_is_told_the_hold_in_its_handoff() {
    // The first note of a hold was typed to the old chief; the new chief starts a new
    // conversation, and the same hold said again is only shown: its handoff names the
    // task and the hold (T-1500's finding 2).
    let context = with_codex();
    let tiers = Tiers::new(&context, &["zeus"]);
    let hold = held_for_quota(&context, &tiers);
    let handoff = handoff_on_switch(&context, &tiers);
    assert!(
        handoff.contains(&format!(
            "What waits on you now:\n- T-1 \"{}\" {hold} (cf task get T-1)",
            tiers.task(1).task.title
        )),
        "{handoff}"
    );
    assert!(
        !handoff.contains("Nothing on the board waits on you."),
        "{handoff}"
    );
}

#[test]
fn a_chief_switched_after_a_held_task_s_member_left_is_told_it_is_paused_and_no_hold() {
    // T-1514's finding A: the human removes the member whose task is held; the task stays
    // paused for its resumption, and the new chief is told so, not that it waits with a member
    // that is gone.
    let context = with_codex();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    held_for_quota(&context, &tiers);
    context.remove_member(tiers.project.id, "zeus").unwrap();
    assert_eq!(tiers.task(1).task.state, "paused");
    let handoff = handoff_on_switch(&context, &tiers);
    assert!(
        handoff.contains(&format!(
            "What waits on you now:\n- T-1 \"{}\" is paused, and its member @zeus has left the staff (cf task get T-1)",
            tiers.task(1).task.title
        )),
        "{handoff}"
    );
    assert!(!handoff.contains("waits with @"), "{handoff}");
}
