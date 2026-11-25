//! A turn that fails is tried again twice before its task fails (D3's N35). A try is a hold of
//! the task and main's own Resume, with nothing pending of its own: where main would fail a
//! working task for a failed end, the task is held for 30 seconds, then for 2 minutes, and the
//! pass that finds the hold due resumes it. The count is a number stored on the task (the
//! ledger raises it with the numbered Resume and resets it on an action, a result or 30 minutes). Every case drives the stand-in window through real passes and the
//! ledger's own messages, with the tries on at the daemon's pace.
use super::fixtures::{given, Tiers};
use cf_engine::testing::{Context, Made};
use cf_harness::records::{Level, Quota, Role};
use cf_harness::seams::Time;

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
    assert_eq!(
        held.state, "paused",
        "a task waiting to be tried reads paused"
    );
    assert!(held.held_until.is_some(), "with the time it goes on");
    let retry = held.retry.expect("the card says it tries again");
    assert_eq!((retry.which, retry.of), (1, 2));
    assert_eq!(t.out_until("zeus"), None, "no member is held");
    assert!(held
        .aside
        .is_some_and(|aside| aside.contains("tries it again")));
    c.advance(29_000);
    passes(&c, 3);
    assert_eq!(resumes(&c, "zeus"), 0, "not before its time");
    c.advance(2_000);
    until_resumed(&c, "zeus", 1);
    // The failed end is still in view at the look after the Resume's paste: nothing is told.
    assert!(
        failed_told(&t)
            .iter()
            .all(|note| note.contains("tries it again")),
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
    assert!(
        told.is_empty(),
        "a held task's failed end is told again: {told:?}"
    );
    drop(window);
}

#[test]
fn the_second_failed_end_in_view_while_its_task_is_held_is_told_to_nobody() {
    // The first try's note is no note of the second: nothing else keeps the failed end from
    // being told as one that came while its task was paused.
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    until_resumed(&c, "zeus", 1);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    fail(&c, &window);
    passes(&c, 6);
    assert_eq!(t.task(1).task.state, "paused");
    let told: Vec<String> = t
        .notes("chief")
        .into_iter()
        .filter(|note| note.contains("failed while"))
        .collect();
    assert!(told.is_empty(), "told again while held: {told:?}");
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
    assert!(
        said[0].contains("connection reset"),
        "and why the last turn failed: {said:?}"
    );
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
    assert_eq!(
        resumes(&c, "zeus"),
        0,
        "a waiting task's failed turn is told"
    );
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
    assert_eq!(
        t.task(1).task.state,
        "working",
        "nothing fails while the child runs"
    );
    assert!(t.task(1).task.retry.is_none(), "and nothing is tried");
    c.adapter.with(&window, |agent| agent.settled = true);
    passes(&c, 3);
    assert!(
        t.task(1).task.retry.is_some(),
        "the settled end is the first try's"
    );
}

#[test]
fn two_user_items_the_harness_wrote_spend_no_try_and_paste_nothing_before_the_time() {
    // Codex writes its environment note and a sub-agent's notice as user items after a turn.
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    let notes = [
        c.adapter
            .item(Role::User, "<environment_context>cwd</environment_context>"),
        c.adapter.item(
            Role::User,
            "<subagent_notification>done</subagent_notification>",
        ),
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
        if given(&c, "zeus")
            .iter()
            .any(|text| text.contains("go on with the plan"))
        {
            break;
        }
    }
    next_turn_opens(&c, &window);
    passes(&c, 2);
    fail(&c, &window);
    passes(&c, 3);
    let held = t.task(1).task;
    assert_eq!(
        held.state, "paused",
        "a failed turn after it has two tries again"
    );
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
    let typed = c
        .adapter
        .item(Role::User, "The connection is back. Finish the report.");
    c.adapter.with(&window, |agent| {
        agent.items.push(typed);
        agent.failed = false;
        agent.failure = None;
    });
    c.adapter
        .answer("zeus", "Receipt 42, the requested action is complete.");
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
    assert_eq!(
        resumes(&c, "zeus"),
        0,
        "no words go into a window that waits"
    );
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
    assert!(
        held.held_until.is_some(),
        "still held: nothing was paused for the restart"
    );
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
    c.ledger
        .borrow_mut()
        .cancel_task(t.project.id, 1, "chief")
        .unwrap();
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
    let typed = c
        .adapter
        .item(Role::User, "The connection is back. Finish the report.");
    c.adapter.with(&window, |agent| {
        agent.items.push(typed);
        agent.failed = false;
        agent.failure = None;
    });
    c.adapter
        .answer("zeus", "Receipt 42, the requested action is complete.");
    c.exit("zeus");
    assert_eq!(
        t.task(1).task.state,
        "done",
        "the exit read the window that went on"
    );
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
fn the_pass_queues_no_words_for_a_task_whose_window_has_a_question_open() {
    // The window is at work, so nothing could be handed over: only the pass's own write is
    // asked.
    let c = retrying();
    let (t, window) = working(&c);
    c.ledger
        .borrow_mut()
        .hold_task(t.project.id, 1, &super::fixtures::now(&c), "out of quota")
        .unwrap();
    c.ask(t.project.id, &window, "chief", 1, "Which destination?");
    c.adapter.with(&window, |agent| agent.settled = false);
    passes(&c, 2);
    let thread = t.task(1);
    assert_eq!(thread.task.state, "waiting");
    assert!(
        thread.messages.iter().all(|m| m.body != RESUMED),
        "no words were queued: {:?}",
        thread
            .messages
            .iter()
            .map(|m| (&m.body, &m.state))
            .collect::<Vec<_>>()
    );
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
    assert_eq!(
        resumes(&c, "zeus"),
        0,
        "nothing is pasted into a window that waits"
    );
    assert_eq!(t.task(1).task.state, "waiting");
}

// ---- the review cases of T-2075 and T-2103 that stand on the hold ----

#[test]
fn a_completed_answer_on_the_first_receipt_look_of_a_count_is_kept_once() {
    let c = retrying();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    c.pass().unwrap();
    assert_eq!(t.task(1).task.state, "queued");
    c.adapter
        .answer("zeus", "The completed result from the last allowance.");
    let quota = count_at_its_limit(&c);
    c.adapter.quota("zeus", Some(quota));
    passes(&c, 3);
    let end = t.task(1);
    assert_eq!(end.task.state, "done");
    assert_eq!(
        end.messages
            .iter()
            .filter(|m| m.kind == "result" && m.body.contains("completed result"))
            .count(),
        1
    );
}

#[test]
fn a_brief_s_first_receipt_with_a_count_and_its_answer_first_seen_at_the_exit_gives_the_result() {
    let c = retrying();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    c.pass().unwrap();
    assert_eq!(t.task(1).task.state, "queued");
    c.adapter.answer("zeus", "The completed result.");
    let quota = count_at_its_limit(&c);
    c.adapter.quota("zeus", Some(quota));
    c.exit("zeus");
    assert_eq!(t.task(1).task.state, "done");
    assert_eq!(results_of(&t, 1), 1);
}

#[test]
fn a_restart_after_the_resume_was_pasted_never_sends_the_brief_again() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    until_resumed(&c, "zeus", 1);
    c.edit_ledger(|_| {});
    c.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = c.make();
    restarted.settle_after_restart().unwrap();
    restarted.resume_project(t.project.id).unwrap();
    for _ in 0..8 {
        restarted.pass().unwrap();
        c.advance(4_000);
    }
    let briefs: Vec<_> = c
        .adapter
        .agent("zeus")
        .items
        .into_iter()
        .filter(|item| item.role == Role::User && item.text.contains("Write the parser"))
        .map(|item| item.id)
        .collect();
    assert!(
        briefs.len() <= 1,
        "the window was not given the brief again: {briefs:?}"
    );
    let message_briefs: Vec<_> = t
        .task(1)
        .messages
        .iter()
        .filter(|m| m.kind == "task" && m.body != RESUMED)
        .map(|m| (m.attempts, m.state.clone()))
        .collect();
    assert_eq!(
        message_briefs,
        vec![(1, "delivered".to_owned())],
        "one brief, one attempt"
    );
}

#[test]
fn a_question_asked_before_or_during_the_hold_and_answered_before_or_after_its_time_is_answered_once(
) {
    for (before_failure, answer_in_time) in [(true, false), (false, false), (false, true)] {
        let c = retrying();
        let (t, window) = working(&c);
        let early = before_failure.then(|| c.ask(t.project.id, &window, "chief", 1, "Which?"));
        if before_failure {
            assert_eq!(t.task(1).task.state, "waiting");
        }
        fail(&c, &window);
        passes(&c, 3);
        let question = early.unwrap_or_else(|| c.ask(t.project.id, &window, "chief", 1, "Which?"));
        if answer_in_time {
            c.answer(t.project.id, question.id, "The west one");
        }
        c.advance(31_000);
        passes(&c, 5);
        if !answer_in_time {
            c.answer(t.project.id, question.id, "The west one");
        }
        for _ in 0..8 {
            c.advance(4_000);
            passes(&c, 1);
        }
        next_turn_opens(&c, &window);
        c.adapter.answer("zeus", "Done at the west one");
        passes(&c, 4);
        let case = format!("before failure {before_failure}, in time {answer_in_time}");
        let answers = given(&c, "zeus")
            .iter()
            .filter(|text| text.contains("The west one"))
            .count();
        assert_eq!(answers, 1, "one answer delivered ({case})");
        assert_eq!(results_of(&t, 1), 1, "one result ({case})");
        assert_ne!(t.task(1).task.state, "failed", "{case}");
    }
}

#[test]
fn a_question_asked_and_answered_between_two_looks_is_never_failed_by_any_time() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    let question = c.ask(t.project.id, &window, "chief", 1, "Which destination?");
    c.answer(t.project.id, question.id, "Use staging");
    c.advance(31_000);
    for _ in 0..8 {
        c.advance(4_000);
        passes(&c, 1);
    }
    next_turn_opens(&c, &window);
    passes(&c, 2);
    // The turn on the answer goes on for longer than any retry's time: nothing fails it.
    c.advance(900_000);
    passes(&c, 4);
    assert_eq!(t.task(1).task.state, "working");
    c.adapter.answer("zeus", "Done on staging");
    passes(&c, 3);
    assert_eq!(t.task(1).task.state, "done");
    assert_eq!(results_of(&t, 1), 1);
}

#[test]
fn a_resume_that_waits_six_minutes_for_its_window_is_pasted_once_and_fails_nothing() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    // The window is at work: main's Resume is queued for its rest.
    c.adapter.with(&window, |agent| {
        agent.failed = false;
        agent.failure = None;
        agent.settled = false;
    });
    passes(&c, 3);
    assert_eq!(resumes(&c, "zeus"), 0);
    c.advance(400_000);
    passes(&c, 3);
    c.adapter.with(&window, |agent| agent.settled = true);
    passes(&c, 6);
    assert_eq!(resumes(&c, "zeus"), 1, "pasted once");
    assert_ne!(t.task(1).task.state, "failed");
}

#[test]
fn a_refusal_seen_while_the_task_is_held_holds_the_member_and_the_task_is_not_resumed_while_it_is_out(
) {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(1_000);
    let quota = super::fixtures::exhausted(
        Some(&super::fixtures::now(&c)),
        Some(&super::fixtures::soon(&c, 5)),
    );
    c.adapter.refuse(&window, quota);
    passes(&c, 3);
    assert!(t.out_until("zeus").is_some(), "main's hold of the member");
    c.advance(40_000);
    passes(&c, 4);
    assert_eq!(
        resumes(&c, "zeus"),
        0,
        "not resumed while its member is out"
    );
    assert_eq!(t.task(1).task.state, "paused");
}

#[test]
fn a_question_asked_during_the_hold_and_a_restart_leave_the_task_held_and_fail_nothing() {
    // The restart closes the question its old process asked (main's rule), so the Resume goes at
    // its time as for any held task: the restart paused nothing and asked for no task Resume.
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.ask(t.project.id, &window, "chief", 1, "Which destination?");
    c.edit_ledger(|_| {});
    c.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = c.make();
    restarted.settle_after_restart().unwrap();
    restarted.resume_project(t.project.id).unwrap();
    let held = t.task(1).task;
    assert_eq!(
        held.state, "paused",
        "a restart pauses nothing and asks for no Resume"
    );
    assert!(held.held_until.is_some());
    c.advance(31_000);
    for _ in 0..6 {
        restarted.pass().unwrap();
        c.advance(2_000);
    }
    assert_ne!(t.task(1).task.state, "failed");
}

#[test]
fn a_restart_after_the_resume_was_received_and_before_its_answer_never_sends_the_brief_again() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    until_resumed(&c, "zeus", 1);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    assert!(t.task(1).task.retry.is_none(), "received");
    c.edit_ledger(|_| {});
    c.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = c.make();
    restarted.settle_after_restart().unwrap();
    restarted.resume_project(t.project.id).unwrap();
    for _ in 0..8 {
        restarted.pass().unwrap();
        c.advance(4_000);
    }
    let briefs: Vec<_> = c
        .adapter
        .agent("zeus")
        .items
        .into_iter()
        .filter(|item| item.role == Role::User && item.text.contains("Write the parser"))
        .collect();
    assert!(briefs.len() <= 1, "the brief was not sent again");
    let attempts: Vec<_> = t
        .task(1)
        .messages
        .iter()
        .filter(|m| m.kind == "task" && m.body != RESUMED)
        .map(|m| m.attempts)
        .collect();
    assert_eq!(attempts, vec![1]);
}

#[test]
fn a_failed_turn_after_an_answer_has_two_tries_and_twice_is_said_only_after_two_were_received() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    until_resumed(&c, "zeus", 1);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    // The retry's turn asks, and the chief's answer is the window's last paste.
    let question = c.ask(t.project.id, &window, "chief", 1, "Which destination?");
    assert_eq!(t.task(1).task.state, "waiting");
    c.adapter.with(&window, |agent| agent.settled = true);
    c.answer(t.project.id, question.id, "Use staging");
    for _ in 0..6 {
        c.advance(4_000);
        passes(&c, 1);
    }
    assert!(given(&c, "zeus")
        .iter()
        .any(|text| text.contains("Use staging")));
    next_turn_opens(&c, &window);
    passes(&c, 2);
    fail(&c, &window);
    passes(&c, 3);
    let held = t.task(1).task;
    assert_eq!(held.state, "paused", "two tries again after the answer");
    assert_eq!(held.retry.expect("held").which, 1);
    assert!(
        failed_told(&t)
            .iter()
            .all(|note| !note.contains("tried again twice")),
        "twice is said only once two were received"
    );
}

#[test]
fn a_count_at_its_limit_in_a_turn_at_work_is_main_s_hold_at_once() {
    let c = retrying();
    let (t, window) = working(&c);
    let quota = count_at_its_limit(&c);
    c.adapter.refuse_unsettled(&window, quota);
    passes(&c, 3);
    assert!(t.out_until("zeus").is_some(), "held at once, as on main");
    assert!(t.task(1).task.retry.is_none(), "no retry is in it");
}

#[test]
fn with_the_tries_off_a_failed_end_fails_the_task_at_once_as_before() {
    let c = Context::tries_off();
    let t = Tiers::new(&c, &["zeus"]);
    t.open();
    passes(&c, 3);
    let window = t.assignee(1);
    fail(&c, &window);
    passes(&c, 3);
    let task = t.task(1).task;
    assert_eq!(task.state, "failed");
    assert!(task.retry.is_none());
    assert_eq!(resumes(&c, "zeus"), 0);
}

// ---- round 2 (T-2131's two blockers) ----

#[test]
fn an_out_member_s_held_window_that_finished_by_itself_and_exits_stays_held_to_the_reset() {
    // T-2131 blocker 1, at both paces: main leaves a task held for its member's quota held
    // through the exit, and one Resume at the reset takes the window up again.
    for tries in [30_000, 0] {
        let c = Context::made(Made {
            turn_retry_ms: tries,
            ..Made::default()
        });
        let (t, window) = working(&c);
        let quota = super::fixtures::exhausted(
            Some(&super::fixtures::now(&c)),
            Some(&super::fixtures::soon(&c, 1)),
        );
        c.adapter.refuse(&window, quota);
        passes(&c, 3);
        assert!(
            t.out_until("zeus").is_some(),
            "tries {tries}: the member is out"
        );
        let typed = c
            .adapter
            .item(Role::User, "The limit is back. Finish the report.");
        c.adapter.with(&window, |agent| {
            agent.items.push(typed);
            agent.failed = false;
            agent.failure = None;
            agent.quota = None;
        });
        c.adapter
            .answer("zeus", "Receipt 42, the requested action is complete.");
        c.exit("zeus");
        let after = t.task(1).task;
        assert_eq!(after.state, "paused", "tries {tries}");
        assert!(
            after.held_until.is_some(),
            "tries {tries}: still held to the reset, as on main"
        );
        c.advance(3_700_000);
        passes(&c, 8);
        let later = t.task(1).task;
        assert!(
            matches!(later.state.as_str(), "queued" | "working" | "done"),
            "tries {tries}: resumed at the reset, not stranded: {}",
            later.state
        );
    }
}

#[test]
fn a_turn_the_harness_woke_after_turns_that_went_through_is_the_first_try_not_the_next() {
    // T-2131 blocker 2: the retry's turn ended well and left a job; an hour later a turn the
    // harness woke by itself fails. Twice over, the task is never failed at once.
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    until_resumed(&c, "zeus", 1);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    c.adapter
        .answer_leaving_a_job("zeus", "The build runs in the background; I wait for it.");
    passes(&c, 3);
    assert_eq!(t.task(1).task.state, "working");
    c.advance(3_600_000);
    c.adapter.wake(
        "zeus",
        "<task-notification>the build ended</task-notification>",
    );
    fail(&c, &window);
    passes(&c, 3);
    let held = t.task(1).task;
    assert_eq!(held.state, "paused");
    assert_eq!(
        held.retry.expect("tried again").which,
        1,
        "1 of 2, not 2 of 2"
    );
    c.advance(31_000);
    until_resumed(&c, "zeus", 2);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    c.adapter
        .answer_leaving_a_job("zeus", "Building again in the background.");
    passes(&c, 3);
    assert_eq!(t.task(1).task.state, "working");
    c.advance(3_600_000);
    c.adapter.wake(
        "zeus",
        "<task-notification>the build ended</task-notification>",
    );
    fail(&c, &window);
    passes(&c, 3);
    let again = t.task(1).task;
    assert_eq!(again.state, "paused", "tried again, not failed at once");
    assert_eq!(again.retry.expect("tried again").which, 1);
}

#[test]
fn a_failed_turn_straight_after_a_retry_that_failed_is_still_the_second_try() {
    // The mend must not lose the count where nothing went through between the two failures.
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
}

#[test]
fn a_second_try_after_an_earlier_quota_hold_says_no_old_quota_line() {
    // `cf task list` reads the task's newest "waits with @…" note while it is held and has no
    // line of its own: at a retry's second hold that was the old quota hold's (T-2131).
    let c = retrying();
    let (t, window) = working(&c);
    let quota = super::fixtures::exhausted(
        Some(&super::fixtures::now(&c)),
        Some(&super::fixtures::soon(&c, 1)),
    );
    c.adapter.refuse(&window, quota);
    passes(&c, 3);
    c.advance(3_700_000);
    until_resumed(&c, "zeus", 1);
    c.adapter.with(&window, |agent| agent.quota = None);
    next_turn_opens(&c, &window);
    passes(&c, 3);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    until_resumed(&c, "zeus", 2);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    fail(&c, &window);
    passes(&c, 3);
    let second = t.task(1).task;
    assert_eq!(second.retry.expect("held").which, 2);
    assert!(
        second
            .aside
            .as_deref()
            .is_none_or(|aside| !aside.contains("waits with")),
        "no old quota line: {:?}",
        second.aside
    );
}

/// A turn that failed, whatever its record holds of the answer it began: a complete assistant
/// item is inside a turn whose own end is a failure (T-2161: Muse's commentary, Codex's final
/// item, Devin's stored stop).
fn fail_after_an_answer_item(c: &Context, window: &str, text: &str) {
    let item = c.adapter.item(Role::Assistant, text);
    c.adapter.with(window, |agent| {
        agent.items.push(item);
        agent.failed = true;
        agent.settled = true;
        agent.failure = Some("connection reset".to_owned());
    });
}

#[test]
fn an_answer_item_inside_a_turn_that_then_failed_is_never_a_turn_that_went_through() {
    // T-2161 finding 2: three failed turns in a row, each holding a complete assistant item,
    // got a third Resume. Only a turn's own end, as a success, counts.
    let c = retrying();
    let (t, window) = working(&c);
    fail_after_an_answer_item(&c, &window, "Looking at the parser.");
    passes(&c, 3);
    assert_eq!(t.task(1).task.retry.expect("held").which, 1);
    c.advance(31_000);
    until_resumed(&c, "zeus", 1);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    fail_after_an_answer_item(&c, &window, "Looking again.");
    passes(&c, 3);
    assert_eq!(t.task(1).task.retry.expect("held").which, 2);
    c.advance(121_000);
    until_resumed(&c, "zeus", 2);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    fail_after_an_answer_item(&c, &window, "And once more.");
    passes(&c, 3);
    c.advance(3_600_000);
    passes(&c, 8);
    assert_eq!(t.task(1).task.state, "failed", "two tries, then it fails");
    assert_eq!(resumes(&c, "zeus"), 2, "never a third automatic Resume");
}

/// Two failed turns, each tried: the window is at its second Resume and its turn has begun.
fn at_the_second_try(c: &Context, window: &str) {
    fail(c, window);
    passes(c, 3);
    c.advance(31_000);
    until_resumed(c, "zeus", 1);
    next_turn_opens(c, window);
    passes(c, 2);
    fail(c, window);
    passes(c, 3);
    c.advance(121_000);
    until_resumed(c, "zeus", 2);
    next_turn_opens(c, window);
    passes(c, 2);
}

#[test]
fn a_look_that_reads_a_turn_settled_and_not_failed_between_two_failures_resets_nothing() {
    // T-2186: a look between the record's read and the wire's read, or an interrupted turn,
    // reads as a success in every harness. The count rests on nothing a harness reads.
    let c = retrying();
    let (t, window) = working(&c);
    at_the_second_try(&c, &window);
    c.adapter.with(&window, |agent| {
        agent.failed = false;
        agent.settled = true;
    });
    passes(&c, 2);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(600_000);
    passes(&c, 8);
    assert_eq!(t.task(1).task.state, "failed", "two tries, then it fails");
    assert_eq!(resumes(&c, "zeus"), 2, "never a third automatic Resume");
}

#[test]
fn a_failure_30_minutes_after_the_last_automatic_resume_gets_a_fresh_pair() {
    let c = retrying();
    let (t, window) = working(&c);
    at_the_second_try(&c, &window);
    c.advance(29 * 60_000);
    fail(&c, &window);
    passes(&c, 3);
    assert_eq!(
        t.task(1).task.state,
        "failed",
        "29 minutes after the last Resume it is the third failure"
    );
    let c = retrying();
    let (t, window) = working(&c);
    at_the_second_try(&c, &window);
    c.advance(31 * 60_000);
    fail(&c, &window);
    passes(&c, 3);
    let held = t.task(1).task;
    assert_eq!(held.state, "paused", "31 minutes after: a fresh pair");
    assert_eq!(held.retry.expect("held").which, 1);
}

#[test]
fn the_hold_notice_offers_the_resume_the_ledger_accepts_and_a_failed_task_the_reopen() {
    // T-2161 finding 7: the notice told the requester to reopen a task the ledger refuses to
    // reopen while it is held.
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    let notice = t
        .notes("chief")
        .into_iter()
        .find(|note| note.contains("tries it again"))
        .expect("the first try's note");
    assert!(
        notice.contains("cf task resume T-1 \"…\""),
        "the held task's way on is a resume: {notice}"
    );
    assert!(!notice.contains("reopen"), "{notice}");
    c.ledger
        .borrow_mut()
        .resume_task(t.project.id, 1, Some("chief"), "The work, rephrased.")
        .expect("the ledger accepts the resume the notice names");
}

/// The window failed twice and was tried twice: its second try is the turn at work.
fn two_tries_used(c: &Context, window: &str) {
    fail(c, window);
    passes(c, 3);
    c.advance(31_000);
    until_resumed(c, "zeus", 1);
    next_turn_opens(c, window);
    passes(c, 2);
    fail(c, window);
    passes(c, 3);
    c.advance(121_000);
    until_resumed(c, "zeus", 2);
    next_turn_opens(c, window);
    passes(c, 2);
}

/// The turn at work fails again: whether the task is held for another try.
fn fails_again_held(c: &Context, t: &Tiers<'_>, window: &str) -> Option<i64> {
    fail(c, window);
    passes(c, 3);
    t.task(1).task.retry.map(|retry| retry.which)
}

fn refused_for_two_minutes(c: &Context, window: &str, busy: bool) {
    c.advance(1_000);
    let at = Some(super::fixtures::now(c));
    let refusal = if busy {
        Quota::Busy {
            at,
            because: "Selected model is at capacity. Please try a different model.".into(),
        }
    } else {
        Quota::Exhausted {
            at,
            resets_at: Some(super::fixtures::after(c, 2 * 60_000)),
        }
    };
    c.adapter.refuse(window, refusal);
    passes(c, 3);
}

/// T-2189's finding 1: the member's quota or busy model returns by itself with its own Resume,
/// which is no numbered try and no reset: the count stands across it.
fn a_short_hold_after_two_tries_fails_the_next_failure(busy: bool) {
    let c = retrying();
    let (t, window) = working(&c);
    two_tries_used(&c, &window);
    refused_for_two_minutes(&c, &window, busy);
    assert_eq!(t.task(1).task.state, "paused", "held to the reset");
    assert!(t.task(1).task.retry.is_none(), "no retry waits");
    c.advance(121_000);
    until_resumed(&c, "zeus", 3);
    c.adapter.with(&window, |agent| agent.quota = None);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    assert_eq!(fails_again_held(&c, &t, &window), None);
    assert_eq!(t.task(1).task.state, "failed", "the third failure in a row");
    assert_eq!(
        resumes(&c, "zeus"),
        3,
        "two tries and the quota's own Resume"
    );
}

#[test]
fn a_two_minute_quota_hold_after_two_tries_leaves_the_next_failure_to_fail_the_task() {
    a_short_hold_after_two_tries_fails_the_next_failure(false);
}

#[test]
fn a_one_minute_busy_model_hold_after_two_tries_leaves_the_next_failure_to_fail_the_task() {
    a_short_hold_after_two_tries_fails_the_next_failure(true);
}

#[test]
fn a_quota_hold_between_two_failures_changes_nobody_s_try() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    until_resumed(&c, "zeus", 1);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    refused_for_two_minutes(&c, &window, false);
    c.advance(121_000);
    until_resumed(&c, "zeus", 2);
    c.adapter.with(&window, |agent| agent.quota = None);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    assert_eq!(
        fails_again_held(&c, &t, &window),
        Some(2),
        "one try was used: the next failure gets the second"
    );
}

#[test]
fn a_clock_put_back_five_minutes_never_gives_a_third_try() {
    // T-2189's finding 1, second part: the wall clock stepped back after the first Resume.
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    until_resumed(&c, "zeus", 1);
    c.step_wall(-5 * 60_000);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    assert_eq!(fails_again_held(&c, &t, &window), Some(2));
    c.advance(121_000);
    until_resumed(&c, "zeus", 2);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    assert_eq!(fails_again_held(&c, &t, &window), None);
    assert_eq!(t.task(1).task.state, "failed");
    assert_eq!(resumes(&c, "zeus"), 2, "never a third try");
}

#[test]
fn a_failure_29_minutes_59_seconds_after_the_second_try_fails_and_30_minutes_after_begins_again() {
    for (after, held) in [(29 * 60_000 + 59_999, None), (30 * 60_000, Some(1))] {
        let c = retrying();
        let (t, window) = working(&c);
        two_tries_used(&c, &window);
        // The second Resume was delivered just now: time passes since its delivery.
        c.advance(after);
        assert_eq!(
            fails_again_held(&c, &t, &window),
            held,
            "{after} ms after the second try"
        );
    }
}

#[test]
fn a_chief_s_tell_delivered_to_the_window_gives_it_two_tries_again() {
    let c = retrying();
    let (t, window) = working(&c);
    two_tries_used(&c, &window);
    c.advance(1_000);
    c.ledger
        .borrow_mut()
        .tell_queued(t.project.id, 1, "chief", "Use the west file.")
        .unwrap();
    for _ in 0..8 {
        passes(&c, 1);
        if given(&c, "zeus")
            .iter()
            .any(|text| text.contains("Use the west file."))
        {
            break;
        }
    }
    next_turn_opens(&c, &window);
    passes(&c, 2);
    assert_eq!(fails_again_held(&c, &t, &window), Some(1));
}

#[test]
fn a_restart_between_the_first_try_and_the_next_failure_keeps_the_count() {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.edit_ledger(|_| {});
    c.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = c.make();
    restarted.settle_after_restart().unwrap();
    restarted.resume_project(t.project.id).unwrap();
    c.advance(31_000);
    for _ in 0..8 {
        restarted.pass().unwrap();
        if resumes(&c, "zeus") > 0 {
            break;
        }
    }
    assert_eq!(resumes(&c, "zeus"), 1, "the first try, after the restart");
    next_turn_opens(&c, &window);
    for _ in 0..2 {
        restarted.pass().unwrap();
    }
    fail(&c, &window);
    for _ in 0..3 {
        restarted.pass().unwrap();
    }
    assert_eq!(
        t.task(1).task.retry.expect("held again").which,
        2,
        "the count stored before the restart is the one read after it"
    );
}

/// zeus has a backup of its own, and each window failed twice and was tried twice.
fn backed_up(c: &Context, t: &Tiers<'_>) {
    c.ledger
        .borrow_mut()
        .add_backup(
            t.project.id,
            "zeus",
            &cf_ledger::NewMember {
                harness: "claude-code".into(),
                model: Some("gaia".into()),
                effort: None,
                model_name: None,
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            },
        )
        .expect("a backup");
}

/// The task's window failed twice and was tried twice, then fails a third time or is moved: the
/// tries of the window it is in now. Which try each of its next two failures is held for.
fn tries_of(c: &Context, t: &Tiers<'_>, handle: &str) -> Vec<i64> {
    let mut seen = Vec::new();
    for n in 1..=3 {
        let Some(which) = fails_again_held(c, t, handle) else {
            break;
        };
        seen.push(which);
        let due = cf_base::time::parse(t.task(1).task.held_until.as_deref().unwrap()).unwrap();
        c.advance(due - c.time.wall_ms() + 1);
        until_resumed(c, handle, n);
        next_turn_opens(c, handle);
        passes(c, 2);
    }
    seen
}

#[test]
fn a_task_given_to_its_member_s_backup_is_a_fresh_run_with_two_tries_of_its_own() {
    // T-2246 finding 3, decided: each window gets at most two automatic Resumes in a row. The
    // daemon's hand-off to a backup begins the count again at the new window.
    let c = retrying();
    let (t, window) = working(&c);
    backed_up(&c, &t);
    two_tries_used(&c, &window);
    c.advance(1_000);
    c.adapter.refuse(
        &window,
        super::fixtures::exhausted(
            Some(&super::fixtures::now(&c)),
            Some(&super::fixtures::after(&c, 20 * 60_000)),
        ),
    );
    passes(&c, 6);
    let backup = t.assignee(1);
    assert_ne!(backup, window, "the backup took it");
    assert_eq!(t.task(1).task.state, "working");
    next_turn_opens(&c, &backup);
    passes(&c, 2);
    assert_eq!(tries_of(&c, &t, &backup), vec![1, 2]);
    assert_eq!(t.task(1).task.state, "failed", "and no third at the backup");
    assert_eq!(resumes(&c, &window), 2, "two at the first window");
    assert_eq!(resumes(&c, &backup), 2, "two at the backup's");
}

#[test]
fn a_task_a_person_reassigns_is_a_fresh_run_with_two_tries_of_its_own() {
    let c = retrying();
    let (t, window) = working(&c);
    two_tries_used(&c, &window);
    c.reassign_task(t.project.id, 1).expect("reassigned");
    passes(&c, 8);
    let next = t.assignee(1);
    assert_ne!(next, window, "another window");
    next_turn_opens(&c, &next);
    passes(&c, 2);
    assert_eq!(fails_again_held(&c, &t, &next), Some(1));
}

/// T-2246 finding 4: try 1 was delivered, try 2 was queued and withdrawn when the window went on
/// by itself with a job still running; the next turn fails `delay` after try 1's delivery.
fn a_withdrawn_second_try_then_a_failure_after(delay: i64) -> Option<i64> {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    c.advance(31_000);
    until_resumed(&c, "zeus", 1);
    let delivered = c.time.wall_ms();
    next_turn_opens(&c, &window);
    passes(&c, 2);
    fail(&c, &window);
    passes(&c, 3);
    let due = cf_base::time::parse(t.task(1).task.held_until.as_deref().unwrap()).unwrap();
    c.advance(due - c.time.wall_ms() + 1);
    c.adapter.with(&window, |agent| agent.settled = false);
    passes(&c, 2);
    let queued = t
        .task(1)
        .messages
        .into_iter()
        .find(|m| m.kind == "task" && m.state == "queued" && m.sender.is_none())
        .expect("the second try waits for the window");
    c.adapter.wake(
        &window,
        "<task-notification>A background step finished.</task-notification>",
    );
    c.adapter.with(&window, |agent| {
        agent.failed = false;
        agent.failure = None;
    });
    c.adapter
        .answer_leaving_a_job(&window, "One other background step is still running.");
    passes(&c, 3);
    assert_eq!(c.message(queued.id).state, "cancelled", "withdrawn");
    c.advance(delivered + delay - c.time.wall_ms());
    c.adapter.wake(
        &window,
        "<task-notification>The last background step finished.</task-notification>",
    );
    fails_again_held(&c, &t, &window)
}

#[test]
fn a_withdrawn_second_try_never_hides_the_first_delivery_30_minutes_later() {
    assert_eq!(
        a_withdrawn_second_try_then_a_failure_after(30 * 60_000),
        Some(1)
    );
}

#[test]
fn a_withdrawn_second_try_still_counts_one_millisecond_before_the_30_minutes() {
    assert_eq!(
        a_withdrawn_second_try_then_a_failure_after(30 * 60_000 - 1),
        None
    );
}

fn stored_tries(c: &Context) -> i64 {
    c.ledger
        .borrow()
        .sqlite()
        .query_row("SELECT turn_retries FROM task WHERE number = 1", [], |r| {
            r.get(0)
        })
        .expect("the count")
}

/// T-2246 finding 5: an unlinked retry hold (the chief resumed it), then `fresh` numbered
/// tries; the clock goes back to that old hold's instant as the window is refused for quota.
fn a_quota_hold_at_an_old_retry_hold_s_instant(fresh: usize) {
    let c = retrying();
    let (t, window) = working(&c);
    fail(&c, &window);
    passes(&c, 3);
    let old = c.time.wall_ms();
    c.advance(1_000);
    c.resume_task(t.project.id, 1, "Continue with the revised task");
    next_turn_opens(&c, &window);
    passes(&c, 2);
    c.adapter.with(&window, |agent| agent.settled = true);
    passes(&c, 6);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    c.advance(5_000);
    for n in 1..=fresh {
        fail(&c, &window);
        passes(&c, 3);
        let due = cf_base::time::parse(t.task(1).task.held_until.as_deref().unwrap()).unwrap();
        c.advance(due - c.time.wall_ms() + 1);
        until_resumed(&c, "zeus", n);
        next_turn_opens(&c, &window);
        passes(&c, 2);
    }
    assert_eq!(stored_tries(&c), fresh as i64);
    c.step_wall(old - c.time.wall_ms());
    c.adapter.refuse(
        &window,
        super::fixtures::exhausted(
            Some(&super::fixtures::now(&c)),
            Some(&super::fixtures::after(&c, 2 * 60_000)),
        ),
    );
    passes(&c, 4);
    assert_eq!(t.task(1).task.state, "paused");
    let shown = t.task(1).task.retry.map(|retry| retry.which);
    let due = cf_base::time::parse(t.task(1).task.held_until.as_deref().unwrap()).unwrap();
    c.advance(due - c.time.wall_ms() + 1);
    passes(&c, 6);
    c.adapter.with(&window, |agent| agent.quota = None);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    assert_eq!(
        stored_tries(&c),
        fresh as i64,
        "the quota's own Resume used no try"
    );
    assert_eq!(shown, None, "and its hold is shown as no retry");
}

#[test]
fn a_quota_s_own_resume_is_never_a_try_at_the_instant_of_an_old_hold_after_two() {
    a_quota_hold_at_an_old_retry_hold_s_instant(2);
}

#[test]
fn a_quota_s_own_resume_is_never_a_try_at_the_instant_of_an_old_hold_after_one() {
    a_quota_hold_at_an_old_retry_hold_s_instant(1);
}

#[test]
fn the_card_shows_the_try_the_next_resume_will_use_after_a_tell_in_the_hold() {
    // T-2246 finding 6: the count was reset by the chief's tell while try 2 was held.
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
    c.advance(1_000);
    c.ledger
        .borrow_mut()
        .tell_queued(t.project.id, 1, "chief", "Use the other file")
        .unwrap();
    assert_eq!(stored_tries(&c), 0);
    assert_eq!(
        t.task(1).task.retry.expect("still held").which,
        1,
        "the next Resume is the first of two again"
    );
}

/// The task's held try comes due and its Resume is queued; the window goes on by itself with a
/// job still running, so the Resume is withdrawn: charged, never delivered.
fn a_try_queued_and_withdrawn(c: &Context, t: &Tiers<'_>, window: &str) {
    let due = cf_base::time::parse(t.task(1).task.held_until.as_deref().unwrap()).unwrap();
    c.advance(due - c.time.wall_ms() + 1);
    c.adapter.with(window, |agent| agent.settled = false);
    passes(c, 2);
    let queued = t
        .task(1)
        .messages
        .into_iter()
        .find(|m| m.kind == "task" && m.state == "queued" && m.sender.is_none())
        .expect("the try waits for the window");
    c.adapter.wake(
        window,
        "<task-notification>One background step finished.</task-notification>",
    );
    c.adapter.with(window, |agent| {
        agent.failed = false;
        agent.failure = None;
    });
    c.adapter
        .answer_leaving_a_job(window, "The other background step is still running.");
    passes(c, 3);
    assert_eq!(c.message(queued.id).state, "cancelled", "withdrawn");
    assert_eq!(t.task(1).task.state, "working");
}

/// The window's next turn, woken by its last job, fails.
fn woken_and_failed(c: &Context, t: &Tiers<'_>, window: &str) -> Option<i64> {
    c.advance(1_000);
    c.adapter.wake(
        window,
        "<task-notification>The last background step finished.</task-notification>",
    );
    fails_again_held(c, t, window)
}

/// Try 1 delivered, then a failure 30 minutes after it: the count renews. Where it is held.
fn renewed_after_30_minutes(c: &Context, t: &Tiers<'_>, window: &str) {
    fail(c, window);
    passes(c, 3);
    c.advance(31_000);
    until_resumed(c, "zeus", 1);
    let delivered = c.time.wall_ms();
    next_turn_opens(c, window);
    passes(c, 2);
    c.advance(delivered + 30 * 60_000 - c.time.wall_ms());
    assert_eq!(fails_again_held(c, t, window), Some(1), "renewed");
    assert_eq!(stored_tries(c), 0);
}

#[test]
fn one_delivery_renews_the_count_once_though_the_fresh_pair_s_first_try_is_withdrawn() {
    // T-2262 finding 8: the delivery that renewed the count renewed it again a second after
    // the fresh pair's first try was queued and withdrawn.
    let c = retrying();
    let (t, window) = working(&c);
    renewed_after_30_minutes(&c, &t, &window);
    a_try_queued_and_withdrawn(&c, &t, &window);
    assert_eq!(stored_tries(&c), 1, "withdrawn, and still charged");
    assert_eq!(
        woken_and_failed(&c, &t, &window),
        Some(2),
        "the second of the pair"
    );
    assert_eq!(stored_tries(&c), 1);
    let due = cf_base::time::parse(t.task(1).task.held_until.as_deref().unwrap()).unwrap();
    c.advance(due - c.time.wall_ms() + 1);
    until_resumed(&c, "zeus", 2);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    assert_eq!(fails_again_held(&c, &t, &window), None);
    assert_eq!(
        t.task(1).task.state,
        "failed",
        "two charged in the fresh pair, no third"
    );
}

#[test]
fn a_fresh_pair_withdrawn_whole_renews_again_31_minutes_after_the_renewal() {
    let c = retrying();
    let (t, window) = working(&c);
    renewed_after_30_minutes(&c, &t, &window);
    let renewed = c.time.wall_ms();
    a_try_queued_and_withdrawn(&c, &t, &window);
    assert_eq!(woken_and_failed(&c, &t, &window), Some(2));
    a_try_queued_and_withdrawn(&c, &t, &window);
    assert_eq!(stored_tries(&c), 2);
    c.advance(renewed + 31 * 60_000 - c.time.wall_ms());
    assert_eq!(
        woken_and_failed(&c, &t, &window),
        Some(1),
        "nothing was sent since the renewal, 31 minutes ago"
    );
    assert_eq!(stored_tries(&c), 0);
}

#[test]
fn one_word_then_its_first_try_withdrawn_then_two_failures_gets_two_tries_and_no_more() {
    // The count started again by a word: the delivery from before it is long past, and must
    // not renew away the first try after it.
    let c = retrying();
    let (t, window) = working(&c);
    two_tries_used(&c, &window);
    c.advance(31 * 60_000);
    c.ledger
        .borrow_mut()
        .tell_queued(t.project.id, 1, "chief", "Use the west file.")
        .unwrap();
    // The window rests, takes the chief's word, and works on it.
    c.adapter.with(&window, |agent| agent.settled = true);
    passes(&c, 8);
    assert!(
        given(&c, "zeus")
            .iter()
            .any(|text| text.contains("Use the west file.")),
        "the word reached the window"
    );
    next_turn_opens(&c, &window);
    passes(&c, 2);
    assert_eq!(fails_again_held(&c, &t, &window), Some(1));
    a_try_queued_and_withdrawn(&c, &t, &window);
    assert_eq!(stored_tries(&c), 1);
    assert_eq!(
        woken_and_failed(&c, &t, &window),
        Some(2),
        "one of the two is charged"
    );
    let due = cf_base::time::parse(t.task(1).task.held_until.as_deref().unwrap()).unwrap();
    c.advance(due - c.time.wall_ms() + 1);
    until_resumed(&c, "zeus", 3);
    next_turn_opens(&c, &window);
    passes(&c, 2);
    assert_eq!(fails_again_held(&c, &t, &window), None);
    assert_eq!(
        t.task(1).task.state,
        "failed",
        "no third try after the word"
    );
}
