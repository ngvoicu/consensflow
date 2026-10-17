//! A continuation tells the window of one task to go on with it, and dies with
//! any change to the task: a pause, a hold, a cancel, a release or an end of it
//! withdraws what its window was not handed, in the write that moves it, and a
//! reassignment never carries it into the next window's brief
//! (`messages/continuations.rs`; the lead's design of 2026-10-17, after both
//! reviews of round 7).

use cf_ledger::{continuation, HandOver, MessageView, NewNote};

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
