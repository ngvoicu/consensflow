//! A follow-up queued behind a window's own work (T-110, 3.0.6; the chief of
//! 2026-10-21, running this project on 3.0.5): `--after T-3` was refused while
//! T-3's window was still on T-3, so the chief could not queue the next step of
//! a window's work while it ran. The follow-up is taken now, waits on the board
//! for T-3 to be accepted (T-3 is one of the tasks it needs), and goes into
//! that window then, as the follow-up of a finished task does. A window that
//! has a task in hand other than the one followed is refused as before.

use cf_ledger::LedgerError;

use crate::fixture::{gated_world, world, World};

/// A task opened for the standard workers, given to a session of zeus and at
/// work: the session and the task.
fn at_work(w: &mut World) -> (String, i64) {
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    assert_eq!(w.state(task), "working");
    (session, task)
}

/// The result of `number` is recorded and accepted.
fn finished(w: &mut World, number: i64) {
    w.ledger
        .record_result(w.project, number, "Done")
        .expect("a result");
    w.ledger
        .accept_task(w.project, number, "chief")
        .expect("accepted");
}

/// The numbers a task is blocked by, in order.
fn blocked_by(w: &World, number: i64) -> Vec<i64> {
    let mut blocked = w
        .ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .blocked_by;
    blocked.sort_unstable();
    blocked
}

/// The notes the chief has been sent about task `number`, oldest first.
fn notes_to_the_chief(w: &World, number: i64) -> Vec<String> {
    let mut notes: Vec<_> = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the chief's inbox")
        .into_iter()
        .filter(|message| message.kind == "note" && message.task_number == Some(number))
        .collect();
    notes.sort_by_key(|message| message.id);
    notes.into_iter().map(|message| message.body).collect()
}

#[test]
fn a_follow_up_of_a_task_at_work_waits_for_it_and_goes_to_its_window_once_it_is_accepted() {
    let mut w = world();
    let (session, a) = at_work(&mut w);

    let b = w
        .follow_up(a, "Tests", &[])
        .expect("taken while the window is on its work");
    assert!(
        b.message.is_none(),
        "nothing of it is queued for the window"
    );
    assert_eq!(b.task.state, "open");
    assert_eq!(b.task.assignee.as_deref(), Some(session.as_str()));
    assert_eq!(b.task.blocked_by, [a], "T-{a} is what it waits for");
    let b = b.task.number;
    assert_eq!(w.next(&session), None, "the window is left to its work");

    // The result is in, and not accepted: the follow-up still waits.
    w.ledger
        .record_result(w.project, a, "Parser, done")
        .expect("a result");
    assert_eq!(w.state(b), "open");
    w.ledger
        .accept_task(w.project, a, "chief")
        .expect("accepted");
    assert_eq!(w.state(b), "queued");
    assert!(blocked_by(&w, b).is_empty());
    let head = w.next(&session).expect("the follow-up for the window");
    assert_eq!(w.message(head).task_number, Some(b));
    assert_eq!(w.message(head).body, "Tests");
    w.deliver(head);
    assert_eq!(w.state(b), "working");
}

#[test]
fn two_follow_ups_of_one_window_at_work_go_to_it_in_order() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w.follow_up(a, "First", &[]).expect("a follow-up").task;
    let c = w.follow_up(a, "Second", &[]).expect("a second one").task;
    assert_eq!((b.state.as_str(), c.state.as_str()), ("open", "open"));
    assert_eq!(c.assignee.as_deref(), Some(session.as_str()));

    finished(&mut w, a);
    assert_eq!(w.state(b.number), "queued", "the first goes");
    assert_eq!(w.state(c.number), "open", "the second waits behind it");
    let head = w.next(&session).expect("the first");
    assert_eq!(w.message(head).task_number, Some(b.number));
    w.deliver(head);
    assert_eq!(w.state(c.number), "open", "while the first is at work");

    w.ledger
        .record_result(w.project, b.number, "First, done")
        .expect("a result");
    assert_eq!(
        w.state(c.number),
        "queued",
        "the second goes with its result"
    );
    let head = w.next(&session).expect("the second");
    assert_eq!(w.message(head).task_number, Some(c.number));
}

#[test]
fn a_follow_up_of_a_follow_up_that_waits_waits_for_it_too() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "First", &[])
        .expect("a follow-up")
        .task
        .number;
    let c = w
        .follow_up(b, "Second", &[])
        .expect("a follow-up of the one that waits");
    assert_eq!(c.task.state, "open");
    assert_eq!(c.task.blocked_by, [b]);
    let c = c.task.number;

    finished(&mut w, a);
    assert_eq!(w.state(b), "queued");
    assert_eq!(w.state(c), "open", "T-{c} waits for T-{b} to be accepted");
    let head = w.next(&session).expect("the first");
    w.deliver(head);
    finished(&mut w, b);
    assert_eq!(w.state(c), "queued");
}

#[test]
fn a_follow_up_that_needs_other_tasks_too_waits_for_all_of_them() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let other = w.open("Docs").task.number;
    let b = w
        .follow_up(a, "Tests", &[other, a])
        .expect("a follow-up with its needs");
    assert_eq!(blocked_by(&w, b.task.number), [a, other], "T-{a} once");
    let b = b.task.number;

    finished(&mut w, a);
    assert_eq!(w.state(b), "open", "T-{other} is not accepted");
    let moved = w.assign(other, "diana");
    w.deliver(moved.message.expect("its brief").id);
    finished(&mut w, other);
    assert_eq!(w.state(b), "queued");
    assert!(w.next(&session).is_some());
}

#[test]
fn a_follow_up_of_a_task_whose_window_is_on_another_is_refused_as_before() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    finished(&mut w, a);
    // B is the window's now; A is accepted, and a follow-up of A is not B's.
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    w.deliver(w.next(&session).expect("its brief"));
    assert_eq!(w.state(b), "working");

    let refused = w
        .follow_up(a, "More", &[])
        .expect_err("the window is on another task");
    assert_eq!(refused.code(), Some("session-busy"));
    assert_eq!(
        refused.to_string(),
        format!(
            "@{session} is still on its work: wait for its result, or open the task for its tier"
        )
    );
}

#[test]
fn a_task_that_fails_leaves_its_follow_up_waiting_and_the_chief_is_told_once() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    assert!(notes_to_the_chief(&w, b).is_empty());

    w.ledger
        .fail_task(w.project, a, "its pane died")
        .expect("failed");
    assert_eq!(w.state(b), "open", "it is not released, nor lost");
    assert_eq!(blocked_by(&w, b), [a]);
    assert_eq!(
        notes_to_the_chief(&w, b),
        [format!(
            "T-{b} waits for T-{a}, which failed: cancel T-{b}, or reopen T-{a} and let it finish."
        )]
    );

    // Reopened, T-a goes on in its window, and its follow-up goes when it is accepted.
    let words = w
        .ledger
        .reopen_task(w.project, a, "chief", "Again")
        .expect("reopened")
        .message
        .expect("its words");
    w.deliver(words.id);
    assert_eq!(w.state(a), "working");
    assert_eq!(w.state(b), "open");
    finished(&mut w, a);
    assert_eq!(w.state(b), "queued");
    assert!(w.next(&session).is_some());
    assert_eq!(notes_to_the_chief(&w, b).len(), 1, "told once");
}

#[test]
fn a_task_with_a_follow_up_waiting_is_not_cancelled_until_the_follow_up_is() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;

    let refused: LedgerError = w
        .ledger
        .cancel_task(w.project, a, "chief")
        .expect_err("T-b waits for it");
    assert_eq!(refused.code(), Some("waited-for"));
    assert_eq!(
        refused.to_string(),
        format!("T-{a} cannot be cancelled while T-{b} waits for it: cancel T-{b} first, or let T-{a} finish")
    );
    assert_eq!(w.state(a), "working");

    w.ledger
        .cancel_task(w.project, b, "chief")
        .expect("the follow-up is cancelled");
    w.ledger
        .cancel_task(w.project, a, "chief")
        .expect("and then the task");
    assert_eq!(w.state(a), "cancelled");
    assert_eq!(w.next(&session), None);
}

#[test]
fn a_follow_up_that_waits_for_a_task_failing_twice_is_told_to_the_chief_each_time() {
    // A failure is not final, as a cancel is: a reopened task can fail again, so the
    // chief is told at each failure, and the once-for-good mark of a cancelled need
    // (`task.stranded`) is not the follow-up's.
    let mut w = world();
    let (_, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;

    w.ledger
        .fail_task(w.project, a, "its pane died")
        .expect("failed");
    let words = w
        .ledger
        .reopen_task(w.project, a, "chief", "Again")
        .expect("reopened")
        .message
        .expect("its words");
    w.deliver(words.id);
    w.ledger
        .fail_task(w.project, a, "its pane died again")
        .expect("failed again");
    let told = format!(
        "T-{b} waits for T-{a}, which failed: cancel T-{b}, or reopen T-{a} and let it finish."
    );
    assert_eq!(notes_to_the_chief(&w, b), [told.clone(), told]);
    assert_eq!(w.state(b), "open");
}

#[test]
fn a_member_removed_with_a_task_a_follow_up_waits_behind_cancels_both_and_tells_the_chief_of_the_rest(
) {
    let mut w = world();
    let (_, a) = at_work(&mut w);
    // Bound to zeus's window, the follow-up goes with it (T-158); a task for a tier
    // that waits for the task is not, and it will never start: the chief is told once.
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    let c = w.open_needing("Docs", &[a]).task.number;

    let removed = w
        .ledger
        .remove_member(w.project, "zeus")
        .expect("zeus leaves");
    assert_eq!(removed.cancelled, [a, b]);
    assert_eq!(w.state(b), "cancelled");
    assert_eq!(w.state(c), "open");
    assert_eq!(
        notes_to_the_chief(&w, c),
        [format!(
            "T-{c} waits on cancelled T-{a} and will never start."
        )]
    );
    assert!(
        notes_to_the_chief(&w, b).is_empty(),
        "it is cancelled, not stranded"
    );
}

#[test]
fn a_follow_up_put_before_the_ones_waiting_for_a_task_with_its_result_in_goes_to_the_window_now() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "Later", &[])
        .expect("a follow-up")
        .task
        .number;
    w.ledger
        .record_result(w.project, a, "Parser, done")
        .expect("a result");

    // The chief says it goes ahead of T-2: the rule of 2026-10-15 holds.
    let first = w
        .follow_up_before(a, "First", &[b])
        .expect("put before the one that waits");
    assert_eq!(first.task.state, "queued");
    let first = first.task.number;
    let head = w.next(&session).expect("for the window");
    assert_eq!(w.message(head).task_number, Some(first));
}

#[test]
fn a_decline_of_the_brief_of_a_task_a_follow_up_waits_for_is_refused_in_the_declines_words() {
    let mut w = gated_world();
    let task = w.open("Parser").task.number;
    let brief = w.assign(task, "zeus").message.expect("its brief").id;
    assert_eq!(
        w.message(brief).state,
        "gated",
        "the brief waits for the human"
    );
    // The window has not begun on T-1, and its follow-up still waits behind it.
    let follow_up = w
        .follow_up(task, "Tests", &[])
        .expect("a follow-up of a task whose brief waits")
        .task
        .number;
    assert_eq!(w.state(follow_up), "open");

    let declined = w
        .ledger
        .decline_message(brief, "human")
        .expect_err("the follow-up waits for the task");
    assert_eq!(declined.code(), Some("waited-for"));
    assert_eq!(
        declined.to_string(),
        format!("T-{task} cannot be cancelled while T-{follow_up} waits for it: cancel T-{follow_up} first, or pass T-{task} on")
    );
    assert_eq!(w.state(task), "queued", "the task was not cancelled");

    w.ledger
        .cancel_task(w.project, follow_up, "chief")
        .expect("the follow-up is cancelled");
    w.ledger
        .decline_message(brief, "human")
        .expect("nothing waits for the task now");
    assert_eq!(w.state(task), "cancelled");
}

// A follow-up ends with its window or with the task it follows (T-158, round 2).

/// The ids of what is on its way to `participant`.
fn on_its_way(w: &World, participant: i64) -> Vec<i64> {
    w.ledger
        .pending(participant)
        .expect("a read")
        .into_iter()
        .map(|message| message.id)
        .collect()
}

#[test]
fn a_member_removed_with_follow_ups_bound_to_its_session_cancels_them_and_nothing_is_queued_for_the_left_session(
) {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    // T-3 behind T-2 behind T-1, all in zeus's window.
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    let c = w
        .follow_up(b, "Docs", &[])
        .expect("a follow-up of it")
        .task
        .number;
    let left = w.id(&session);

    let removed = w
        .ledger
        .remove_member(w.project, "zeus")
        .expect("zeus leaves");
    assert_eq!(
        removed.cancelled,
        [a, b, c],
        "the follow-ups go with the window"
    );
    for number in [a, b, c] {
        assert_eq!(w.state(number), "cancelled", "T-{number}");
    }

    // Whatever is accepted anywhere in the project later releases what waits, and
    // nothing of it is queued for a session no window opens for again.
    let other = w.open("Lexer").task.number;
    let moved = w.assign(other, "diana");
    w.deliver(moved.message.expect("its brief").id);
    finished(&mut w, other);
    assert!(on_its_way(&w, left).is_empty());
}

#[test]
fn a_follow_up_behind_a_task_paused_in_a_removed_members_window_is_cancelled_and_the_task_goes_to_another_member(
) {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    let left = w.id(&session);
    w.pause(a);

    // The removal leaves the paused task for its resumption, and not the follow-up
    // that is bound to the window which goes.
    let removed = w
        .ledger
        .remove_member(w.project, "zeus")
        .expect("zeus leaves");
    assert_eq!(removed.cancelled, [b]);
    assert_eq!(w.state(a), "paused");
    assert_eq!(w.state(b), "cancelled");

    w.resume(a, "Go on");
    assert_eq!(w.state(a), "open", "back on the board for a member");
    let moved = w.assign(a, "diana");
    w.deliver(moved.message.expect("its brief").id);
    finished(&mut w, a);
    assert!(on_its_way(&w, left).is_empty());
}

/// A ledger 3.0.5 left: the member and its session gone, a follow-up still open and
/// bound to the session (`left_at` set on `who` by hand).
fn left_in_an_older_build(w: &mut World, session: &str, member_too: bool) {
    let session = session.to_owned();
    w.edit(|db| {
        db.execute(
            "UPDATE participant SET left_at = '2026-10-21T10:00:00.000Z'
             WHERE handle = ?1 OR (?2 AND id = (SELECT member_id FROM participant WHERE handle = ?1))",
            rusqlite::params![session, member_too],
        )
        .expect("the member and its session are gone");
    });
}

#[test]
fn a_follow_up_bound_to_a_session_that_left_in_an_older_build_is_cancelled_when_it_would_go_and_the_chief_is_told(
) {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    let left = w.id(&session);
    left_in_an_older_build(&mut w, &session, true);

    finished(&mut w, a);
    assert_eq!(
        w.state(b),
        "cancelled",
        "not queued for a window nobody opens"
    );
    assert_eq!(
        notes_to_the_chief(&w, b),
        [format!(
            "T-{b} was cancelled: @{session} has left the staff, so no window can take it."
        )]
    );
    assert!(on_its_way(&w, left).is_empty());
}

#[test]
fn a_follow_up_bound_to_a_session_the_human_deleted_brings_it_back_as_any_follow_up_does() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    // Only the session is gone (the member is on the staff): its conversation is kept.
    left_in_an_older_build(&mut w, &session, false);

    finished(&mut w, a);
    assert_eq!(w.state(b), "queued");
    let head = w.next(&session).expect("the session is back on the board");
    assert_eq!(w.message(head).task_number, Some(b));
}

#[test]
fn a_follow_up_added_after_the_result_but_before_the_acceptance_lines_up_behind_the_ones_waiting() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "First", &[])
        .expect("a follow-up")
        .task
        .number;
    w.ledger
        .record_result(w.project, a, "Parser, done")
        .expect("a result");

    // The chief reads the result and adds another before it accepts: it waits its
    // turn behind T-2, and does not overtake it.
    let c = w.follow_up(a, "Second", &[]).expect("a second follow-up");
    assert_eq!(c.task.state, "open");
    assert!(c.message.is_none());
    assert_eq!(c.task.blocked_by, [a]);
    let c = c.task.number;

    w.ledger
        .accept_task(w.project, a, "chief")
        .expect("accepted");
    assert_eq!(w.state(b), "queued");
    assert_eq!(w.state(c), "open");
    let head = w.next(&session).expect("the first");
    assert_eq!(w.message(head).task_number, Some(b));
    w.deliver(head);
    w.ledger
        .record_result(w.project, b, "First, done")
        .expect("a result");
    assert_eq!(
        w.state(c),
        "queued",
        "the second goes with the first's result"
    );
}

#[test]
fn a_task_taken_back_from_a_window_names_the_follow_ups_still_bound_to_it_to_its_requester() {
    let mut w = world();
    let (session, a) = at_work(&mut w);
    let b = w
        .follow_up(a, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    let c = w.follow_up(a, "Docs", &[]).expect("another").task.number;
    let d = w.open("Lexer").task.number;
    assert_eq!(w.state(d), "open");

    w.ledger
        .release_task(w.project, a, "out of quota")
        .expect("taken back");
    let told = notes_to_the_chief(&w, a);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].ends_with(&format!(
            " T-{b}, T-{c} stay bound to @{session}, which no longer has T-{a}: cancel them, or add them again for a tier with --needs T-{a}."
        )),
        "{told:?}"
    );
}

#[test]
fn a_follow_up_of_a_task_nobody_has_taken_yet_is_told_to_wait_for_it_with_needs() {
    let mut w = world();
    let task = w.open("Parser").task.number;
    let refused = w
        .follow_up(task, "Tests", &[])
        .expect_err("no window has it yet");
    assert_eq!(refused.code(), Some("no-window-yet"));
    assert_eq!(
        refused.to_string(),
        format!("T-{task} has no window yet: nobody has taken it. Add the next step for a tier with --needs T-{task}, and it waits until T-{task} is accepted")
    );
}
