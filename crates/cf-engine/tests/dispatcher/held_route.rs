//! The probe of T-2106 (its ten cases) on the build: a task held for a retry at a failed end goes
//! through main's own held-task route (`hold_task`, `resume_held`, the daemon's Resume words,
//! `went_on_by_itself`, the window's exit, a restart) and nothing else. Each case asserts the
//! design's oracle; the hold is made by the engine's own step at the failed end.
#![allow(clippy::expect_used)]
use super::fixtures::{given, now, Tiers};
use cf_engine::testing::{Context, Made};
use cf_harness::records::Role;
use serde_json::json;

const RESUMED: &str = "Resumed: Go on where you stopped.";

fn run(pace: i64) -> Context {
    Context::made(Made { settle_ms: pace, turn_retry_ms: 30_000, ..Made::default() })
}
fn passes(c: &Context, count: usize) {
    for _ in 0..count { c.pass().unwrap(); }
}
fn working(c: &Context) -> (Tiers<'_>, String) {
    let t = Tiers::new(c, &["zeus"]);
    t.open();
    for _ in 0..8 {
        c.pass().unwrap();
        if t.task(1).task.state == "working" { break; }
        c.advance(4_000);
    }
    assert_eq!(t.task(1).task.state, "working");
    let window = t.assignee(1);
    (t, window)
}
fn fail(c: &Context, window: &str) {
    c.adapter.with(window, |agent| {
        agent.failed = true;
        agent.settled = true;
        agent.failure = Some("connection reset".into());
    });
}
fn clear_failed(c: &Context, w: &str) {
    c.adapter.with(w, |a| { a.failed = false; a.failure = None; a.quota = None; });
}
/// A failed end: the engine's own step holds the task for 30 seconds at the look that reads it.
fn fail_and_hold(c: &Context, t: &Tiers<'_>, w: &str, ms: i64) {
    assert_eq!(ms, 30_000, "the first try waits 30 seconds");
    fail(c, w);
    c.pass().unwrap();
    assert_eq!(t.task(1).task.state, "paused", "held at the failed end");
}
fn count(c: &Context, window: &str, words: &str) -> usize {
    given(c, window).iter().filter(|text| text.contains(words)).count()
}
fn results(t: &Tiers<'_>) -> Vec<String> {
    t.task(1).messages.into_iter().filter(|m| m.kind == "result").map(|m| m.body).collect()
}
fn show(c: &Context, t: &Tiers<'_>, label: &str) {
    let thread = t.task(1);
    let w = t.assignee(1);
    println!("{label} at {}: task={} hold={:?} messages={:?} user_items={:?}",
        now(c), thread.task.state, thread.task.held_until,
        thread.messages.iter().map(|m| (&m.kind, &m.state, &m.body, m.attempts)).collect::<Vec<_>>(),
        given(c, &w));
}
/// Passes until the daemon's Resume is in the window, the reader's contract kept: the failed end
/// is out of view once a later paste is in the record.
fn until_resumed(c: &Context, w: &str, most: usize) -> bool {
    for _ in 0..most {
        c.pass().unwrap();
        if count(c, w, RESUMED) > 0 {
            clear_failed(c, w);
            c.adapter.with(w, |a| a.settled = false);
            return true;
        }
        c.advance(1_000);
    }
    false
}

#[test]
fn p1_a_held_failed_turn_is_resumed_once_at_its_time_and_gives_its_result() {
    let c = run(0);
    let (t, w) = working(&c);
    fail_and_hold(&c, &t, &w, 30_000);
    passes(&c, 3);
    show(&c, &t, "p1 held, before its time");
    assert_eq!(t.task(1).task.state, "paused");
    assert_eq!(count(&c, &w, RESUMED), 0);
    c.advance(29_000);
    passes(&c, 2);
    assert_eq!(count(&c, &w, RESUMED), 0, "not before its time");
    c.advance(1_001);
    assert!(until_resumed(&c, &w, 10), "resumed at its time");
    passes(&c, 2);
    show(&c, &t, "p1 resumed, its turn at work");
    assert_eq!(t.task(1).task.state, "working");
    c.adapter.answer(&w, "Parser written.");
    passes(&c, 4);
    show(&c, &t, "p1 end");
    assert_eq!(results(&t), vec!["Parser written."]);
    assert_eq!(count(&c, &w, RESUMED), 1);
    let noise: Vec<_> = t.task(1).messages.into_iter()
        .filter(|m| m.kind == "note" && m.body.contains("failed while")).map(|m| m.body).collect();
    println!("p1 notes of a failed turn while held: {noise:?}");
    assert!(noise.is_empty(), "nothing more is told of the failed end while its task is held for it");
}

#[test]
fn p2_a_question_answered_during_the_hold_arrives_once() {
    let c = run(0);
    let (t, w) = working(&c);
    fail_and_hold(&c, &t, &w, 30_000);
    passes(&c, 2);
    let q = c.ask(t.project.id, &w, "chief", 1, "Which destination?");
    show(&c, &t, "p2 asked while held");
    let who = c.id(t.project.id, "chief");
    let a = c.ledger.borrow_mut().answer(q.id, who, Some(&json!("Use the west destination.")), None);
    println!("p2 answer: {:?}", a.as_ref().map(|a| (a.id, &a.state)));
    passes(&c, 2);
    show(&c, &t, "p2 answered while held");
    c.advance(30_001);
    for _ in 0..12 {
        c.pass().unwrap();
        if count(&c, &w, "Use the west destination.") > 0 { clear_failed(&c, &w); break; }
        if count(&c, &w, RESUMED) > 0 { clear_failed(&c, &w); }
        c.advance(1_000);
    }
    passes(&c, 3);
    show(&c, &t, "p2 after its time");
    assert_eq!(count(&c, &w, "Use the west destination."), 1);
    c.adapter.answer(&w, "Finished at the west destination.");
    passes(&c, 6);
    show(&c, &t, "p2 end");
    assert_eq!(results(&t), vec!["Finished at the west destination."]);
    assert_ne!(t.task(1).task.state, "failed");
}

#[test]
fn p3_no_resume_words_go_into_a_task_whose_question_is_open() {
    let c = run(0);
    let (t, w) = working(&c);
    fail_and_hold(&c, &t, &w, 30_000);
    passes(&c, 2);
    let _q = c.ask(t.project.id, &w, "chief", 1, "Which destination?");
    c.advance(30_001);
    for _ in 0..8 { c.pass().unwrap(); c.advance(1_000); }
    show(&c, &t, "p3 past its time, question open");
    assert_eq!(count(&c, &w, RESUMED), 0, "the question is open: nothing speaks into the window");
    assert_eq!(t.task(1).task.state, "waiting");
}

#[test]
fn p4_a_window_that_went_on_by_itself_gives_its_result_and_gets_no_resume() {
    for at_exit in [false, true] {
        let c = run(0);
        let (t, w) = working(&c);
        fail_and_hold(&c, &t, &w, 30_000);
        passes(&c, 2);
        let own = c.adapter.item(Role::User, "Connection restored. Finish the report.");
        c.adapter.with(&w, |a| a.items.push(own));
        clear_failed(&c, &w);
        c.adapter.answer(&w, "Finished once. Receipt 42.");
        if at_exit { c.exit(&w); } else { passes(&c, 4); }
        show(&c, &t, if at_exit { "p4 own turn, then the window's exit" } else { "p4 own turn, then a look" });
        let got = results(&t);
        c.advance(31_000);
        for _ in 0..6 { c.pass().unwrap(); c.advance(1_000); }
        show(&c, &t, if at_exit { "p4 exit, past its time" } else { "p4 look, past its time" });
        println!("p4 at_exit={at_exit}: results at once={got:?}, resumes pasted={}", count(&c, &w, RESUMED));
        assert_eq!(got, vec!["Finished once. Receipt 42."], "at_exit={at_exit}");
        assert_eq!(count(&c, &w, RESUMED), 0, "at_exit={at_exit}");
    }
}

#[test]
fn p5_a_window_that_exits_during_the_hold_is_not_failed() {
    let c = run(0);
    let (t, w) = working(&c);
    fail_and_hold(&c, &t, &w, 30_000);
    passes(&c, 2);
    c.exit(&w);
    show(&c, &t, "p5 exit while held");
    assert_ne!(t.task(1).task.state, "failed");
    c.advance(30_001);
    let resumed = until_resumed(&c, &w, 12);
    passes(&c, 2);
    show(&c, &t, "p5 past its time");
    println!("p5 resumed in a window again={resumed}");
    assert_ne!(t.task(1).task.state, "failed");
}

#[test]
fn p6_a_restart_during_the_hold_fails_nothing() {
    let c = run(0);
    let (t, w) = working(&c);
    fail_and_hold(&c, &t, &w, 30_000);
    passes(&c, 2);
    c.edit_ledger(|_| {});
    println!("p6 ledger reopened: state={} hold={:?}", t.task(1).task.state, t.task(1).task.held_until);
    c.ledger.borrow_mut().suspend_for_restart().unwrap();
    let d = c.make();
    d.settle_after_restart().unwrap();
    d.resume_project(t.project.id).unwrap();
    show(&c, &t, "p6 restarted, project resumed, before its time");
    c.advance(30_001);
    for _ in 0..10 {
        d.pass().unwrap();
        if count(&c, &w, RESUMED) > 0 { clear_failed(&c, &w); break; }
        c.advance(2_000);
    }
    show(&c, &t, "p6 past its time");
    println!("p6 window open={} resumes={}", d.pane(t.id(&w)).is_some(), count(&c, &w, RESUMED));
    assert_ne!(t.task(1).task.state, "failed");
}

#[test]
fn p7_a_laptop_that_wakes_late_resumes_once_and_fails_nothing() {
    let c = run(3_000);
    let (t, w) = working(&c);
    fail_and_hold(&c, &t, &w, 30_000);
    passes(&c, 2);
    c.advance(396_000);
    assert!(until_resumed(&c, &w, 12));
    passes(&c, 2);
    show(&c, &t, "p7 woke 6 minutes late");
    assert_eq!(count(&c, &w, RESUMED), 1);
    assert_eq!(t.task(1).task.state, "working");
}

#[test]
fn p8_a_pause_of_the_chiefs_takes_the_hold_over() {
    let c = run(0);
    let (t, w) = working(&c);
    fail_and_hold(&c, &t, &w, 30_000);
    passes(&c, 2);
    let paused = c.ledger.borrow_mut().pause_task(t.project.id, 1, Some("chief"), None);
    println!("p8 chief's pause: {:?}", paused.as_ref().map(|task| (&task.state, &task.held_until)));
    c.advance(31_000);
    for _ in 0..6 { c.pass().unwrap(); c.advance(1_000); }
    show(&c, &t, "p8 past its time after the chief's pause");
    assert_eq!(count(&c, &w, RESUMED), 0);
    assert_eq!(t.task(1).task.state, "paused");
}

#[test]
fn p9_a_failed_end_still_in_view_at_the_resumes_receipt() {
    // What main does where a reader keeps the earlier failed end in view after a later paste:
    // the reader's contract the design stands on, as a reopened task does today.
    let c = run(0);
    let (t, w) = working(&c);
    fail_and_hold(&c, &t, &w, 30_000);
    c.advance(30_001);
    for _ in 0..10 { c.pass().unwrap(); if count(&c, &w, RESUMED) > 0 { break; } c.advance(1_000); }
    passes(&c, 3);
    show(&c, &t, "p9 stale failed end at the receipt");
    // The stand-in reads a window at work once the Resume is pasted: the failed end still in
    // view is a turn in flight, which decides nothing (a reader that took it out of view is
    // the readers' contract, held at every prefix of the recordings).
    let held = t.task(1).task;
    assert_eq!(held.state, "working");
    assert!(held.retry.is_none());
    // Read as over with the same failed end, the task is not failed by it: it is held again as
    // the second try's, or waits for the Resume's receipt.
    c.adapter.with(&w, |a| a.settled = true);
    passes(&c, 3);
    assert_ne!(t.task(1).task.state, "failed");
}

#[test]
fn p10_a_cancel_during_the_hold_ends_it() {
    let c = run(0);
    let (t, w) = working(&c);
    fail_and_hold(&c, &t, &w, 30_000);
    passes(&c, 2);
    let cancelled = c.ledger.borrow_mut().cancel_task(t.project.id, 1, "chief");
    assert!(cancelled.is_ok());
    c.advance(31_000);
    for _ in 0..6 { c.pass().unwrap(); c.advance(1_000); }
    show(&c, &t, "p10 past its time after a cancel");
    assert_eq!(count(&c, &w, RESUMED), 0);
}
