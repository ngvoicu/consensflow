//! Reproductions from the T-1800 review of moved answers.

use crate::fixture::{world, World};
use cf_ledger::{Claim, HandOver};
use serde_json::json;

fn assigned(w: &mut World) -> (i64, String, i64) {
    let task = w.open("Parser").task.number;
    let given = w.assign(task, "zeus");
    let session = given.task.assignee.unwrap();
    let brief = given.message.unwrap().id;
    (task, session, brief)
}

fn notices(w: &World, words: &str) -> Vec<String> {
    w.ledger
        .inbox(w.id("chief"), 100)
        .unwrap()
        .into_iter()
        .filter(|m| m.kind == "note" && m.body.contains(words))
        .map(|m| m.body)
        .collect()
}

#[test]
fn t1800_finding_02_and_t1803_finding_02_accept_reports_a_queued_answer() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    let answer = w.answer(question, "Use JSON").id;
    w.ledger.record_result(w.project, task, "Done").unwrap();
    w.ledger.accept_task(w.project, task, "chief").unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(
        notices(&w, "Use JSON").len(),
        1,
        "the chief must hear the words that did not arrive"
    );
}

#[test]
fn t1800_finding_02_and_t1803_finding_02_accept_cannot_admit_an_answer_already_in_flight() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    let answer = w.answer(question, "Use JSON").id;
    w.begin(answer);
    w.ledger.record_result(w.project, task, "Done").unwrap();
    w.ledger.accept_task(w.project, task, "chief").unwrap();
    assert_ne!(w.ledger.admit(answer, &[]).unwrap(), HandOver::Admitted);
    assert_eq!(notices(&w, "Use JSON").len(), 1);
}

#[test]
fn t1800_finding_04_and_t1803_finding_04_failed_brief_shuts_the_question_door() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.begin(brief);
    // The window has been handed the brief, although its delivery has not
    // yet been confirmed; an unhanded brief does not establish a holding.
    w.ledger.question_brief_handed_over(brief).unwrap();
    let question = w.ask_with_options(&session, task).id;
    let window = w.id(&session);
    w.ledger.give_up_delivery(brief, "window refused").unwrap();
    assert_eq!(w.state(task), "failed");
    assert_eq!(
        w.ledger.claim_answer(question, window).unwrap(),
        Claim::Closed
    );
}

#[test]
fn t1800_finding_04_and_t1803_finding_04_failed_tell_shuts_the_answered_door() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.begin(brief);
    let told = w
        .ledger
        .tell_queued(w.project, task, "chief", "Do this")
        .unwrap()
        .id;
    w.confirm(brief);
    let question = w.ask_with_options(&session, task).id;
    w.choose(question, "red");
    w.begin(told);
    w.ledger.give_up_delivery(told, "window refused").unwrap();
    assert_eq!(w.state(task), "failed");
    assert_eq!(
        w.ledger.claim_answer(question, w.id(&session)).unwrap(),
        Claim::Closed
    );
}

#[test]
fn t1800_finding_05_own_done_then_reopen_in_same_session_keeps_the_question() {
    for reopen_ledger in [false, true] {
        let mut w = world();
        let (task, session, brief) = assigned(&mut w);
        w.deliver(brief);
        let question = w.ask(&session, task, "Which?").id;
        w.ledger.record_result(w.project, task, "Done").unwrap();
        if reopen_ledger {
            w.edit(|_| {});
        }
        let reopened = w
            .ledger
            .reopen_task(w.project, task, "chief", "Go on")
            .unwrap();
        w.deliver(reopened.message.unwrap().id);
        let chief = w.id("chief");
        assert!(
            w.ledger
                .answer(question, chief, Some(&json!("Use JSON")), None)
                .is_ok(),
            "reopen_ledger={reopen_ledger}"
        );
    }
}

#[test]
fn t1800_finding_08_a_deleted_failed_task_is_not_suggested_for_reopen() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    w.ledger.release_task(w.project, task, "by human").unwrap();
    let next = w.assign(task, "diana");
    w.deliver(next.message.unwrap().id);
    w.ledger.fail_task(w.project, task, "failed").unwrap();
    w.ledger.delete_tasks(w.project, &[task]).unwrap();
    let chief = w.id("chief");
    let words = w
        .ledger
        .answer(question, chief, Some(&json!("JSON")), None)
        .unwrap_err()
        .to_string();
    assert!(!words.contains("cf task reopen"), "{words}");
    assert!(words.contains("new task"), "{words}");
}

#[test]
fn review_02_human_cancel_reports_a_queued_answer() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    let answer = w.answer(question, "Use JSON").id;
    w.ledger.cancel_task(w.project, task, "human").unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(notices(&w, "Use JSON").len(), 1);
}

#[test]
fn review_02_answer_approved_after_done_is_reported_at_accept() {
    let mut w = crate::fixture::gated_world();
    let (task, session, brief) = assigned(&mut w);
    w.ledger.approve_message(brief, "human").unwrap();
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    w.ledger.approve_message(question, "human").unwrap();
    let answer = w.answer(question, "Use JSON").id;
    assert_eq!(w.message(answer).state, "gated");
    w.ledger.record_result(w.project, task, "Done").unwrap();
    w.ledger.approve_message(answer, "human").unwrap();
    w.ledger.accept_task(w.project, task, "human").unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(notices(&w, "Use JSON").len(), 1);
}

#[test]
fn an_active_member_removal_returns_the_answer_that_never_arrived() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    let answer = w.answer(question, "Use JSON").id;
    w.ledger.remove_member(w.project, "zeus").unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(notices(&w, "Use JSON").len(), 1);
}

#[test]
fn a_failed_task_returns_the_answer_that_never_arrived() {
    for give_up in [false, true] {
        let mut w = world();
        let (task, session, brief) = assigned(&mut w);
        w.deliver(brief);
        let question = w.ask(&session, task, "Which?").id;
        let answer = w.answer(question, "Use JSON").id;
        if give_up {
            w.ledger
                .give_up_task(w.project, task, "window failed")
                .unwrap();
        } else {
            w.ledger
                .fail_task(w.project, task, "window failed")
                .unwrap();
        }
        assert_eq!(w.message(answer).state, "cancelled");
        assert_eq!(notices(&w, "Use JSON").len(), 1, "give_up={give_up}");
    }
}

#[test]
fn a_nonchief_answerer_is_told_when_an_active_task_fails() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w
        .ledger
        .ask(
            w.project,
            &cf_ledger::NewQuestion {
                from: Some(session),
                to: "diana".into(),
                task: Some(task),
                body: Some("Which?".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
    w.deliver(question);
    let diana = w.id("diana");
    w.ledger
        .answer(question, diana, Some(&json!("Private answer")), None)
        .unwrap();
    w.ledger.fail_task(w.project, task, "failed").unwrap();
    let next = w
        .ledger
        .next_delivery(diana)
        .unwrap()
        .expect("the author is told");
    assert!(next.body.contains("Private answer"));
    assert!(notices(&w, "Private answer").is_empty());
}
