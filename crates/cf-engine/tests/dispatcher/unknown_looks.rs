//! A read of a window that fails, or answers too late to say what the window does
//! now, is unknown: it counts as no rest towards a paste's arrival window, and no
//! Enter is pressed and nothing is pasted again on it (`arrival`). Before a paste
//! its record never showed goes again, the window is read once more, and a record
//! that shows it then confirms it. And a pass waits a bounded time for its steps: a
//! window whose read hangs is left to it, and the other windows are served.
//!
//! The report: 2026-10-22, ConsensFlow 3.0.5, this board under a load average of
//! about 100. m-3093, T-243's brief to the chief, was handed over at 14:21:45Z; the
//! pane host answered nothing in time for three minutes (two passes of 30 s and
//! 37 s, each a snapshot at the bridge's 30 s deadline), while the chief's record and
//! status read at rest. The arrival window ran on through the stalls, the brief was
//! pasted again at 14:24:42Z, and both copies reached the chief: the first began its
//! turn at 14:24:42.689Z, the second went into that turn's queue 3 s later (the
//! chief's transcript, offsets 36845750 and 36866108). None of these is held to a
//! Node trace: Node's dispatcher read no window once more and waited for every step.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::delivery_text::delivery_text;
use cf_engine::testing::Context;
use cf_harness::records::Role;
use cf_harness::seams::Time;
use cf_proto::ledger::MessageView;
use serde_json::{json, Value};

use crate::deliveries::enters;
use crate::fixtures::Tiers;
use crate::work_in_flight::looks_at;

/// The engine's arrival window in the kit, in milliseconds.
const ARRIVAL_MS: i64 = 30_000;

/// What a look that could not read a record reads, as the records thread says one
/// whose read failed.
const UNREADABLE: &str = "unreadable: the records thread ended";

/// How many times a window was handed a message from now on.
fn counting(context: &Context) -> Rc<Cell<usize>> {
    let made = Rc::new(Cell::new(0));
    let counted = Rc::clone(&made);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        counted.set(counted.get() + 1);
        Box::pin(async move { Ok(taking()) })
    }));
    made
}

/// `message` shows in the record of `handle`'s window: the harness ran it.
fn shows(context: &Context, handle: &str, message: &MessageView) {
    let item = context
        .adapter
        .item(Role::User, &delivery_text(message, &[]));
    context.adapter.with(handle, |agent| agent.items.push(item));
}

/// A note from zeus on its way into the chief's window, which its record does not
/// show yet: the project's id and the note.
fn on_its_way(context: &Context) -> (i64, MessageView) {
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    (project.id, note)
}

/// The state and attempts of `message` now.
fn where_is(context: &Context, message: &MessageView) -> (String, i64) {
    let now = context.message(message.id);
    (now.state, now.attempts)
}

#[test]
fn never_pastes_again_a_message_while_every_look_cannot_read_its_record() {
    let context = Context::new();
    let pasted = counting(&context);
    let (_, note) = on_its_way(&context);
    assert_eq!(pasted.get(), 1);

    // The window took it, but no look can read the record: eight arrival windows
    // of looks that say nothing, the window at rest throughout as its own word goes,
    // and quiet, as one an Enter would be pressed into.
    context.adapter.with("chief", |agent| {
        agent.unreadable = Some(UNREADABLE.to_owned());
    });
    context
        .host
        .set_snapshot(json!({ "ok": true, "outputQuietMs": 5_000 }));
    context.advance(11_000);
    context.pass().unwrap();
    for _ in 0..8 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 1));
    assert_eq!(pasted.get(), 1, "not pasted again");
    assert_eq!(
        enters(&context),
        0,
        "no Enter pressed on a look that read nothing"
    );

    // The record can be read again, and shows it: confirmed, once.
    context
        .adapter
        .with("chief", |agent| agent.unreadable = None);
    shows(&context, "chief", &context.message(note.id));
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivered".to_owned(), 1));
    assert_eq!(pasted.get(), 1);
}

#[test]
fn presses_no_enter_and_pastes_nothing_again_while_the_pane_host_cannot_say_what_the_window_does() {
    let context = Context::new();
    let pasted = counting(&context);
    let (project, note) = on_its_way(&context);
    // Every snapshot comes back at the bridge's deadline.
    context
        .host
        .set_snapshot(json!({ "ok": false, "error": "deadline" }));
    context.advance(11_000);
    context.pass().unwrap();
    for _ in 0..8 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 1));
    assert_eq!(pasted.get(), 1, "not pasted again");
    assert_eq!(
        enters(&context),
        0,
        "no Enter into a window nobody could read"
    );

    // The host answers again, at once, and nothing goes in: a paste the record
    // still does not show after a whole arrival window of rest goes again.
    context
        .host
        .set_snapshot(json!({ "ok": true, "outputQuietMs": 5_000 }));
    context.advance(ARRIVAL_MS + 1_000);
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 2));
    assert_eq!(pasted.get(), 2, "pasted again once");
    let events = context.ledger.borrow().events(project, 0, 500).unwrap();
    let why = events
        .iter()
        .rfind(|event| event.kind == "delivery.retried")
        .and_then(|event| event.data["reason"].as_str().map(str::to_owned));
    assert_eq!(
        why.as_deref(),
        Some("the harness record never showed it in the 30 s its window was at rest since it could be read again")
    );
}

#[test]
fn pastes_nothing_again_while_the_pane_host_answers_too_late_and_confirms_what_the_window_ran_when_it_comes_back(
) {
    let context = Context::new();
    let pasted = counting(&context);
    let (_, note) = on_its_way(&context);
    // The field's shape: the host answers each snapshot, but 31 s after it was
    // asked, while the window's record and its own word read at rest.
    let clock = Rc::clone(&context.time);
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op: &str, _: &Value| {
        if op == "pane.snapshot" {
            clock.settle_at(clock.wall_ms() + 31_000);
        }
    }));
    context
        .host
        .set_snapshot(json!({ "ok": true, "outputQuietMs": 60_000 }));
    context.advance(11_000);
    context.pass().unwrap();
    for _ in 0..6 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 1));
    assert_eq!(pasted.get(), 1, "not pasted again");
    assert_eq!(
        enters(&context),
        0,
        "no Enter on a snapshot as old as its stall"
    );

    // The host comes back, and the window runs what it was handed at last.
    *context.host.on_request.borrow_mut() = None;
    shows(&context, "chief", &context.message(note.id));
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivered".to_owned(), 1));
    assert_eq!(pasted.get(), 1, "the window was given it once");
}

#[test]
fn confirms_a_paste_its_record_shows_when_it_is_read_once_more_before_it_would_go_again() {
    let context = Context::new();
    let pasted = counting(&context);
    let (_, note) = on_its_way(&context);
    context.advance(ARRIVAL_MS + 1_000);
    // The record shows it by the time the window is read once more: the host's
    // answer comes as the harness writes it.
    let (adapter, header) = (
        Rc::downgrade(&context.adapter),
        delivery_text(&context.message(note.id), &[]),
    );
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op: &str, _: &Value| {
        if let (true, Some(adapter)) = (op == "pane.snapshot", adapter.upgrade()) {
            let item = adapter.item(Role::User, &header);
            adapter.with("chief", |agent| agent.items.push(item));
        }
    }));
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivered".to_owned(), 1));
    assert_eq!(pasted.get(), 1, "never pasted again");
}

#[test]
fn a_pass_leaves_a_window_whose_look_hangs_to_it_and_serves_the_others() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");

    // zeus's look hangs: its harness's record never answers.
    let launch = context.adapter.agent("zeus").launch;
    let release = context.adapter.observe_holds.hold(looks_at(launch));
    let note = context.note(tiers.project.id, "zeus", "chief", "hello");
    context.pass().unwrap();
    assert_eq!(
        context.message(note.id).state,
        "delivering",
        "handed to the chief's window in the pass that waited for zeus's look"
    );
    // The passes after it serve the chief while zeus's look still hangs.
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivered");

    // zeus's look answers, and its window is stepped again.
    release.open();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "done");
}

/// The reason of the last `delivery.retried` of `project`.
fn why_retried(context: &Context, project: i64) -> String {
    let events = context.ledger.borrow().events(project, 0, 500).unwrap();
    events
        .iter()
        .rfind(|event| event.kind == "delivery.retried")
        .and_then(|event| event.data["reason"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[test]
fn counts_no_rest_before_a_snapshot_for_the_enter_that_failed() {
    let context = Context::new();
    let pasted = counting(&context);
    let (project, note) = on_its_way(&context);
    // The Enter's snapshot meets its deadline once; the host answers after it.
    context
        .host
        .set_snapshot(json!({ "ok": false, "error": "deadline" }));
    context.advance(11_000);
    context.pass().unwrap();
    context
        .host
        .set_snapshot(json!({ "ok": true, "outputQuietMs": 5_000 }));
    context.advance(1_000);
    context.pass().unwrap();
    // Past an arrival window from the hand-over, but not from the look after the
    // failed read: the rest before it is not known, and counts for nothing.
    context.advance(20_000);
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 1));
    context.advance(11_000);
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 2));
    assert_eq!(pasted.get(), 2);
    assert_eq!(
        why_retried(&context, project),
        "the harness record never showed it in the 30 s its window was at rest since it could be read again"
    );
}

#[test]
fn pastes_nothing_again_while_the_pane_host_says_a_paste_is_still_going_in() {
    let context = Context::new();
    let pasted = counting(&context);
    let (_, note) = on_its_way(&context);
    context.host.set_snapshot(json!({ "pasteInFlight": true }));
    for _ in 0..4 {
        context.advance(ARRIVAL_MS + 1_000);
        context.pass().unwrap();
    }
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 1));
    assert_eq!(pasted.get(), 1, "not pasted behind the one going in");
    // It went in, and the record had its window: it goes again.
    context.host.set_snapshot(json!({ "pasteInFlight": false }));
    context.advance(ARRIVAL_MS + 1_000);
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 2));
}

#[test]
fn pastes_nothing_again_when_the_record_cannot_be_read_as_the_window_is_read_once_more() {
    let context = Context::new();
    let pasted = counting(&context);
    let (_, note) = on_its_way(&context);
    context.advance(ARRIVAL_MS + 1_000);
    let adapter = Rc::downgrade(&context.adapter);
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op: &str, _: &Value| {
        if let (true, Some(adapter)) = (op == "pane.snapshot", adapter.upgrade()) {
            adapter.with("chief", |agent| {
                agent.unreadable = Some(UNREADABLE.to_owned());
            });
        }
    }));
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 1));
    assert_eq!(
        pasted.get(),
        1,
        "not pasted again on a record nobody could read"
    );
}

#[test]
fn pastes_nothing_again_into_a_window_found_in_a_turn_as_it_is_read_once_more() {
    let context = Context::new();
    let pasted = counting(&context);
    let (project, note) = on_its_way(&context);
    context.advance(ARRIVAL_MS + 1_000);
    let adapter = Rc::downgrade(&context.adapter);
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op: &str, _: &Value| {
        if let (true, Some(adapter)) = (op == "pane.snapshot", adapter.upgrade()) {
            adapter.busy("chief");
        }
    }));
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 1));
    assert_eq!(pasted.get(), 1, "not pasted into the turn");
    // At rest again with nothing shown, it has its whole window, then goes again.
    *context.host.on_request.borrow_mut() = None;
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    context.advance(ARRIVAL_MS + 1_000);
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 2));
    assert_eq!(
        why_retried(&context, project),
        "the harness record never showed it in the 30 s its window was at rest since its last turn"
    );
}

#[test]
fn counts_no_rest_before_a_look_that_failed() {
    let context = Context::new();
    let pasted = counting(&context);
    let (_, note) = on_its_way(&context);
    context.advance(20_000);
    context.pass().unwrap();
    // One look fails.
    let (launch, once) = (
        context.adapter.agent("chief").launch,
        Rc::new(Cell::new(true)),
    );
    let picks = looks_at(launch);
    context
        .adapter
        .observe_holds
        .hold_instead(
            move |args| picks(args) && once.replace(false),
            "the record could not be read".to_owned(),
        )
        .open();
    context.advance(1_000);
    context.pass().unwrap();
    // Past an arrival window from the hand-over, at rest all of it as the looks
    // that read the window say: the time before the failed look counts for nothing.
    context.advance(15_000);
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 1));
    assert_eq!(pasted.get(), 1);
}

#[test]
fn writes_down_a_step_that_fails_after_its_pass_went_on() {
    let context = Context::new();
    context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let launch = context.adapter.agent("chief").launch;
    let release = context.adapter.observe_holds.hold(looks_at(launch));
    context.pass().unwrap();
    // The chief's look answers after the pass went on, and its step fails.
    let failing = Rc::new(Cell::new(true));
    context.ledger.borrow_mut().watch(Box::new(move |made, _| {
        if made == "next_delivery" && failing.replace(false) {
            return Err(cf_ledger::LedgerError::refused(
                "ledger-failed",
                "the ledger could not be read",
            ));
        }
        Ok(())
    }));
    release.open();
    context.settle();
    assert_eq!(
        *context.log.failures.borrow(),
        ["the ledger could not be read"]
    );
}

#[test]
fn pastes_nothing_again_that_the_pane_host_wrote_only_as_the_window_is_read_once_more() {
    let context = Context::new();
    let pasted = counting(&context);
    let (project, note) = on_its_way(&context);
    context.advance(ARRIVAL_MS + 1_000);
    // The host was slow: as the window is read once more, it catches up, writes the
    // paste it held, and answers at once that nothing is going in. The record shows
    // the paste a look later.
    let (host, asked) = (Rc::downgrade(&context.host), Rc::new(Cell::new(0)));
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op: &str, _: &Value| {
        if let (true, Some(host)) = (op == "pane.snapshot", host.upgrade()) {
            asked.set(asked.get() + 1);
            host.set_snapshot(if asked.get() == 1 {
                json!({ "ok": true, "pasteQuietMs": 0 })
            } else {
                json!({ "ok": true })
            });
        }
    }));
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 1));
    assert_eq!(
        pasted.get(),
        1,
        "not pasted again behind the one just written"
    );
    assert!(
        context
            .log
            .warnings
            .borrow()
            .iter()
            .any(|line| line.contains("was written a paste 0 ms ago")),
        "{:?}",
        context.log.warnings.borrow()
    );

    // Its record has a whole arrival window from the write, and a paste it never
    // shows goes again once that is past.
    context.advance(ARRIVAL_MS - 1_000);
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 1));
    context.advance(2_000);
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivering".to_owned(), 2));
    assert_eq!(
        why_retried(&context, project),
        "the harness record never showed it in the 30 s its window was at rest since it could be read again"
    );
}

#[test]
fn confirms_a_paste_the_pane_host_wrote_only_as_the_window_is_read_once_more_when_the_record_shows_it_a_look_later(
) {
    let context = Context::new();
    let pasted = counting(&context);
    let (_, note) = on_its_way(&context);
    context.advance(ARRIVAL_MS + 1_000);
    let (host, once) = (Rc::downgrade(&context.host), Rc::new(Cell::new(true)));
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op: &str, _: &Value| {
        if let (true, Some(host)) = (op == "pane.snapshot" && once.replace(false), host.upgrade()) {
            host.set_snapshot(json!({ "ok": true, "pasteQuietMs": 0 }));
        }
    }));
    context.pass().unwrap();
    shows(&context, "chief", &context.message(note.id));
    context.pass().unwrap();
    assert_eq!(where_is(&context, &note), ("delivered".to_owned(), 1));
    assert_eq!(pasted.get(), 1, "the window was given it once");
}

/// The lines the daemon's log was given that `words` begin.
fn logged(context: &Context, words: &str) -> Vec<String> {
    context
        .log
        .warnings
        .borrow()
        .iter()
        .filter(|line| line.starts_with(words))
        .cloned()
        .collect()
}

#[test]
fn says_in_the_log_at_most_once_a_minute_which_window_could_not_be_read_for_which_message() {
    let context = Context::new();
    let (_, note) = on_its_way(&context);
    // Every look for three minutes, one every 20 s, cannot read the record.
    context.adapter.with("chief", |agent| {
        agent.unreadable = Some(UNREADABLE.to_owned());
    });
    context.pass().unwrap();
    for _ in 0..9 {
        context.advance(20_000);
        context.pass().unwrap();
    }
    let id = note.id;
    let line = |reads: u32, secs: u32| {
        format!("the window p1-chief could not be read (its record could not be read): m-{id} waits, its rest begun again; {reads} reads of it came to nothing in {secs} s")
    };
    assert_eq!(
        logged(&context, "the window p1-chief could not be read"),
        [line(1, 0), line(4, 60), line(7, 120), line(10, 180)]
    );
    // And the pane host's silence is said in its own words.
    context
        .adapter
        .with("chief", |agent| agent.unreadable = None);
    context
        .host
        .set_snapshot(json!({ "ok": false, "error": "deadline" }));
    for _ in 0..2 {
        context.advance(ARRIVAL_MS + 1_000);
        context.pass().unwrap();
    }
    assert_eq!(
        logged(&context, "the window p1-chief could not be read").last(),
        Some(&format!("the window p1-chief could not be read (its pane host did not answer in time): m-{id} waits, its rest begun again; 11 reads of it came to nothing in 242 s"))
    );
}

#[test]
fn says_in_the_log_which_window_a_pass_went_on_without_and_how_long_its_step_took() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let (zeus, diana) = (tiers.assignee(1), tiers.assignee(2));
    // Each hold is for one look of the window's: a hold let go picks its calls still.
    let held = |handle: &str| {
        let launch = context.adapter.agent(handle).launch;
        context.adapter.observe_holds.hold(looks_at(launch))
    };
    let zeus_look = held("zeus");
    context.pass().unwrap();
    assert_eq!(
        logged(&context, "a pass went on"),
        [format!(
            "a pass went on after 5 s without the steps of @{zeus}"
        )]
    );
    // A pass that goes on without another step within the minute says nothing.
    context.advance(20_000);
    let chief_look = held("chief");
    context.pass().unwrap();
    assert_eq!(logged(&context, "a pass went on").len(), 1);
    zeus_look.open();
    context.settle();
    assert_eq!(
        logged(&context, "the step of"),
        [format!(
            "the step of @{zeus} ended 20 s after its pass began"
        )]
    );
    // A minute on, the next says it, and how many it did not.
    context.advance(41_000);
    let diana_look = held("diana");
    context.pass().unwrap();
    assert_eq!(
        logged(&context, "a pass went on").last(),
        Some(&format!(
            "a pass went on after 5 s without the steps of @{diana} (1 passes more went on without some since the last line)"
        ))
    );
    chief_look.open();
    diana_look.open();
    context.settle();
}

#[test]
fn writes_down_every_step_that_fails_after_its_pass_ended_at_the_first() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    // zeus's look waits; the chief's step fails, and the pass with it.
    let launch = context.adapter.agent("zeus").launch;
    let release = context
        .adapter
        .observe_holds
        .hold_instead(looks_at(launch), "the record could not be read".to_owned());
    let failing = Rc::new(Cell::new(true));
    let fails = Rc::clone(&failing);
    context.ledger.borrow_mut().watch(Box::new(move |made, _| {
        if made == "next_delivery" && fails.replace(false) {
            return Err(cf_ledger::LedgerError::refused(
                "ledger-failed",
                "the ledger could not be read",
            ));
        }
        Ok(())
    }));
    assert!(context.pass().is_err(), "the chief's step failed it");
    assert!(!failing.get());
    // zeus's look fails after the pass ended, and its step fails too: the trace of
    // what its window does now cannot be told.
    let launch_failing = Rc::new(Cell::new(true));
    let fails = Rc::clone(&launch_failing);
    context.ledger.borrow_mut().watch(Box::new(move |made, _| {
        if made == "projects" && fails.replace(false) {
            return Err(cf_ledger::LedgerError::refused(
                "ledger-failed",
                "the ledger could not be read again",
            ));
        }
        Ok(())
    }));
    release.open();
    context.settle();
    assert!(
        context
            .log
            .failures
            .borrow()
            .iter()
            .any(|cause| cause == "the ledger could not be read again"),
        "{:?}",
        context.log.failures.borrow()
    );
}

/// The results of T-`number` its requester was given or is to be given.
fn results_of(tiers: &Tiers<'_>, number: i64) -> Vec<String> {
    tiers
        .task(number)
        .messages
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

/// T-1 given to zeus, at work in its window: zeus asked the chief which format, and
/// its turn ended on the question. The chief's answer is on its way into zeus's
/// window, which its record does not show yet: the project's id and the answer.
fn answer_on_its_way(context: &Context) -> (Tiers<'_>, MessageView) {
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    let project = tiers.project.id;
    context.give(project, "zeus", "Record the run");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let question = context.ask(project, "zeus", "chief", 1, "Which format?");
    context
        .adapter
        .answer("zeus", "Asked the chief which format to record the run in");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.with("zeus", |agent| agent.arrive = false);
    let answer = context.answer(project, question.id, "JSON");
    context.pass().unwrap();
    assert_eq!(context.message(answer.id).state, "delivering");
    (tiers, answer)
}

#[test]
fn an_answer_the_pane_host_held_goes_in_once_and_the_task_s_result_is_what_answers_it() {
    // Turn-end meets the retype rule: while the host holds the answer, zeus's record
    // reads at rest with the turn that asked ended in complete words, and nothing
    // left running. The task waits for its answer until a record shows it, so those
    // words are no result, and the answer is never pasted again behind the one the
    // host held.
    let context = Context::new();
    let (tiers, answer) = answer_on_its_way(&context);
    context
        .host
        .set_snapshot(json!({ "ok": false, "error": "deadline" }));
    context.advance(11_000);
    context.pass().unwrap();
    for _ in 0..4 {
        context.advance(ARRIVAL_MS);
        context.pass().unwrap();
    }
    assert_eq!(where_is(&context, &answer), ("delivering".to_owned(), 1));
    assert_eq!(results_of(&tiers, 1), Vec::<String>::new());
    assert_eq!(
        tiers.task(1).task.state,
        "waiting",
        "waiting for the answer, which nothing showed it had"
    );

    // The host comes back and writes the paste it held only as the window is read
    // once more; the record shows it a look later, and zeus answers it.
    context.host.set_snapshot(json!({ "ok": true }));
    context.advance(ARRIVAL_MS + 1_000);
    let (host, once) = (Rc::downgrade(&context.host), Rc::new(Cell::new(true)));
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op: &str, _: &Value| {
        if let (true, Some(host)) = (op == "pane.snapshot" && once.replace(false), host.upgrade()) {
            host.set_snapshot(json!({ "ok": true, "pasteQuietMs": 0 }));
        }
    }));
    context.pass().unwrap();
    assert_eq!(where_is(&context, &answer), ("delivering".to_owned(), 1));
    assert_eq!(results_of(&tiers, 1), Vec::<String>::new());
    assert_eq!(tiers.task(1).task.state, "waiting");
    shows(&context, "zeus", &context.message(answer.id));
    context.adapter.busy("zeus");
    context.pass().unwrap();
    assert_eq!(where_is(&context, &answer), ("delivered".to_owned(), 1));
    context.adapter.answer("zeus", "Recorded the run as JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(results_of(&tiers, 1), ["Recorded the run as JSON"]);
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(where_is(&context, &answer), ("delivered".to_owned(), 1));
}

#[test]
fn an_answer_its_record_shows_as_the_window_is_read_once_more_is_what_the_result_follows() {
    // The receipt the look once more confirms with names the item turn-end finds the
    // answer to the task's latest message after (`delivered_at`).
    let context = Context::new();
    let (tiers, answer) = answer_on_its_way(&context);
    context.advance(ARRIVAL_MS + 1_000);
    let (adapter, header) = (
        Rc::downgrade(&context.adapter),
        delivery_text(&context.message(answer.id), &[]),
    );
    *context.host.on_request.borrow_mut() = Some(Rc::new(move |op: &str, _: &Value| {
        if let (true, Some(adapter)) = (op == "pane.snapshot", adapter.upgrade()) {
            let shown = adapter.with("zeus", |agent| {
                agent.items.iter().any(|item| *item.text == *header)
            });
            if !shown {
                let item = adapter.item(Role::User, &header);
                adapter.with("zeus", |agent| {
                    agent.items.push(item);
                    agent.settled = false;
                });
            }
        }
    }));
    context.pass().unwrap();
    assert_eq!(where_is(&context, &answer), ("delivered".to_owned(), 1));
    let receipt = context.message(answer.id).receipt;
    let item = context.adapter.with("zeus", |agent| {
        agent
            .items
            .iter()
            .rfind(|item| item.role == Role::User)
            .map(|item| item.id.to_string())
    });
    assert_eq!(receipt["item"].as_str(), item.as_deref(), "{receipt}");
    assert_eq!(results_of(&tiers, 1), Vec::<String>::new());
    *context.host.on_request.borrow_mut() = None;
    context.adapter.answer("zeus", "Recorded the run as JSON");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(results_of(&tiers, 1), ["Recorded the run as JSON"]);
    assert_eq!(where_is(&context, &answer), ("delivered".to_owned(), 1));
}
