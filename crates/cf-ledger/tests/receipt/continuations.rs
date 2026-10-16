//! A continuation names the tasks its window goes on with, and whenever it is
//! begun, on its own or riding in another's paste, it names only those its window
//! still holds, and goes no more where it holds none (`messages/continuations.rs`;
//! both reviews of 2026-10-16, round 6).

use cf_ledger::{continuation, MessageView, NewNote};

use crate::fixture::{world, World};

/// T-1 at work in a session of zeus, and T-2 at work there too, as a ledger
/// written before one task per session may have it; and a continuation queued
/// for the session naming both, about T-1.
fn holding_two(w: &mut World) -> MessageView {
    let first = w.open("Lexer").task.number;
    let brief = w.assign(first, "zeus").message.expect("its brief").id;
    w.deliver(brief);
    let session = w
        .ledger
        .task(w.project, first)
        .expect("a read")
        .expect("the task")
        .task
        .assignee
        .expect("its session");
    let second = w.open("Parser").task.number;
    w.hand_to(second, &session);
    w.edit(|db| {
        db.execute(
            "UPDATE task SET state = 'working' WHERE number = ?",
            [second],
        )
        .expect("at work");
    });
    w.ledger
        .note(
            w.project,
            &NewNote {
                from: None,
                to: session,
                task: Some(first),
                body: continuation(&[first, second]),
            },
        )
        .expect("the continuation")
}

#[test]
fn a_continuation_begun_names_only_the_tasks_its_window_still_holds() {
    let mut w = world();
    let note = holding_two(&mut w);
    w.pause(1);

    let stands = w.ledger.as_it_stands(note.id).unwrap().expect("it goes");

    assert_eq!(
        (stands.body.as_str(), stands.task_number),
        (continuation(&[2]).as_str(), Some(2)),
        "about the task it still names"
    );
}

#[test]
fn a_continuation_whose_window_holds_none_of_its_tasks_goes_no_more() {
    let mut w = world();
    let note = holding_two(&mut w);
    w.pause(1);
    w.pause(2);

    assert_eq!(w.ledger.as_it_stands(note.id).unwrap(), None);
    assert_eq!(w.message(note.id).state, "cancelled");
}

#[test]
fn a_continuation_whose_tasks_all_stand_and_any_other_message_are_as_they_were() {
    let mut w = world();
    let note = holding_two(&mut w);
    let other = w.note(
        "chief",
        "zeus",
        1,
        "ConsensFlow stopped while you were working on T-1.",
    );

    assert_eq!(
        w.ledger.as_it_stands(note.id).unwrap(),
        Some(w.message(note.id))
    );
    assert_eq!(w.message(note.id).body, continuation(&[1, 2]));
    assert_eq!(w.ledger.as_it_stands(other.id).unwrap(), Some(other));
}

/// A continuation about T-1 kept while T-1 was paused rides in the words that
/// resume T-1: begun with them, it names only what its window still holds, T-1
/// among it, queued with its words, and not T-2, called off meanwhile.
#[test]
fn a_continuation_riding_in_the_words_that_resume_its_task_names_only_what_its_window_still_holds()
{
    let mut w = world();
    holding_two(&mut w);
    w.pause(1);
    w.ledger
        .cancel_task(w.project, 2, "chief")
        .expect("called off");
    let words = w
        .resume(1, "Go on with the lexer")
        .message
        .expect("its words");

    let begun = w.begin(words.id);

    let carried: Vec<&str> = begun.carried.iter().map(|row| row.body.as_str()).collect();
    assert_eq!(carried, [continuation(&[1]).as_str()]);
}
