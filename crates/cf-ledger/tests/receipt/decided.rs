//! A result is put to its requester to decide on. The decision (accepting the
//! task or sending it back) withdraws a result the requester was not given
//! yet, in its own transaction, so it is never pasted after the decision it
//! asks for: the chief's turn of 2026-10-08, which read T-9's result queued
//! behind it, accepted the task and put T-10 after it, was then given the
//! result with "Decide with: cf task accept T-9 …". A result being pasted or
//! given stays. A cancel is the third decision on a result, as the chief's text
//! tells it (`cf task cancel T-3`): a task that is done is called off, its
//! result stays as it was but for one the requester was not given yet, which is
//! withdrawn as a decision's is, and the window that did it is told nothing and
//! stopped for nothing (T-1122, the field report of 2026-11-02: the chief could
//! neither cancel nor send back a done task whose window held a paused one).

use cf_ledger::{LedgerError, MessageView, NewNote};

use crate::fixture::{world, World};

/// A task opened for the standard workers and given to a session of `member`,
/// its brief received and its result recorded: the task, and its result,
/// which waits in the chief's queue.
fn finished(w: &mut World, member: &str, body: &str) -> (i64, MessageView) {
    let task = w.open(body).task.number;
    let moved = w.assign(task, member);
    w.deliver(moved.message.expect("its brief").id);
    let result = w
        .ledger
        .record_result(w.project, task, &format!("{body}, done"))
        .expect("a result")
        .message
        .expect("its result");
    assert_eq!(w.state(task), "done");
    (task, result)
}

fn accept(w: &mut World, task: i64) {
    w.ledger
        .accept_task(w.project, task, "chief")
        .expect("accepted");
}

fn send_back(w: &mut World, task: i64) {
    w.ledger
        .reopen_task(w.project, task, "chief", "Handle empty input too")
        .expect("sent back");
}

/// A decision on a task's result: what the chief does, how it is made, and
/// the reason it gives a result that it withdraws.
struct Decision {
    what: &'static str,
    make: fn(&mut World, i64),
    reason: &'static str,
}

/// The two decisions on a result.
const DECISIONS: [Decision; 2] = [
    Decision {
        what: "accepts",
        make: accept,
        reason: "T-1 was accepted",
    },
    Decision {
        what: "sends back",
        make: send_back,
        reason: "T-1 was sent back",
    },
];

/// What a message is now: its state and the reason it was given.
fn fate(w: &World, id: i64) -> (String, Option<String>) {
    let message = w.message(id);
    (message.state, message.reason)
}

/// ConsensFlow's own note to the chief about a task.
fn told_chief(w: &mut World, task: i64, body: &str) -> MessageView {
    w.ledger
        .note(
            w.project,
            &NewNote {
                from: None,
                to: "chief".into(),
                body: body.into(),
                task: Some(task),
            },
        )
        .expect("a note")
}

#[test]
fn the_chief_that_read_a_result_and_accepted_its_task_in_its_turn_is_not_given_the_result_after() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    // The chief's own turn is running: the result waits at the head of its
    // queue, and `cf task get` shows it without receiving it.
    assert_eq!(w.next("chief"), Some(result.id));
    let thread = w
        .ledger
        .task(w.project, task)
        .expect("a read")
        .expect("T-1");
    assert!(
        thread
            .messages
            .iter()
            .any(|message| message.id == result.id && message.state == "queued"),
        "the thread it printed shows the result still queued"
    );
    assert_eq!(
        w.message(result.id).state,
        "queued",
        "a read receives nothing"
    );

    // It decides, and puts the next task after this one.
    accept(&mut w, task);
    let next = w
        .follow_up(task, "Lexer", &[])
        .expect("T-2 after T-1")
        .message
        .expect("its brief");

    assert_eq!(
        fate(&w, result.id),
        ("cancelled".to_owned(), Some("T-1 was accepted".to_owned()))
    );
    assert_eq!(
        w.next("chief"),
        None,
        "its turn ends, and nothing is pasted"
    );
    assert_eq!(w.state(task), "accepted");
    assert_eq!(w.message(next.id).state, "queued", "the next task goes on");
}

#[test]
fn the_chief_that_read_a_result_and_sent_its_task_back_in_its_turn_is_not_given_the_result_after() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    assert_eq!(w.next("chief"), Some(result.id));

    let moved = w
        .ledger
        .reopen_task(w.project, task, "chief", "Handle empty input too")
        .expect("sent back");
    let follow_up = moved.message.expect("its follow-up");

    assert_eq!(
        fate(&w, result.id),
        ("cancelled".to_owned(), Some("T-1 was sent back".to_owned()))
    );
    assert_eq!(
        w.next("chief"),
        None,
        "its turn ends, and nothing is pasted"
    );
    assert_eq!(w.state(task), "queued");
    assert_eq!(
        w.message(follow_up.id).state,
        "queued",
        "the work sent back goes to its window"
    );
}

#[test]
fn a_decision_withdraws_the_result_of_its_task_and_nothing_else_the_chief_is_waiting_for() {
    for decision in DECISIONS {
        let mut w = world();
        let (task, result) = finished(&mut w, "zeus", "Parser");
        let (other, other_result) = finished(&mut w, "diana", "Lexer");
        let note = told_chief(&mut w, task, "T-1 waits with @zeus: out of quota.");
        let elsewhere = told_chief(&mut w, other, "T-2 waits with @diana: out of quota.");

        (decision.make)(&mut w, task);

        assert_eq!(
            fate(&w, result.id),
            ("cancelled".to_owned(), Some(decision.reason.to_owned()))
        );
        for (kept, why) in [
            (other_result.id, "the result of another task"),
            (note.id, "a note about the same task"),
            (elsewhere.id, "a note about another task"),
        ] {
            assert_eq!(w.message(kept).state, "queued", "{why}");
        }
        assert_eq!(
            w.next("chief"),
            Some(other_result.id),
            "the chief is still given what it waits for, oldest first"
        );
        assert_eq!(w.state(other), "done", "the other task is as it was");
    }
}

#[test]
fn a_result_being_pasted_when_the_decision_comes_stays_and_arrives() {
    for decision in DECISIONS {
        let mut w = world();
        let (task, result) = finished(&mut w, "zeus", "Parser");
        w.begin(result.id);

        (decision.make)(&mut w, task);

        assert_eq!(
            fate(&w, result.id),
            ("delivering".to_owned(), None),
            "the chief {} T-1 while its result is being pasted: it is in the window",
            decision.what
        );
        w.confirm(result.id);
        assert_eq!(
            w.message(result.id).state,
            "delivered",
            "and the paste is proved, as ever"
        );
    }
}

#[test]
fn a_result_the_chief_was_given_already_stays_when_it_decides() {
    for decision in DECISIONS {
        let mut w = world();
        let (task, result) = finished(&mut w, "zeus", "Parser");
        w.deliver(result.id);
        let given = w.message(result.id);

        (decision.make)(&mut w, task);

        let after = w.message(result.id);
        assert_eq!(
            (after.state.as_str(), after.reason.as_deref()),
            ("delivered", None),
            "the chief {} T-1 after it was given the result",
            decision.what
        );
        assert_eq!(after.receipt, given.receipt, "with its receipt");
    }
}

#[test]
fn a_refused_decision_withdraws_nothing() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    w.ledger
        .remove_member(w.project, "zeus")
        .expect("the member leaves the staff");

    // The session that did the work cannot go on: it is not sent back to it.
    let refused = w
        .ledger
        .reopen_task(w.project, task, "chief", "Again")
        .expect_err("the session has ended");
    assert_eq!(refused.code(), Some("session-ended"));
    assert_eq!(
        w.message(result.id).state,
        "queued",
        "so the chief is still to be given it"
    );
    assert_eq!(w.state(task), "done");
}

/// The session that did `task`.
fn session_of(w: &World, task: i64) -> String {
    w.ledger
        .task(w.project, task)
        .expect("a read")
        .expect("the task")
        .task
        .assignee
        .expect("a session")
}

/// The task a session's window is asked to stop for, and how many times.
fn stop_of(w: &World, session: &str) -> Option<(i64, i64)> {
    w.ledger
        .stop_of(w.id(session))
        .expect("a read")
        .map(|stop| (stop.number, stop.seq))
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

fn cancel(w: &mut World, task: i64, by: &str) -> Result<(), LedgerError> {
    w.ledger.cancel_task(w.project, task, by).map(|_| ())
}

#[test]
fn the_chief_cancels_a_done_task_whose_result_it_was_given_and_the_result_stays_as_it_was() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    let session = session_of(&w, task);
    w.deliver(result.id);
    let given = w.message(result.id);

    let cancelled = w
        .ledger
        .cancel_task(w.project, task, "chief")
        .expect("a done task is called off, as the chief's text says");

    assert_eq!(cancelled.state, "cancelled");
    let after = w.message(result.id);
    assert_eq!(
        (after.state.as_str(), after.reason.as_deref()),
        ("delivered", None),
        "the result the chief was given stays as it was"
    );
    assert_eq!(after.receipt, given.receipt, "with its receipt");
    assert!(
        !w.ledger.has_task_in_hand(w.id(&session)).expect("a read"),
        "the window that did it has nothing in hand, as after an acceptance"
    );
    assert_eq!(w.next(&session), None, "nothing is put to the window");
    assert_eq!(stop_of(&w, &session), None, "its turn is over: no stop");
    assert_eq!(
        w.next("chief"),
        None,
        "the chief is told nothing of its own cancel"
    );
}

#[test]
fn a_cancel_withdraws_the_result_of_a_done_task_the_chief_was_not_given_yet_as_a_decision_does() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    let (other, other_result) = finished(&mut w, "diana", "Lexer");
    assert_eq!(w.next("chief"), Some(result.id));

    cancel(&mut w, task, "chief").expect("called off");

    assert_eq!(
        fate(&w, result.id),
        ("cancelled".to_owned(), Some("T-1 was cancelled".to_owned())),
        "it asks for a decision that was taken"
    );
    assert_eq!(
        w.next("chief"),
        Some(other_result.id),
        "the result of another task is still to be given"
    );
    assert_eq!(w.state(other), "done");
}

#[test]
fn a_result_being_pasted_when_a_done_task_is_cancelled_stays_and_arrives() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    w.begin(result.id);

    cancel(&mut w, task, "chief").expect("called off");

    assert_eq!(fate(&w, result.id), ("delivering".to_owned(), None));
    w.confirm(result.id);
    assert_eq!(w.message(result.id).state, "delivered");
}

#[test]
fn whoever_asked_for_a_done_task_is_told_it_was_cancelled_and_that_no_window_was_stopped() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    w.deliver(result.id);

    cancel(&mut w, task, "human").expect("called off");

    assert_eq!(
        notes_to_the_chief(&w, task),
        ["@human cancelled T-1 (Parser)."],
        "as for any cancel, but its window had ended its turn: nothing was stopped"
    );
}

#[test]
fn a_done_task_other_tasks_wait_for_is_not_cancelled_until_they_are() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    w.deliver(result.id);
    let waiting = w
        .follow_up(task, "Tests", &[task])
        .expect("a follow-up that waits for T-1")
        .task
        .number;
    assert_eq!(w.state(waiting), "open");

    let refused = cancel(&mut w, task, "chief").expect_err("T-2 waits for it");

    assert_eq!(refused.code(), Some("waited-for"));
    assert_eq!(
        refused.to_string(),
        "T-1 cannot be cancelled while T-2 waits for it: accept T-1, or cancel T-2 first"
    );
    assert_eq!(w.state(task), "done", "nothing of it is touched");
    assert_eq!(w.state(waiting), "open", "and nothing is released");
    assert_eq!(w.message(result.id).state, "delivered");

    cancel(&mut w, waiting, "chief").expect("the waiter first");
    cancel(&mut w, task, "chief").expect("then the task");
    assert_eq!(w.state(task), "cancelled");
}

/// A follow-up of a done task waits for its acceptance (T-1118), so it is a task that
/// waits for the done one: the task is not cancelled until the follow-up is, as for any
/// task another waits for.
#[test]
fn a_done_task_whose_follow_up_waits_for_its_acceptance_is_not_cancelled_until_the_follow_up_is() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    w.deliver(result.id);
    let waiting = w
        .follow_up(task, "Tests", &[])
        .expect("a follow-up")
        .task
        .number;
    assert_eq!(w.state(waiting), "open", "it waits for the decision");

    let refused = cancel(&mut w, task, "chief").expect_err("T-2 waits for it");
    assert_eq!(refused.code(), Some("waited-for"));
    assert_eq!(
        refused.to_string(),
        "T-1 cannot be cancelled while T-2 waits for it: accept T-1, or cancel T-2 first",
        "a done task has nothing left to finish: it is accepted or its waiters go first"
    );
    assert_eq!(w.state(task), "done", "nothing of it is touched");
    assert_eq!(w.state(waiting), "open", "and nothing is released");

    cancel(&mut w, waiting, "chief").expect("the follow-up first");
    cancel(&mut w, task, "chief").expect("then the task");
    assert_eq!(w.state(task), "cancelled");
}

/// The report of 2026-11-02: T-6 was done, and T-8 went into its window before T-6 was
/// decided, and was paused. T-6 could be neither sent back (the window holds T-8) nor
/// cancelled (it is done), so the only decision left was to accept work that was never
/// done. A follow-up goes into no window that holds an undecided result now (T-1118), so
/// T-8 cannot go in that way; a task named for the session still can, and the case is
/// that shape: T-1 is done, T-2 is named for its session, goes in and is paused.
#[test]
fn a_done_task_is_cancelled_while_its_window_holds_a_paused_task_and_the_window_goes_on_with_it() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    w.deliver(result.id);
    let session = session_of(&w, task);
    let named = w
        .give_to(&session, &[], "Tests")
        .expect("a task named for the session, which holds nothing");
    let (second, brief) = (named.task.number, named.message.expect("its brief").id);
    w.deliver(brief);
    w.pause(second);
    assert_eq!(w.state(second), "paused");
    let stop = stop_of(&w, &session);

    let refused = w
        .ledger
        .reopen_task(w.project, task, "chief", "Again")
        .expect_err("a window holds one task at a time");
    assert_eq!(refused.code(), Some("session-busy"));

    cancel(&mut w, task, "chief").expect("a done task is called off");

    assert_eq!(w.state(task), "cancelled");
    assert_eq!(w.state(second), "paused", "the paused task is as it was");
    assert!(
        w.ledger.has_task_in_hand(w.id(&session)).expect("a read"),
        "the window still holds it"
    );
    assert_eq!(stop_of(&w, &session), stop, "and is asked nothing more");
    let resumed = w.resume(second, "Go on");
    assert_eq!(
        w.next(&session),
        Some(resumed.message.expect("its words").id),
        "the paused task goes on in the same window"
    );
}

#[test]
fn a_cancel_withdraws_a_result_that_an_older_build_left_queued_for_a_task_sent_back() {
    let mut w = world();
    let (task, result) = finished(&mut w, "zeus", "Parser");
    send_back(&mut w, task);
    // An older build never withdrew it: the task went back to its window with
    // its result still waiting in the chief's queue.
    w.edit(|db| {
        db.execute(
            "UPDATE message SET state = 'queued', reason = NULL WHERE id = ?",
            [result.id],
        )
        .expect("the result is queued again");
    });
    assert_eq!(w.next("chief"), Some(result.id));

    w.ledger
        .cancel_task(w.project, task, "chief")
        .expect("called off");

    assert_eq!(
        fate(&w, result.id),
        (
            "cancelled".to_owned(),
            Some("cancelled by @chief".to_owned())
        )
    );
    assert_eq!(w.next("chief"), None);
}
