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
fn a_task_resumed_after_its_session_ended_returns_the_answer_and_keeps_the_note() {
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
        .unwrap();
    w.ledger.end_session(w.project, &session, "human").unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(
        w.ledger
            .inbox(w.id("chief"), 100)
            .unwrap()
            .iter()
            .filter(|m| m.kind == "note" && m.body.contains("JSON"))
            .count(),
        1
    );
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(w.message(unseen.id).state, "cancelled");
    let moved = w.resume(task, "Go on");
    assert_eq!(moved.message, None);
    let reopened = w.ledger.task(w.project, task).unwrap().unwrap().task;
    assert!(!reopened.body.contains("(answer m-"));
    assert!(reopened.body.contains("Mind the tests"));
    assert_eq!(w.message(note).state, "cancelled");
    let next = w.assign(task, "diana");
    w.deliver(next.message.unwrap().id);
    assert_eq!(w.state(task), "working");
}

#[test]
fn a_release_returns_the_answer_and_tells_what_other_words_waited_for_the_human() {
    let mut w = gated_world();
    let Kept {
        session,
        task,
        answer,
        note,
        ..
    } = kept_for_a_session(&mut w);
    w.ledger.approve_message(answer, "human").unwrap();
    w.ledger
        .release_task(w.project, task, "ran out of quota")
        .unwrap();
    let released = w.ledger.task(w.project, task).unwrap().unwrap().task;
    assert_eq!(released.state, "open");
    assert!(!released.body.contains("(answer m-"));
    assert_eq!(w.states(&[answer, note]), ["cancelled", "cancelled"]);
    let inbox = w.ledger.inbox(w.id("chief"), 100).unwrap();
    assert_eq!(
        inbox
            .iter()
            .filter(|m| m.kind == "note"
                && m.body.contains(
                    "Your words:
JSON"
                ))
            .count(),
        1
    );
    assert!(inbox.iter().any(|m| m.body.contains(&format!(
        "Withdrawn with it, still waiting for the human: m-{note}"
    )) && m.body.contains(&session)));
}

#[test]
fn a_release_during_delivery_returns_the_answer_instead_of_carrying_it() {
    let mut w = gated_world();
    let Kept { task, answer, .. } = kept_for_a_session(&mut w);
    w.ledger.approve_message(answer, "human").unwrap();
    w.begin(answer);
    w.ledger.release_task(w.project, task, "by @human").unwrap();
    let released = w.ledger.task(w.project, task).unwrap().unwrap().task;
    assert!(!released.body.contains("(answer m-"));
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(
        w.ledger.admit(answer, &[]).unwrap(),
        cf_ledger::HandOver::Withdrawn
    );
    assert_eq!(
        w.ledger
            .inbox(w.id("chief"), 100)
            .unwrap()
            .iter()
            .filter(|m| m.body.contains(
                "Your words:
JSON"
            ))
            .count(),
        1
    );
}

#[test]
fn a_gated_answer_stays_for_approval_when_the_session_ends() {
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
        .unwrap();
    w.ledger.end_session(w.project, &session, "human").unwrap();
    assert_eq!(w.message(answer).state, "gated");
    w.resume(task, "Go on");
    let told = w
        .ledger
        .inbox(w.id("chief"), 100)
        .unwrap()
        .into_iter()
        .find(|m| m.body.starts_with("Withdrawn with it"))
        .unwrap();
    assert_eq!(
        told.body,
        format!("Withdrawn with it, still waiting for the human: m-{note}.")
    );
    w.ledger.approve_message(answer, "human").unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(
        w.ledger
            .inbox(w.id("chief"), 100)
            .unwrap()
            .iter()
            .filter(|m| m.body.contains(
                "Your words:
JSON"
            ))
            .count(),
        1
    );
}
