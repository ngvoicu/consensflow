//! A turn that fails is tried again twice before its task fails (D3's N35). A try is a hold of
//! the task and main's own Resume, with nothing pending of its own: where main would fail a
//! working task for a failed end, the task is held for 30 seconds, then for 2 minutes, and the
//! pass that finds the hold due resumes it. The count is read from the message last pasted into
//! the window, never kept. Every case drives the stand-in window through real passes and the
//! ledger's own messages, with the tries on at the daemon's pace.
use super::fixtures::{given, Tiers};
use cf_engine::testing::{Context, Made};
use cf_harness::records::{Level, Quota, Role};

const RESUMED: &str = "Resumed: Go on where you stopped.";

fn retrying() -> Context {
    Context::made(Made {
        turn_retry_ms: 30_000,
        ..Made::default()
    })
}

fn passes(c: &Context, n: usize) {
    for _ in 0..n {
        c.pass().unwrap();
    }
}

/// zeus at work on T-1 in its one window.
fn working(c: &Context) -> (Tiers<'_>, String) {
    let t = Tiers::new(c, &["zeus"]);
    t.open();
    passes(c, 3);
    assert_eq!(t.task(1).task.state, "working");
    let window = t.assignee(1);
    (t, window)
}

fn fail(c: &Context, window: &str) {
    c.adapter.with(window, |agent| {
        agent.failed = true;
        agent.settled = true;
        agent.failure = Some("connection reset".to_owned());
    });
}

/// The times the window was given main's Resume.
fn resumes(c: &Context, handle: &str) -> usize {
    given(c, handle)
        .iter()
        .filter(|text| text.contains(RESUMED))
        .count()
}

/// Passes until the window has been given the Resume `want` times.
fn until_resumed(c: &Context, handle: &str, want: usize) {
    for _ in 0..8 {
        c.pass().unwrap();
        if resumes(c, handle) >= want {
            return;
        }
    }
    panic!("the Resume number {want} never reached @{handle}");
}

/// The harness's next turn opens on the Resume's words: the failed end before it is out of
/// view, as every reader takes it out once a later paste is in the record, and the turn is at
/// work.
fn next_turn_opens(c: &Context, window: &str) {
    c.adapter.with(window, |agent| {
        agent.failed = false;
        agent.failure = None;
        agent.settled = false;
    });
}

fn results_of(t: &Tiers<'_>, number: i64) -> usize {
    t.task(number)
        .messages
        .iter()
        .filter(|message| message.kind == "result")
        .count()
}

fn failed_told(t: &Tiers<'_>) -> Vec<String> {
    t.notes("chief")
        .into_iter()
        .filter(|note| note.contains("failed"))
        .collect()
}

#[test]
fn a_failed_end_holds_the_task_for_30_seconds_and_main_s_resume_tries_it() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    let held = t.task(1).task;
    assert_eq!(held.state, "paused", "a task waiting to be tried reads paused");
    assert!(held.held_until.is_some(), "with the time it goes on");
    let retry = held.retry.expect("the card says it tries again");
    assert_eq!((retry.which, retry.of), (1, 2));
    assert_eq!(t.out_until("zeus"), None, "no member is held");
    assert!(held.aside.is_some_and(|aside| aside.contains("tries it again")));
    c.advance(29_000);
    passes(&c, 3);
    assert_eq!(resumes(&c, "zeus"), 0, "not before its time");
    c.advance(2_000);
    until_resumed(&c, "zeus", 1);
    // The failed end is still in view at the look after the Resume's paste: nothing is told.
    assert!(
        failed_told(&t).iter().all(|note| note.contains("tries it again")),
        "the chief hears of the first try only: {:?}",
        failed_told(&t)
    );
    next_turn_opens(&c, &window);
    passes(&c, 2);
    assert_eq!(t.task(1).task.state, "working");
    assert!(t.task(1).task.retry.is_none(), "received: nothing waits");
    c.adapter.answer("zeus", "Parser done");
    passes(&c, 3);
    assert_eq!(t.task(1).task.state, "done");
    assert_eq!(results_of(&t, 1), 1, "one result, nothing failed");
}

#[test]
fn a_failed_end_in_view_while_the_task_is_held_is_told_to_nobody() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 6);
    let told: Vec<String> = t
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("failed while"))
        .collect();
    assert!(told.is_empty(), "a held task's failed end is told again: {told:?}");
    drop(window);
}

#[test]
fn the_second_failed_end_holds_two_minutes_from_its_look_and_the_third_fails_the_task() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    until_resumed(&c, "zeus", 1);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    fail(&c, &window);
    passes(&c, 3);
    let held = t.task(1).task;
    assert_eq!(held.state, "paused");
    assert_eq!(held.retry.expect("tries again").which, 2);
    c.advance(100_000);
    passes(&c, 3);
    assert_eq!(resumes(&c, "zeus"), 1, "not before 2 minutes");
    c.advance(21_000);
    until_resumed(&c, "zeus", 2);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    fail(&c, &window);
    passes(&c, 3);
    let last = t.task(1).task;
    assert_eq!(last.state, "failed");
    assert_eq!(t.out_until("zeus"), None, "its member stays available");
    let said: Vec<String> = t
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("tried again twice"))
        .collect();
    assert_eq!(said.len(), 1, "the failure says what was tried: {said:?}");
    let tries: Vec<String> = t
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("tries it again in its window"))
        .collect();
    assert!(tries.len() <= 1, "no second note: {tries:?}");
    assert_eq!(resumes(&c, "zeus"), 2, "never a third try");
}

#[test]
fn nothing_reads_why_the_turn_failed() {
    for why in ["other", "unknown", "model_not_found", "sandbox_error"] {
        let c = retrying();
        let (t, window) = working(&c);
        c.adapter.with(&window, |agent| {
            agent.failed = true;
            agent.settled = true;
            agent.failure = Some(why.to_owned());
        });
        passes(&c, 3);
        assert_eq!(t.task(1).task.state, "paused", "{why}");
        c.advance(31_000);
        until_resumed(&c, "zeus", 1);
    }
}

#[test]
fn the_chiefs_failed_turn_a_waiting_task_and_a_shared_record_are_not_tried() {
    let c = retrying();
    let t = Tiers::new(&c, &["zeus"]);
    passes(&c, 2);
    fail(&c, "chief");
    passes(&c, 3);
    c.advance(200_000);
    passes(&c, 3);
    assert_eq!(resumes(&c, "chief"), 0, "the chief's turn is not tried");
    drop(t);

    let c = retrying();
    let (t, window) = working(&c);
    c.ask(t.project.id, &window, "chief", 1, "Which destination?");
    assert_eq!(t.task(1).task.state, "waiting");
    fail(&c, &window);
    passes(&c, 3);
    c.advance(200_000);
    passes(&c, 3);
    assert_eq!(resumes(&c, "zeus"), 0, "a waiting task's failed turn is told");
    assert_eq!(t.task(1).task.state, "waiting");

    let c = retrying();
    let (t, window) = working(&c);
    c.adapter.with(&window, |agent| agent.shared = true);
    fail(&c, &window);
    passes(&c, 3);
    assert!(t.task(1).task.retry.is_none(), "a shared record is told");
    c.advance(31_000);
    passes(&c, 3);
    assert_eq!(resumes(&c, "zeus"), 0);
}

#[test]
fn a_failed_end_the_harness_still_works_on_decides_nothing_until_it_settles() {
    let c = retrying();
    let (t, window) = working(&c);
    c.adapter.with(&window, |agent| {
        agent.failed = true;
        agent.settled = false;
        agent.failure = Some("stream disconnected".to_owned());
    });
    passes(&c, 4);
    c.advance(60_000);
    passes(&c, 3);
    assert_eq!(t.task(1).task.state, "working", "nothing fails while the child runs");
    assert!(t.task(1).task.retry.is_none(), "and nothing is tried");
    c.adapter.with(&window, |agent| agent.settled = true);
    passes(&c, 3);
    assert!(t.task(1).task.retry.is_some(), "the settled end is the first try's");
}

#[test]
fn two_user_items_the_harness_wrote_spend_no_try_and_paste_nothing_before_the_time() {
    // Codex writes its environment note and a sub-agent's notice as user items after a turn.
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    let notes = [
        c.adapter.item(Role::User, "<environment_context>cwd</environment_context>"),
        c.adapter.item(Role::User, "<subagent_notification>done</subagent_notification>"),
    ];
    c.adapter.with(&window, |agent| agent.items.extend(notes));
    c.advance(20_000);
    passes(&c, 4);
    assert_eq!(resumes(&c, "zeus"), 0, "nothing is pasted before the time");
    assert_eq!(t.task(1).task.state, "paused");
    c.advance(11_000);
    passes(&c, 6);
    assert_eq!(resumes(&c, "zeus"), 1, "one Resume at the time");
}

#[test]
fn a_person_s_resume_pasted_since_gives_the_window_two_tries_again() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    until_resumed(&c, "zeus", 1);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    fail(&c, &window);
    passes(&c, 3);
    assert_eq!(t.task(1).task.retry.expect("held").which, 2);
    // The chief resumes it with its own words: they are the window's last paste.
    c.resume_task(t.project.id, 1, "go on with the plan");
    for _ in 0..8 {
        passes(&c, 1);
        if given(&c, "zeus").iter().any(|text| text.contains("go on with the plan")) {
            break;
        }
    }
    next_turn_opens(&c, &window);
    passes(&c, 2);
    fail(&c, &window);
    passes(&c, 3);
    let held = t.task(1).task;
    assert_eq!(held.state, "paused", "a failed turn after it has two tries again");
    assert_eq!(held.retry.expect("held").which, 1);
}

#[test]
fn a_dispatcher_that_wakes_six_minutes_late_resumes_once_and_fails_nothing() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(360_000);
    until_resumed(&c, "zeus", 1);
    assert_ne!(t.task(1).task.state, "failed");
    assert_eq!(t.out_until("zeus"), None);
    assert_eq!(resumes(&c, "zeus"), 1);
}

#[test]
fn a_turn_the_window_got_through_by_itself_during_the_hold_is_the_result_with_no_resume() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(5_000);
    let typed = c.adapter.item(Role::User, "The connection is back. Finish the report.");
    c.adapter.with(&window, |agent| {
        agent.items.push(typed);
        agent.failed = false;
        agent.failure = None;
    });
    c.adapter.answer("zeus", "Receipt 42, the requested action is complete.");
    passes(&c, 3);
    c.advance(40_000);
    passes(&c, 3);
    assert_eq!(t.task(1).task.state, "done");
    assert_eq!(results_of(&t, 1), 1, "one result");
    assert_eq!(resumes(&c, "zeus"), 0, "no Resume");
}

#[test]
fn a_question_open_at_the_holds_time_gets_no_words_and_its_answer_is_what_goes_in() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    let question = c.ask(t.project.id, &window, "chief", 1, "Which destination?");
    c.advance(31_000);
    passes(&c, 5);
    assert_eq!(resumes(&c, "zeus"), 0, "no words go into a window that waits");
    assert_eq!(t.task(1).task.state, "waiting");
    c.answer(t.project.id, question.id, "Use staging");
    passes(&c, 6);
    let answers = given(&c, "zeus")
        .iter()
        .filter(|text| text.contains("Use staging"))
        .count();
    assert_eq!(answers, 1, "the late answer is delivered once");
    assert_eq!(resumes(&c, "zeus"), 0);
}

#[test]
fn a_question_answered_during_the_hold_rides_in_the_resume_once() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    let question = c.ask(t.project.id, &window, "chief", 1, "Which destination?");
    c.answer(t.project.id, question.id, "Use staging");
    c.advance(31_000);
    passes(&c, 8);
    let answers = given(&c, "zeus")
        .iter()
        .filter(|text| text.contains("Use staging"))
        .count();
    assert_eq!(answers, 1, "the answer once");
    assert_eq!(resumes(&c, "zeus"), 1, "in the Resume");
}

#[test]
fn a_restart_keeps_the_hold_and_asks_for_no_task_resume() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.edit_ledger(|_| {});
    c.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = c.make();
    restarted.settle_after_restart().unwrap();
    restarted.resume_project(t.project.id).unwrap();
    let held = t.task(1).task;
    assert_eq!(held.state, "paused");
    assert!(held.held_until.is_some(), "still held: nothing was paused for the restart");
    c.advance(31_000);
    for _ in 0..8 {
        restarted.pass().unwrap();
        if resumes(&c, "zeus") > 0 {
            break;
        }
    }
    assert_eq!(resumes(&c, "zeus"), 1, "one Resume at its time");
}

#[test]
fn a_window_that_exits_while_its_task_waits_is_opened_again_at_the_time() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.exit("zeus");
    let held = t.task(1).task;
    assert_eq!(held.state, "paused", "not failed by the exit");
    assert!(held.held_until.is_some());
    assert!(t.task(1).messages.iter().all(|m| m.kind != "result"));
    c.advance(31_000);
    passes(&c, 8);
    let on = t.task(1).task;
    assert!(
        matches!(on.state.as_str(), "queued" | "working"),
        "resumed at its time: {}",
        on.state
    );
}

#[test]
fn the_chiefs_pause_or_cancel_takes_the_hold_over() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.pause_task(t.project.id, 1);
    assert_eq!(t.task(1).task.held_until, None, "paused with no time");
    c.advance(200_000);
    passes(&c, 4);
    assert_eq!(resumes(&c, "zeus"), 0, "only their Resume goes on");

    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.ledger.borrow_mut().cancel_task(t.project.id, 1, "chief").unwrap();
    c.advance(40_000);
    passes(&c, 4);
    assert_eq!(t.task(1).task.state, "cancelled");
    assert_eq!(resumes(&c, "zeus"), 0, "no Resume after a cancel");
    assert!(t.task(1).task.retry.is_none());
}

#[test]
fn the_windows_of_one_member_that_fail_in_one_pass_are_each_tried() {
    let c = retrying();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    c.give(t.project.id, "zeus", "Second");
    passes(&c, 8);
    let windows = [t.assignee(1), t.assignee(2)];
    for window in &windows {
        fail(&c, window);
    }
    passes(&c, 3);
    for number in 1..=2 {
        assert_eq!(t.task(number).task.state, "paused", "T-{number} is held");
    }
    c.advance(31_000);
    passes(&c, 8);
    let given: usize = windows.iter().map(|w| resumes(&c, w)).sum();
    assert_eq!(given, 2, "each window was tried again");
}

fn count_at_its_limit(c: &Context) -> Quota {
    Quota::Usage {
        level: Level::Exhausted,
        used_percent: Some(100.into()),
        resets_at: Some(super::fixtures::soon(c, 5)),
    }
}

#[test]
fn a_proven_answer_is_collected_before_the_hold_at_a_count_that_reached_its_limit() {
    let c = retrying();
    let (t, window) = working(&c);
    c.adapter.answer("zeus", "Parser done");
    let quota = count_at_its_limit(&c);
    c.adapter.quota(&window, Some(quota));
    passes(&c, 3);
    assert_eq!(t.task(1).task.state, "done", "the answer was collected");
    assert_eq!(results_of(&t, 1), 1);
    assert!(t.out_until("zeus").is_some(), "then main's hold");
}

#[test]
fn a_proven_answer_is_collected_at_an_exit_that_first_sees_a_count_at_its_limit() {
    let c = retrying();
    let (t, window) = working(&c);
    c.adapter.answer("zeus", "Parser done");
    let quota = count_at_its_limit(&c);
    c.adapter.quota(&window, Some(quota));
    c.exit("zeus");
    assert_eq!(t.task(1).task.state, "done");
    assert_eq!(results_of(&t, 1), 1);
}

#[test]
fn a_finished_turn_first_seen_at_the_exit_of_a_held_task_gives_its_result() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    let typed = c.adapter.item(Role::User, "The connection is back. Finish the report.");
    c.adapter.with(&window, |agent| {
        agent.items.push(typed);
        agent.failed = false;
        agent.failure = None;
    });
    c.adapter.answer("zeus", "Receipt 42, the requested action is complete.");
    c.exit("zeus");
    assert_eq!(t.task(1).task.state, "done", "the exit read the window that went on");
    assert_eq!(results_of(&t, 1), 1);
}

#[test]
fn a_brief_s_first_receipt_and_its_answer_first_seen_at_the_exit_give_the_result() {
    let c = retrying();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    c.pass().unwrap();
    let window = t.assignee(1);
    assert_eq!(t.task(1).task.state, "queued");
    c.adapter.answer("zeus", "The completed result.");
    c.exit("zeus");
    let _ = window;
    assert_eq!(t.task(1).task.state, "done");
    assert_eq!(results_of(&t, 1), 1);
}

// ---- the renewal's words and a question open (every hold) ----

#[test]
fn a_quota_renewal_puts_no_words_into_a_window_whose_question_is_open() {
    let c = retrying();
    let (t, window) = working(&c);
    // The window's turn was cut short and rests; the daemon holds the task for its member's
    // quota, to a time that is already come.
    c.adapter.with(&window, |agent| agent.settled = true);
    c.ledger
        .borrow_mut()
        .hold_task(t.project.id, 1, &super::fixtures::now(&c), "out of quota")
        .unwrap();
    let question = c.ask(t.project.id, &window, "chief", 1, "Which destination?");
    passes(&c, 5);
    assert_eq!(resumes(&c, "zeus"), 0, "no words go in");
    assert_eq!(t.task(1).task.state, "waiting");
    c.advance(60_000);
    c.answer(t.project.id, question.id, "Use staging");
    for _ in 0..6 {
        c.advance(4_000);
        passes(&c, 1);
    }
    let answers = given(&c, "zeus")
        .iter()
        .filter(|text| text.contains("Use staging"))
        .count();
    assert_eq!(answers, 1, "the late answer is delivered once");
    assert_eq!(resumes(&c, "zeus"), 0);
}

#[test]
fn a_question_opened_after_the_resume_was_queued_turns_its_bare_words_away_at_the_hand_over() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    // The Resume is queued; the window is at work, so it waits for the window's rest.
    c.adapter.with(&window, |agent| agent.settled = false);
    passes(&c, 2);
    let queued = t
        .task(1)
        .messages
        .iter()
        .any(|m| m.body == RESUMED && m.state == "queued");
    assert!(queued, "main's Resume waits for the window");
    c.ask(t.project.id, &window, "chief", 1, "Which destination?");
    c.adapter.with(&window, |agent| agent.settled = true);
    passes(&c, 5);
    assert_eq!(resumes(&c, "zeus"), 0, "nothing is pasted into a window that waits");
    assert_eq!(t.task(1).task.state, "waiting");
}
