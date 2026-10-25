//! A continuation tells the window of one task to go on with it, and dies with
//! any change to the task: a pause, a hold, a cancel, a release or an end of it
//! withdraws what its window was not handed, in the write that moves it, and a
//! reassignment never carries it into the next window's brief
//! (`messages/continuations.rs`; the lead's design of 2026-10-17, after both
//! reviews of round 7).

use cf_ledger::{continuation, GivenUp, HandOver, MessageView, NewNote};

use crate::fixture::{world, World};

/// T-1 at work in a session of zeus, the session, and a continuation queued for
/// it about T-1.
fn continued(w: &mut World) -> (i64, String, MessageView) {
    let number = w.open("Lexer").task.number;
    let brief = w.assign(number, "zeus").message.expect("its brief").id;
    w.deliver(brief);
    let session = w
        .ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .assignee
        .expect("its session");
    let note = w
        .ledger
        .note(
            w.project,
            &NewNote {
                from: None,
                to: session.clone(),
                task: Some(number),
                body: continuation(number),
            },
        )
        .expect("the continuation");
    (number, session, note)
}

/// The state and the reason of message `id`.
fn withdrawn(w: &World, id: i64) -> (String, Option<String>) {
    let message = w.message(id);
    (message.state, message.reason)
}

#[test]
fn every_move_of_its_task_out_of_the_work_withdraws_a_continuation_still_queued_in_the_same_write()
{
    type Move = fn(&mut World, i64);
    let moves: [(&str, Move); 5] = [
        ("paused", |w, number| w.pause(number)),
        ("paused", |w, number| w.hold(number)),
        ("cancelled", |w, number| {
            w.ledger
                .cancel_task(w.project, number, "chief")
                .expect("called off");
        }),
        ("done", |w, number| {
            w.ledger
                .record_result(w.project, number, "Done")
                .expect("its result");
        }),
        ("failed", |w, number| {
            w.ledger
                .fail_task(w.project, number, "the window gave up")
                .expect("failed");
        }),
    ];
    for (to, change) in moves {
        let mut w = world();
        let (number, _, note) = continued(&mut w);

        change(&mut w, number);

        let (state, reason) = withdrawn(&w, note.id);
        assert_eq!(state, "cancelled", "{to}");
        // A cancel and a failure take every message of their task back first.
        if !matches!(to, "cancelled" | "failed") {
            assert_eq!(reason, Some(format!("T-{number} is {to} now")), "{to}");
        }
    }
}

/// An earlier build's continuation, tied to no task, goes with a pause of the task
/// it names to the window that has it; one naming another task stays.
#[test]
fn a_continuation_an_earlier_build_left_tied_to_no_task_goes_with_a_pause_of_the_task_it_names() {
    let mut w = world();
    let (number, session, _) = continued(&mut w);
    let quit = |tasks: &str| {
        of_no_task(
            &format!(
                "ConsensFlow quit while you were working on {tasks}. Go on where you stopped."
            ),
            &session,
        )
    };
    let named = quit(&format!("T-{number}"));
    let other = quit("T-9");
    let named = w
        .ledger
        .note(w.project, &named)
        .expect("an earlier build's");
    let other = w
        .ledger
        .note(w.project, &other)
        .expect("an earlier build's");

    w.pause(number);

    assert_eq!(w.states(&[named.id, other.id]), ["cancelled", "queued"]);
}

/// A note of ConsensFlow's to `to`, about no task.
fn of_no_task(body: &str, to: &str) -> NewNote {
    NewNote {
        from: None,
        to: to.to_owned(),
        task: None,
        body: body.to_owned(),
    }
}

/// One begun and not yet let go by its transport is unsent too, and goes; one
/// its transport let go is the window's, and runs its course.
#[test]
fn a_continuation_only_begun_goes_with_a_pause_and_one_handed_over_stays_with_its_window() {
    let mut w = world();
    let (number, _, note) = continued(&mut w);
    w.begin(note.id);
    w.pause(number);
    assert_eq!(w.message(note.id).state, "cancelled", "only begun");

    let mut w = world();
    let (number, _, note) = continued(&mut w);
    w.begin(note.id);
    assert_eq!(
        w.ledger.admit(note.id, &[]).expect("admitted"),
        HandOver::Admitted
    );
    w.pause(number);
    assert_eq!(w.message(note.id).state, "delivering", "handed over");
}

/// A move within the work (a question asked, and answered) leaves it as it is.
#[test]
fn a_move_of_its_task_within_the_work_leaves_a_continuation_queued() {
    let mut w = world();
    let (number, session, note) = continued(&mut w);
    let question = w.ask(&session, number, "Which dialect?");
    w.deliver(question.id);
    assert_eq!(w.state(number), "waiting");

    assert_eq!(w.message(note.id).state, "queued");
}

/// Astra's review of 2026-10-17: a continuation handed over, its task paused while
/// the paste waited for its reply, came back after a refusal with nothing written,
/// and a reader read a word to go on with a task paused since. One that comes back
/// goes on only while its task is at work: it is withdrawn in the write that sends
/// it back, and in the one that gives it up, which fails nothing.
#[test]
fn a_continuation_handed_over_that_comes_back_after_its_task_moved_is_withdrawn() {
    for given_up in [false, true] {
        let mut w = world();
        let (number, _, note) = continued(&mut w);
        w.begin(note.id);
        w.ledger.admit(note.id, &[]).expect("admitted");
        w.pause(number);
        assert_eq!(
            w.message(note.id).state,
            "delivering",
            "handed over: it runs its course"
        );

        if given_up {
            let (_, came_to) = w
                .ledger
                .give_up_delivery(note.id, "the window never showed it")
                .expect("given up");
            // The merge of main's field 3.0.6 (T-392): said as withdrawn, so the
            // engine tells nobody of a failure.
            assert_eq!(came_to, GivenUp::Withdrawn, "said as withdrawn");
        } else {
            w.ledger
                .retry_delivery(note.id, "the paste wrote nothing", true)
                .expect("tried again");
        }

        assert_eq!(
            withdrawn(&w, note.id),
            (
                "cancelled".to_owned(),
                Some(format!("T-{number} is paused now"))
            ),
            "given up {given_up}"
        );
    }
}

/// One whose task is still at work goes back to its queue as any message does.
#[test]
fn a_continuation_that_comes_back_while_its_task_is_at_work_goes_again() {
    let mut w = world();
    let (_, _, note) = continued(&mut w);
    w.begin(note.id);
    w.ledger.admit(note.id, &[]).expect("admitted");

    w.ledger
        .retry_delivery(note.id, "the paste wrote nothing", true)
        .expect("tried again");

    assert_eq!(w.message(note.id).state, "queued");
}

/// The transport's admission asks it too, of a continuation however it came to
/// be begun with its task not at work (an earlier build moved the task and left
/// it): it is withdrawn, and nothing goes in.
#[test]
fn a_continuation_whose_task_is_not_at_work_is_withdrawn_by_the_transport_s_admission() {
    // Its task paused under it, taken to the board, or named with another, as a
    // build before 3.0.5 wrote it.
    let ways: [(&str, &str, &str); 3] = [
        ("paused", "UPDATE task SET state = 'paused' WHERE number = 1", "T-1 is paused now"),
        (
            "taken back",
            "UPDATE task SET state = 'open', assignee_id = NULL WHERE number = 1",
            "T-1 went from its window",
        ),
        (
            "named with another",
            "UPDATE message SET body = 'ConsensFlow quit while you were working on T-1, T-2. Go on where you stopped.'
               WHERE kind = 'note' AND sender_id IS NULL",
            "it names several tasks",
        ),
    ];
    for (way, sql, reason) in ways {
        let mut w = world();
        let (number, _, note) = continued(&mut w);
        assert_eq!(number, 1);
        w.begin(note.id);
        w.ledger
            .write_raw(sql)
            .expect("as an earlier build left it");

        assert_eq!(
            w.ledger.admit(note.id, &[]).expect("asked"),
            HandOver::Withdrawn,
            "{way}"
        );
        assert_eq!(
            withdrawn(&w, note.id),
            ("cancelled".to_owned(), Some(reason.to_owned())),
            "{way}"
        );
    }
}

/// One withdrawn already as its transport asks (its task cancelled while the paste
/// waited) is left as it was: the cancel's words stand, and nothing is written.
#[test]
fn a_continuation_withdrawn_already_is_left_as_it_was_by_the_transport_s_admission() {
    let mut w = world();
    let (number, _, note) = continued(&mut w);
    w.begin(note.id);
    w.ledger
        .cancel_task(w.project, number, "chief")
        .expect("called off");
    let before = withdrawn(&w, note.id);
    assert_eq!(before.0, "cancelled");

    assert_eq!(
        w.ledger.admit(note.id, &[]).expect("asked"),
        HandOver::Withdrawn
    );
    assert_eq!(withdrawn(&w, note.id), before);
}

/// As an earlier build could leave it: T-1's continuation riding in a task message
/// of T-1's for its window (the words a hold's end resumes it with), the carrier
/// `carrier` and the continuation `riding`, and T-1 paused under them by no write
/// of this build's: the carrier, and the continuation.
fn riding_with_its_task_paused(w: &mut World, carrier: &str, riding: &str) -> (i64, i64) {
    let (number, session, note) = continued(w);
    let window = w.id(&session);
    w.ledger
        .write_raw(&format!(
            "INSERT INTO message (project_id, recipient_id, kind, task_id, body, state, attempts, created_at)
               SELECT project_id, {window}, 'task', id, 'Resumed: Go on where you stopped.', '{carrier}', 1, created_at
               FROM task WHERE number = {number};
             UPDATE message SET carried_by = last_insert_rowid(), state = '{riding}' WHERE id = {note};
             UPDATE task SET state = 'paused' WHERE number = {number};",
            note = note.id
        ))
        .expect("as an earlier build left it");
    let words = w
        .ledger
        .inbox(window, 100)
        .expect("its inbox")
        .into_iter()
        .find(|message| message.kind == "task" && message.body.starts_with("Resumed: "))
        .expect("the carrier")
        .id;
    (words, note.id)
}

/// What a carrier carries comes back with it, or is let go of by it: a
/// continuation among it whose task is not at work is withdrawn in that write,
/// whichever way it came back: its carrier's admission refused, its carrier tried
/// again (as the start settles one on its way when the app quit, which keeps its
/// set), given up, taken back out of its window, or let go of at a start (left
/// queued under a carrier an earlier build delivered).
#[test]
fn a_continuation_riding_in_a_carrier_that_comes_back_is_withdrawn_unless_its_task_is_at_work() {
    type Back = fn(&mut World, i64, i64);
    let ways: [(&str, &str, &str, Back); 5] = [
        ("admitted", "delivering", "queued", |w, words, note| {
            assert_eq!(
                w.ledger.admit(words, &[note]).expect("asked"),
                HandOver::Withdrawn
            );
        }),
        ("tried again", "delivering", "queued", |w, words, _| {
            w.ledger
                .retry_delivery(words, "the paste wrote nothing", true)
                .expect("tried again");
        }),
        ("given up", "delivering", "queued", |w, words, _| {
            w.ledger
                .fail_delivery(words, "the window never showed it")
                .expect("given up");
        }),
        ("taken back", "delivered", "delivered", |w, words, _| {
            w.ledger
                .take_back(words, "the window took it back")
                .expect("taken back");
        }),
        ("let go of at a start", "delivered", "queued", |w, _, _| {
            w.edit(|_| {});
        }),
    ];
    for (way, carrier, riding, back) in ways {
        let mut w = world();
        let (words, note) = riding_with_its_task_paused(&mut w, carrier, riding);

        back(&mut w, words, note);

        assert_eq!(w.message(note).state, "cancelled", "{way}");
    }
}

/// Both reviews of 2026-10-16, round 7: a continuation still on its way went
/// into the next window's brief when its task was reassigned, telling a window
/// that never began the turn to go on with it. It goes with the window, handed
/// to it or not, and the brief says nothing of it.
#[test]
fn a_reassignment_never_carries_a_continuation_into_the_next_brief() {
    for handed in [false, true] {
        let mut w = world();
        let (number, _, note) = continued(&mut w);
        if handed {
            w.begin(note.id);
            w.ledger.admit(note.id, &[]).expect("admitted");
        }

        w.ledger
            .release_task(w.project, number, "the chief takes it back")
            .expect("back on the board");

        let task = w
            .ledger
            .task(w.project, number)
            .expect("a read")
            .expect("the task")
            .task;
        assert!(
            !task
                .body
                .contains("ConsensFlow stopped while you were working on"),
            "handed {handed}: {}",
            task.body
        );
        assert_eq!(
            withdrawn(&w, note.id),
            (
                "cancelled".to_owned(),
                Some(format!("T-{number} went from its window"))
            ),
            "handed {handed}"
        );
    }
}
