//! What a window kept for a task goes with the task when the window is gone:
//! into the brief of the next one, once, when its session ended or the task
//! was taken back from it.

use crate::fixture::{gated_world, world, World};

/// A task opened for the standard workers and given to a session of zeus,
/// whose brief was received, that asked a question and was answered, and was
/// written to by the chief. Where the project holds hand-offs at the gate, the
/// answer and the note wait there. The session's handle, the task's number,
/// and the ids of the messages.
struct Kept {
    session: String,
    task: i64,
    question: i64,
    answer: i64,
    note: i64,
}

fn kept_for_a_session(w: &mut World) -> Kept {
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    let brief = moved.message.expect("its brief").id;
    if w.message(brief).state == "gated" {
        w.ledger.approve_message(brief, "human").expect("passed on");
    }
    w.deliver(brief);
    let question = w.ask(&session, task, "Which format?");
    let answer = w.answer(question.id, "JSON");
    let note = w.note("chief", &session, task, "Mind the tests");
    Kept {
        session,
        task,
        question: question.id,
        answer: answer.id,
        note: note.id,
    }
}

#[test]
fn a_task_resumed_after_its_session_ended_takes_what_the_session_kept_into_its_brief_once() {
    let mut w = world();
    let Kept {
        session,
        task,
        question,
        answer,
        note,
    } = kept_for_a_session(&mut w);
    let unseen = w.ask(&session, task, "And which version?");
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("the human pauses");
    w.ledger
        .end_session(w.project, &session, "human")
        .expect("a session that holds only a paused task ends");

    let moved = w.resume(task, "Go on");
    assert_eq!(
        moved.message, None,
        "nothing is pasted: the task goes back on the board"
    );
    let reopened = w.ledger.task(w.project, task).unwrap().unwrap().task;
    assert_eq!(reopened.state, "open");
    assert_eq!(
        reopened.body,
        format!(
            "Parser\n\nKept from before, never delivered to @{session}:\n\n(answer m-{answer} from @chief to m-{question} of @{session}: Which format?)\nJSON\n\n(note m-{note} from @chief)\nMind the tests\n\nResumed after a pause, in a fresh window (the one that had it ended; check the working tree for partial changes): Go on"
        ),
        "what it kept, each once, by id, and then the words that resume it"
    );
    for id in [answer, note] {
        assert_eq!(w.message(id).state, "cancelled");
        assert_eq!(
            w.message(id).reason.as_deref(),
            Some("carried into T-1's brief for its next window")
        );
    }
    assert_eq!(
        w.message(unseen.id).reason,
        Some(format!("@{session} no longer has T-1")),
        "what it asked and the chief never saw goes with it"
    );

    // The next window's own questions are the only ones that count.
    let next = w.assign(task, "diana");
    w.deliver(next.message.expect("its brief").id);
    assert_eq!(
        w.state(task),
        "working",
        "the old session's questions do not hold the new brief back"
    );
}

#[test]
fn a_release_takes_what_the_window_kept_into_the_brief_and_says_what_waited_for_the_human() {
    let mut w = gated_world();
    let Kept {
        session,
        task,
        question,
        answer,
        note,
    } = kept_for_a_session(&mut w);
    // The human passed the answer on; the chief's note still waits for them.
    w.ledger
        .approve_message(answer, "human")
        .expect("passed on");
    assert_eq!(w.message(note).state, "gated");

    w.ledger
        .release_task(w.project, task, "ran out of quota")
        .expect("the task is taken back");
    let released = w.ledger.task(w.project, task).unwrap().unwrap().task;
    assert_eq!(released.state, "open");
    assert_eq!(
        released.body,
        format!(
            "Parser\n\nKept from before, never delivered to @{session}:\n\n(answer m-{answer} from @chief to m-{question} of @{session}: Which format?)\nJSON\n\nReassigned from @{session} (ran out of quota); check the working tree for partial changes."
        )
    );
    assert_eq!(w.states(&[answer, note]), ["cancelled", "cancelled"]);
    assert_eq!(
        w.message(note).reason,
        Some(format!("withdrawn: @{session}'s window ended first")),
        "what the human never passed on is withdrawn, not carried"
    );
    let told = w
        .ledger
        .inbox(w.id("chief"), 100)
        .unwrap()
        .into_iter()
        .find(|message| message.body.contains("was taken back"))
        .expect("the requester is told");
    assert_eq!(
        told.body,
        format!(
            "T-{task} was taken back from @{session} (ran out of quota) and waits for another standard worker. Withdrawn with it, still waiting for the human: m-{note}."
        )
    );
}

#[test]
fn a_release_during_a_delivery_has_the_approved_answer_that_was_being_pasted_in_the_brief() {
    let mut w = gated_world();
    let Kept {
        session,
        task,
        question,
        answer,
        ..
    } = kept_for_a_session(&mut w);
    w.ledger
        .approve_message(answer, "human")
        .expect("passed on");
    // Its paste has begun when the task is taken back.
    w.begin(answer);
    assert_eq!(w.message(answer).state, "delivering");

    w.ledger
        .release_task(w.project, task, "by @human")
        .expect("taken back");
    let released = w.ledger.task(w.project, task).unwrap().unwrap().task;
    assert!(
        released.body.contains(&format!(
            "(answer m-{answer} from @chief to m-{question} of @{session}: Which format?)\nJSON\n\nReassigned from"
        )),
        "{}",
        released.body
    );
    assert_eq!(
        w.message(answer).state,
        "cancelled",
        "and is not delivered twice"
    );
}

#[test]
fn whoever_resumed_is_told_what_waited_for_the_human_when_the_session_had_ended() {
    let mut w = gated_world();
    let Kept {
        session,
        task,
        answer,
        note,
        ..
    } = kept_for_a_session(&mut w);
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("the human pauses");
    w.ledger
        .end_session(w.project, &session, "human")
        .expect("ended");
    w.resume(task, "Go on");
    let told = w
        .ledger
        .inbox(w.id("chief"), 100)
        .unwrap()
        .into_iter()
        .find(|message| message.body.starts_with("Withdrawn with it"))
        .expect("the chief, who resumed it, is told");
    assert_eq!(
        told.body,
        format!("Withdrawn with it, still waiting for the human: m-{answer}, m-{note}.")
    );
}
