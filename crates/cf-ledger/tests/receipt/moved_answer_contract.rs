//! Reproductions of the review findings under the T-1813 holding contract.
use crate::fixture::{gated_world, world, World};
use cf_ledger::{Claim, NewQuestion};
use serde_json::json;

fn assigned(w: &mut World, gated: bool) -> (i64, String, i64) {
    let task = w.open("Review fixture").task.number;
    let given = w.assign(task, "zeus");
    let brief = given.message.unwrap().id;
    if gated {
        w.ledger.approve_message(brief, "human").unwrap();
    }
    (task, given.task.assignee.unwrap(), brief)
}

fn word_notices(w: &World, words: &str) -> usize {
    w.ledger
        .inbox(w.id("chief"), 100)
        .unwrap()
        .iter()
        .filter(|m| m.kind == "note" && m.body.contains(words))
        .count()
}

#[test]
fn t1813_pi_inbox_send_changes_holding_before_extension_takes_it() {
    let mut w = world();
    let (previous, session, first) = assigned(&mut w, false);
    w.deliver(first);
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let next = w.give_to(&session, &[], "Next Pi task").unwrap();
    let brief = next.message.unwrap().id;
    w.begin(brief);
    assert_eq!(
        w.ledger
            .question_holding(w.id(&session))
            .unwrap()
            .unwrap()
            .task_number,
        previous
    );
    w.ledger.question_brief_handed_over(brief).unwrap();
    assert_eq!(
        w.ledger
            .question_holding(w.id(&session))
            .unwrap()
            .unwrap()
            .task_number,
        next.task.number
    );
    let question = w
        .ask(&session, next.task.number, "Pi asks after the send")
        .id;
    let answer = w.answer(question, "Pi's full answer").id;
    w.ledger
        .cancel_task(w.project, next.task.number, "human")
        .unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Pi's full answer"), 1);
}

#[test]
fn t1813_pi_refusal_after_send_rebinds_to_the_previous_holding() {
    let mut w = world();
    let (previous, session, first) = assigned(&mut w, false);
    w.deliver(first);
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let next = w.give_to(&session, &[], "Next Pi task").unwrap();
    let brief = next.message.unwrap().id;
    w.begin(brief);
    w.ledger.question_brief_handed_over(brief).unwrap();
    let question = w.ask(&session, next.task.number, "Pi asks during send").id;
    let answer = w.answer(question, "Words after the send").id;
    w.ledger.question_brief_refused(brief).unwrap();
    assert_eq!(
        w.ledger
            .question_holding(w.id(&session))
            .unwrap()
            .unwrap()
            .task_number,
        previous
    );
    assert_eq!(w.message(answer).task_number, Some(previous));
    assert_eq!(word_notices(&w, "Words after the send"), 0);
}

#[test]
fn t1813_approval_during_a_brief_send_waits_for_its_result() {
    for succeeds in [false, true] {
        let mut w = gated_world();
        let (previous, session, first) = assigned(&mut w, true);
        w.deliver(first);
        let question = w.ask(&session, previous, "Question before the send").id;
        w.ledger.approve_message(question, "human").unwrap();
        w.deliver(question);
        let answer = w.answer(question, "A gated full answer").id;
        w.ledger.record_result(w.project, previous, "Done").unwrap();
        let next = w.give_to(&session, &[], "Next task").unwrap();
        let brief = next.message.unwrap().id;
        w.ledger.approve_message(brief, "human").unwrap();
        w.begin(brief);
        w.ledger.question_brief_handed_over(brief).unwrap();
        w.ledger.approve_message(answer, "human").unwrap();
        assert_eq!(w.message(answer).state, "queued");
        assert_eq!(word_notices(&w, "A gated full answer"), 0);
        if succeeds {
            w.ledger.question_brief_reported(brief).unwrap();
            assert_eq!(w.message(answer).state, "cancelled");
            assert_eq!(word_notices(&w, "A gated full answer"), 1);
        } else {
            w.ledger.retry_delivery(brief, "refused", true).unwrap();
            assert_ne!(w.message(answer).state, "cancelled");
            assert_eq!(word_notices(&w, "A gated full answer"), 0);
        }
    }
}

#[test]
fn t1813_previous_task_ending_during_a_send_returns_its_answer_once() {
    let mut w = world();
    let (previous, session, first) = assigned(&mut w, false);
    w.deliver(first);
    let question = w.ask(&session, previous, "Question before the send").id;
    let answer = w.answer(question, "Answer of the previous task").id;
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let next = w.give_to(&session, &[], "Next task").unwrap();
    let brief = next.message.unwrap().id;
    w.begin(brief);
    w.ledger.question_brief_handed_over(brief).unwrap();
    w.ledger.accept_task(w.project, previous, "human").unwrap();
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Answer of the previous task"), 1);
    w.ledger.retry_delivery(brief, "refused", true).unwrap();
    assert_eq!(word_notices(&w, "Answer of the previous task"), 1);
}

#[test]
fn t1813_a_refused_brief_keeps_existing_questions_of_the_previous_holding() {
    let mut w = world();
    let (previous, session, first) = assigned(&mut w, false);
    w.deliver(first);
    let question = w.ask(&session, previous, "Question before the send").id;
    let answer = w.answer(question, "Previous holding's answer").id;
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let next = w.give_to(&session, &[], "Next task").unwrap();
    let brief = next.message.unwrap().id;
    w.begin(brief);
    w.ledger.question_brief_handed_over(brief).unwrap();
    assert!(matches!(
        w.ledger.claim_answer(question, w.id(&session)).unwrap(),
        cf_ledger::Claim::Waiting
    ));
    w.ledger.retry_delivery(brief, "refused", true).unwrap();
    assert_eq!(
        w.ledger
            .question_holding(w.id(&session))
            .unwrap()
            .unwrap()
            .task_number,
        previous
    );
    assert_ne!(w.message(question).state, "cancelled");
    assert_ne!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Previous holding's answer"), 0);
    assert_eq!(
        w.ledger
            .next_delivery_of(w.id(&session), answer)
            .unwrap()
            .map(|m| m.id),
        Some(answer)
    );
}

#[test]
fn t1813_a_successful_brief_closes_the_previous_holdings_questions_once() {
    let mut w = world();
    let (previous, session, first) = assigned(&mut w, false);
    w.deliver(first);
    let question = w.ask(&session, previous, "Question before the send").id;
    let answer = w.answer(question, "Previous holding's answer").id;
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let next = w.give_to(&session, &[], "Next task").unwrap();
    let brief = next.message.unwrap().id;
    w.begin(brief);
    w.ledger.question_brief_handed_over(brief).unwrap();
    assert_eq!(word_notices(&w, "Previous holding's answer"), 0);
    w.ledger.question_brief_reported(brief).unwrap();
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Previous holding's answer"), 1);
    w.ledger.question_brief_reported(brief).unwrap();
    assert_eq!(word_notices(&w, "Previous holding's answer"), 1);
}

#[test]
fn t1813_a_refused_brief_rebinds_its_questions_and_answers_to_the_previous_holding() {
    let mut w = world();
    let (previous, session, first) = assigned(&mut w, false);
    w.deliver(first);
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let next = w.give_to(&session, &[], "Next task").unwrap();
    let brief = next.message.unwrap().id;
    w.begin(brief);
    w.ledger.question_brief_handed_over(brief).unwrap();
    let question = w
        .ask(&session, next.task.number, "Question during the send")
        .id;
    let answer = w.answer(question, "Words to keep").id;
    w.ledger
        .retry_delivery(brief, "the pane refused the write", true)
        .unwrap();
    let holding = w.ledger.question_holding(w.id(&session)).unwrap().unwrap();
    assert_eq!((holding.task_number, holding.active), (previous, true));
    assert_ne!(w.message(question).state, "cancelled");
    assert_ne!(w.message(answer).state, "cancelled");
    assert_eq!(w.message(answer).task_number, Some(previous));
    assert_eq!(word_notices(&w, "Words to keep"), 0);
    w.ledger.accept_task(w.project, previous, "human").unwrap();
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Words to keep"), 1);
}

#[test]
fn t1813_a_refused_brief_returns_its_answer_if_the_previous_holding_ended() {
    let mut w = world();
    let (previous, session, first) = assigned(&mut w, false);
    w.deliver(first);
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let next = w
        .ledger
        .create_task(
            w.project,
            &cf_ledger::NewTask {
                from: "chief".into(),
                after: Some(previous),
                body: "Follow-up".into(),
                ..Default::default()
            },
        )
        .unwrap();
    w.ledger.accept_task(w.project, previous, "human").unwrap();
    let brief = w
        .ledger
        .task(w.project, next.task.number)
        .unwrap()
        .unwrap()
        .messages
        .into_iter()
        .find(|m| m.kind == "task")
        .unwrap()
        .id;
    w.begin(brief);
    w.ledger.question_brief_handed_over(brief).unwrap();
    let question = w
        .ask(&session, next.task.number, "Question during the send")
        .id;
    let answer = w.answer(question, "Words for the chief").id;
    w.ledger
        .retry_delivery(brief, "the pane refused the write", true)
        .unwrap();
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Words for the chief"), 1);
    let notice = w
        .ledger
        .inbox(w.id("chief"), 100)
        .unwrap()
        .into_iter()
        .find(|m| m.kind == "note" && m.body.contains("Words for the chief"))
        .unwrap();
    assert!(
        notice.body.contains(&format!("T-{previous}")),
        "{}",
        notice.body
    );
}

#[test]
fn t1803_finding_05_own_done_keeps_a_gated_question_gated() {
    let mut w = gated_world();
    let (task, session, brief) = assigned(&mut w, true);
    w.deliver(brief);
    let question = w.ask(&session, task, "Question at the gate").id;
    assert_eq!(w.message(question).state, "gated");
    w.ledger.record_result(w.project, task, "Result").unwrap();
    assert_eq!(
        w.message(question).state,
        "gated",
        "worker done does not end the holding"
    );
    w.edit(|_| {});
    let reopen = w
        .ledger
        .reopen_task(w.project, task, "human", "Continue")
        .unwrap()
        .message
        .unwrap()
        .id;
    if w.message(reopen).state == "gated" {
        w.ledger.approve_message(reopen, "human").unwrap();
    }
    w.deliver(reopen);
    assert_eq!(w.message(question).state, "gated");
    w.ledger.approve_message(question, "human").unwrap();
}

#[test]
fn t1803_finding_06_taskless_plain_answer_is_dispatchable() {
    for gated in [false, true] {
        let mut w = if gated { gated_world() } else { world() };
        let (task, session, brief) = assigned(&mut w, gated);
        w.deliver(brief);
        w.ledger.record_result(w.project, task, "Result").unwrap();
        w.ledger.accept_task(w.project, task, "human").unwrap();
        let question = w
            .ledger
            .ask(
                w.project,
                &NewQuestion {
                    from: Some(session.clone()),
                    to: "chief".into(),
                    body: Some("Idle question".into()),
                    ..NewQuestion::default()
                },
            )
            .unwrap()
            .id;
        if gated {
            w.ledger.approve_message(question, "human").unwrap();
        }
        w.deliver(question);
        let answer = w.answer(question, "Idle answer").id;
        if gated {
            w.ledger.approve_message(answer, "human").unwrap();
        }
        assert_eq!(
            w.ledger
                .next_delivery_of(w.id(&session), answer)
                .unwrap()
                .map(|m| m.id),
            Some(answer),
            "gate={gated}"
        );
    }
}

#[test]
fn t1800_finding_07_and_t1803_finding_07_open_does_not_retire_or_report_old_rows() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Old question").id;
    w.deliver(question);
    let old_state = w.message(question).state;
    // A pre-alpha.11 state: a moved task with a question left at its old window.
    let task_id = w.ledger.task(w.project, task).unwrap().unwrap().task.id;
    let other = w.id("diana");
    w.edit(|db| {
        db.execute(
            "UPDATE task SET assignee_id = ?1 WHERE id = ?2",
            [other, task_id],
        )
        .unwrap();
    });
    assert_eq!(w.message(question).state, old_state);
    assert_eq!(word_notices(&w, "conflict"), 0);
    w.edit(|_| {});
    assert_eq!(w.message(question).state, old_state);
}

#[test]
fn t1803_finding_08_deleted_failed_question_never_suggests_reopen() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Still needed?").id;
    w.deliver(question);
    w.ledger
        .give_up_task(w.project, task, "Window ended")
        .unwrap();
    w.ledger.delete_tasks(w.project, &[task]).unwrap();
    let error = w
        .ledger
        .answer(question, w.id("chief"), Some(&json!("Reply")), None)
        .unwrap_err()
        .to_string();
    assert!(!error.contains("cf task reopen"), "{error}");
}

#[test]
fn t1803_finding_08_failed_task_does_not_prescribe_reopen_to_a_busy_session() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which file?").id;
    w.deliver(question);
    w.ledger.give_up_task(w.project, task, "Stopped").unwrap();
    let next = w.give_to(&session, &[], "Other work").unwrap();
    w.deliver(next.message.unwrap().id);
    assert!(w
        .ledger
        .reopen_task(w.project, task, "chief", "Continue")
        .is_err());
    let refusal = w
        .ledger
        .answer(question, w.id("chief"), Some(&json!("Full reply")), None)
        .unwrap_err()
        .to_string();
    assert!(!refusal.contains("cf task reopen"), "{refusal}");
}

#[test]
fn t1803_finding_08_a_moved_failed_task_with_a_departed_member_has_usable_advice() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Original question").id;
    w.deliver(question);
    w.ledger.release_task(w.project, task, "by human").unwrap();
    let next = w.assign(task, "diana");
    w.deliver(next.message.unwrap().id);
    w.ledger
        .give_up_task(w.project, task, "Window ended")
        .unwrap();
    w.ledger.remove_member(w.project, "diana").unwrap();
    let attempt = w.ledger.reopen_task(w.project, task, "chief", "Continue");
    assert!(attempt.is_err());
    let refusal = w
        .ledger
        .answer(question, w.id("chief"), Some(&json!("Reply")), None)
        .unwrap_err()
        .to_string();
    assert!(!refusal.contains("cf task reopen"), "{refusal}");
}

#[test]
fn t1803_finding_09_failed_tell_returns_answer_before_deletion() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.begin(brief);
    let tell = w
        .ledger
        .tell_queued(w.project, task, "chief", "Do this")
        .unwrap()
        .id;
    w.confirm(brief);
    let question = w.ask_with_options(&session, task).id;
    let answer = w.choose(question, "red").id;
    let words = w.message(answer).body;
    w.begin(tell);
    w.ledger.give_up_delivery(tell, "Delivery gave up").unwrap();
    assert_eq!(w.state(task), "failed");
    assert_eq!(
        w.ledger.claim_answer(question, w.id(&session)).unwrap(),
        Claim::Closed
    );
    assert_eq!(
        word_notices(&w, &words),
        1,
        "failure ends the holding and returns the full answer"
    );
    w.ledger.delete_tasks(w.project, &[task]).unwrap();
    w.edit(|_| {});
    assert_eq!(word_notices(&w, &words), 1);
}

#[test]
fn t1800_finding_03_and_t1803_finding_03_removal_returns_approved_answer_to_chief_once() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which file?").id;
    w.deliver(question);
    w.pause(task);
    let answer = w.answer(question, "Full answer on removal").id;
    w.ledger.remove_member(w.project, "zeus").unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Full answer on removal"), 1);
    w.edit(|_| {});
    assert_eq!(word_notices(&w, "Full answer on removal"), 1);
}

#[test]
fn t1803_finding_02_gated_answer_waits_for_approval_after_acceptance() {
    let mut w = gated_world();
    let (task, session, brief) = assigned(&mut w, true);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which file?").id;
    w.ledger.approve_message(question, "human").unwrap();
    w.deliver(question);
    let answer = w.answer(question, "The approved exact words").id;
    w.ledger.record_result(w.project, task, "Result").unwrap();
    w.ledger.accept_task(w.project, task, "human").unwrap();
    assert_eq!(w.message(answer).state, "gated");
    assert_eq!(word_notices(&w, "The approved exact words"), 0);
    w.ledger.approve_message(answer, "human").unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "The approved exact words"), 1);
    w.edit(|_| {});
    assert_eq!(word_notices(&w, "The approved exact words"), 1);
}

#[test]
fn t1800_finding_06_taskless_question_closes_with_its_window() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    w.ledger.record_result(w.project, task, "Result").unwrap();
    w.ledger.accept_task(w.project, task, "human").unwrap();
    let question = w
        .ledger
        .ask(
            w.project,
            &NewQuestion {
                from: Some(session.clone()),
                to: "chief".into(),
                body: Some("Idle question".into()),
                ..NewQuestion::default()
            },
        )
        .unwrap()
        .id;
    w.deliver(question);
    let answer = w.answer(question, "Idle answer at window end").id;
    w.ledger
        .close_question_window(w.id(&session), None)
        .unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Idle answer at window end"), 1);
    assert_eq!(
        w.ledger.admit(answer, &[]).unwrap(),
        cf_ledger::HandOver::Withdrawn
    );
}

#[test]
fn t1800_finding_05_question_and_answer_keep_the_holding_after_own_done() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Still a question").id;
    w.deliver(question);
    w.ledger.record_result(w.project, task, "Result").unwrap();
    let answer = w.answer(question, "Still for this window").id;
    assert_eq!(
        w.ledger
            .next_delivery_of(w.id(&session), answer)
            .unwrap()
            .map(|m| m.id),
        Some(answer)
    );
    w.begin(answer);
    assert_eq!(
        w.ledger.admit(answer, &[]).unwrap(),
        cf_ledger::HandOver::Admitted
    );
}

#[test]
fn reassign_returns_answer_instead_of_giving_it_to_another_window() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which file?").id;
    w.deliver(question);
    let answer = w.answer(question, "Only the first window asked this").id;
    w.ledger.release_task(w.project, task, "Reassign").unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Only the first window asked this"), 1);
    let new_brief = w.assign(task, "diana").message.unwrap();
    assert!(!new_brief.body.contains("Only the first window asked this"));
}

#[test]
fn t1803_findings_02_and_03_gate_survives_reassignment_failure_and_removal() {
    for ending in ["reassign", "failure", "removal"] {
        let mut w = gated_world();
        let (task, session, brief) = assigned(&mut w, true);
        w.deliver(brief);
        let question = w.ask(&session, task, "Which file?").id;
        w.ledger.approve_message(question, "human").unwrap();
        w.deliver(question);
        if ending == "removal" {
            w.pause(task);
        }
        let answer = w.answer(question, "Approval must decide these words").id;
        assert_eq!(w.message(answer).state, "gated");
        match ending {
            "reassign" => {
                w.ledger.release_task(w.project, task, "Reassign").unwrap();
            }
            "failure" => {
                w.ledger
                    .give_up_task(w.project, task, "Window failed")
                    .unwrap();
            }
            _ => {
                w.ledger.remove_member(w.project, "zeus").unwrap();
            }
        }
        assert_eq!(w.message(answer).state, "gated", "{ending}");
        assert_eq!(
            word_notices(&w, "Approval must decide these words"),
            0,
            "{ending}"
        );
        w.edit(|_| {});
        w.ledger.approve_message(answer, "human").unwrap();
        assert_eq!(w.message(answer).state, "cancelled", "{ending}");
        assert_eq!(
            word_notices(&w, "Approval must decide these words"),
            1,
            "{ending}"
        );
        w.edit(|_| {});
        assert_eq!(
            word_notices(&w, "Approval must decide these words"),
            1,
            "{ending}"
        );
    }
}

#[test]
fn t1803_finding_02_interrupted_answer_after_acceptance_is_not_retried_or_reported_twice() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    let answer = w.answer(question, "Interrupted exact answer").id;
    w.begin(answer);
    w.ledger.record_result(w.project, task, "Result").unwrap();
    w.ledger.accept_task(w.project, task, "human").unwrap();
    assert_eq!(
        w.ledger.admit(answer, &[]).unwrap(),
        cf_ledger::HandOver::Withdrawn
    );
    w.edit(|_| {});
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Interrupted exact answer"), 1);
    assert_eq!(w.next(&session), None);
}

#[test]
fn t1803_finding_05_a_question_already_in_flight_survives_own_done_and_reopen() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Question in flight").id;
    w.begin(question);
    let result = w
        .ledger
        .record_result(w.project, task, "Result")
        .unwrap()
        .message
        .unwrap()
        .id;
    assert_eq!(w.message(question).state, "delivering");
    w.ledger
        .retry_delivery(question, "Interrupted by restart", true)
        .unwrap();
    w.edit(|_| {});
    let reopen = w
        .ledger
        .reopen_task(w.project, task, "human", "Continue")
        .unwrap()
        .message
        .unwrap()
        .id;
    w.deliver(reopen);
    assert!(w.next("chief") == Some(question) || w.next("chief") == Some(result));
    assert_eq!(w.message(question).state, "queued");
}

#[test]
fn t1803_finding_05_reopen_does_not_restore_a_second_delivery_over_the_chiefs_result() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w
        .ask(&session, task, "Question in the chief's transport")
        .id;
    w.begin(question);
    let result = w
        .ledger
        .record_result(w.project, task, "Result")
        .unwrap()
        .message
        .unwrap()
        .id;
    assert_eq!(w.message(question).state, "delivering");
    w.ledger
        .retry_delivery(question, "Interrupted", true)
        .unwrap();
    w.begin(result);
    let reopened = w.ledger.reopen_task(w.project, task, "human", "Continue");
    assert!(
        reopened.is_ok(),
        "reopen while the result is in flight: {reopened:?}"
    );
    assert_eq!(w.message(question).state, "queued");
    assert_eq!(w.message(result).state, "delivering");
}

#[test]
fn t1803_finding_07_alpha10_messages_are_unchanged_by_upgrade_and_reopening() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Old question").id;
    w.deliver(question);
    let answer = w.answer(question, "Old queued words").id;
    let before = (w.message(question), w.message(answer));
    w.edit(|db| {
        db.execute("DELETE FROM question_binding", []).unwrap();
        db.execute("DELETE FROM question_holding", []).unwrap();
    });
    assert_eq!((w.message(question), w.message(answer)), before);
    w.edit(|_| {});
    assert_eq!((w.message(question), w.message(answer)), before);
    assert_eq!(word_notices(&w, "Old queued words"), 0);
}

#[test]
fn an_alpha10_brief_is_not_rebuilt_into_a_holding_on_open() {
    let mut w = world();
    let (_task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    w.edit(|db| {
        db.execute("DELETE FROM question_holding", []).unwrap();
    });
    assert_eq!(w.ledger.question_holding(w.id(&session)).unwrap(), None);
    let question = w
        .ledger
        .ask(
            w.project,
            &NewQuestion {
                from: Some(session),
                to: "chief".into(),
                body: Some("New idle question".into()),
                ..NewQuestion::default()
            },
        )
        .unwrap();
    assert_eq!(question.task_number, None);
}

#[test]
fn an_ended_paused_session_returns_the_answer_before_resume() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which file?").id;
    let answer = w.answer(question, "Words before session ended").id;
    w.pause(task);
    w.ledger.end_session(w.project, &session, "human").unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Words before session ended"), 1);
}

#[test]
fn a_different_delivered_brief_ends_the_previous_done_tasks_holding() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Before done?").id;
    w.deliver(question);
    let answer = w.answer(question, "Full answer before next task").id;
    w.ledger.record_result(w.project, task, "Done").unwrap();
    assert_ne!(w.message(answer).state, "cancelled", "done alone keeps it");
    let next = w.give_to(&session, &[], "Another task").unwrap();
    w.deliver(next.message.unwrap().id);
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Full answer before next task"), 1);
    assert_eq!(
        w.ledger
            .question_holding(w.id(&session))
            .unwrap()
            .unwrap()
            .task_number,
        next.task.number
    );
}

#[test]
fn t1803_finding_07_a_question_about_another_task_follows_the_asking_windows_holding() {
    for deliver_answer in [false, true] {
        let mut w = world();
        let (task, session, brief) = assigned(&mut w, false);
        w.deliver(brief);
        let other = w.open("Other worker's task").task.number;
        let other_brief = w.assign(other, "diana").message.unwrap().id;
        w.deliver(other_brief);
        let question = w
            .ledger
            .ask(
                w.project,
                &NewQuestion {
                    from: Some(session.clone()),
                    to: "chief".into(),
                    task: Some(other),
                    body: Some("About the other task".into()),
                    ..NewQuestion::default()
                },
            )
            .expect("a window may ask about another task")
            .id;
        w.deliver(question);
        let answer = w.answer(question, "Full cross-task answer").id;
        assert_eq!(
            w.ledger
                .next_delivery_of(w.id(&session), answer)
                .unwrap()
                .map(|m| m.id),
            Some(answer)
        );
        if deliver_answer {
            w.deliver(answer);
            assert_eq!(w.message(answer).state, "delivered");
        } else {
            w.ledger
                .give_up_task(w.project, task, "Window failed")
                .unwrap();
            assert_eq!(w.message(answer).state, "cancelled");
            assert_eq!(word_notices(&w, "Full cross-task answer"), 1);
            assert_eq!(w.state(other), "working");
        }
    }
}

#[test]
fn t1803_finding_07_a_current_holding_can_ask_about_its_previous_task() {
    let mut w = world();
    let (previous, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    w.ledger.accept_task(w.project, previous, "human").unwrap();
    let current = w.give_to(&session, &[], "Next task").unwrap();
    w.deliver(current.message.unwrap().id);
    let question = w
        .ledger
        .ask(
            w.project,
            &NewQuestion {
                from: Some(session.clone()),
                to: "chief".into(),
                task: Some(previous),
                body: Some("About the previous task".into()),
                ..NewQuestion::default()
            },
        )
        .expect("a question from the current holding may discuss the previous task")
        .id;
    w.deliver(question);
    let answer = w.answer(question, "Answer for this window").id;
    assert_eq!(
        w.ledger
            .next_delivery_of(w.id(&session), answer)
            .unwrap()
            .map(|m| m.id),
        Some(answer)
    );
    w.ledger
        .give_up_task(w.project, current.task.number, "Window ended")
        .unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(word_notices(&w, "Answer for this window"), 1);
}

#[test]
fn a13_a_question_about_an_accepted_topic_still_reminds_for_its_current_holding() {
    use cf_ledger::{HandOver, NewNote};
    let mut w = world();
    let (previous, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    w.ledger.accept_task(w.project, previous, "human").unwrap();
    let current = w.give_to(&session, &[], "Next task").unwrap();
    w.deliver(current.message.unwrap().id);
    let question = w.ask(&session, previous, "About the previous task").id;
    w.deliver(question);
    assert_eq!(w.state(current.task.number), "waiting");
    assert_eq!(w.message(question).state, "delivered");

    let waits = w.ledger.waiting_questions(w.project).unwrap();
    let wait = waits
        .iter()
        .find(|wait| wait.id == question)
        .expect("the question's current holding is still waiting");
    assert_eq!(wait.held_task, Some(current.task.number));
    assert_eq!(
        wait.task,
        Some(previous),
        "the subject stays the previous task"
    );
    let reminder = w
        .ledger
        .remind_question(
            w.project,
            question,
            None,
            &NewNote {
                from: None,
                to: "chief".into(),
                task: Some(previous),
                body: "A reminder".into(),
            },
        )
        .unwrap()
        .expect("the actual holding still waits when the reminder is written");
    w.begin(reminder.id);
    assert_eq!(
        w.ledger.admit(reminder.id, &[]).unwrap(),
        HandOver::Admitted,
        "the accepted topic does not make the reminder stale"
    );
}

#[test]
fn a13_a_queued_reminder_survives_its_topics_acceptance() {
    use cf_ledger::{HandOver, NewNote};
    let mut w = world();
    let (previous, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let current = w.give_to(&session, &[], "Next task").unwrap();
    w.deliver(current.message.unwrap().id);
    let question = w.ask(&session, previous, "About the previous task").id;
    w.deliver(question);
    let reminder = w
        .ledger
        .remind_question(
            w.project,
            question,
            None,
            &NewNote {
                from: None,
                to: "chief".into(),
                task: Some(previous),
                body: "A reminder".into(),
            },
        )
        .unwrap()
        .expect("a reminder while the topic is done");

    w.ledger.accept_task(w.project, previous, "human").unwrap();
    assert_eq!(w.state(current.task.number), "waiting");
    assert_eq!(w.message(question).state, "delivered");
    w.begin(reminder.id);
    assert_eq!(
        w.ledger.admit(reminder.id, &[]).unwrap(),
        HandOver::Admitted,
        "the open question still belongs to the current holding"
    );
}

#[test]
fn a13_a_cross_task_question_reminder_names_its_held_task() {
    let mut w = world();
    let (held, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let topic = w.open("Other worker's task").task.number;
    let other_brief = w.assign(topic, "diana").message.unwrap().id;
    w.deliver(other_brief);
    let question = w.ask(&session, topic, "About the other task").id;
    w.deliver(question);

    let waits = w.ledger.waiting_questions(w.project).unwrap();
    let wait = waits.iter().find(|wait| wait.id == question).unwrap();
    assert_eq!(
        wait.task,
        Some(topic),
        "the question's subject is unchanged"
    );
    assert_eq!(
        wait.held_task,
        Some(held),
        "Finish must address the asking window's task"
    );
}

#[test]
fn a13_an_idle_window_reminds_its_question_without_offering_its_topic_to_finish() {
    let mut w = world();
    let (topic, session, _brief) = assigned(&mut w, false);
    w.ledger.release_task(w.project, topic, "Reassign").unwrap();
    let moved = w.assign(topic, "diana");
    w.deliver(moved.message.unwrap().id);
    let question = w.ask(&session, topic, "Idle question about the task").id;
    w.deliver(question);

    let waits = w.ledger.waiting_questions(w.project).unwrap();
    let wait = waits.iter().find(|wait| wait.id == question).unwrap();
    assert_eq!(wait.task, Some(topic));
    assert_eq!(
        wait.held_task, None,
        "an idle window holds no task to finish"
    );
    let answer = w.answer(question, "Reply to the idle window").id;
    assert_eq!(
        w.ledger
            .next_delivery_of(w.id(&session), answer)
            .unwrap()
            .unwrap()
            .id,
        answer
    );
}

#[test]
fn a13_an_untracked_question_keeps_its_legacy_topic_rule() {
    let mut w = world();
    let (held, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let topic = w.open("Other worker's task").task.number;
    let other_brief = w.assign(topic, "diana").message.unwrap().id;
    w.deliver(other_brief);
    let question = w.ask(&session, topic, "An old cross-task question").id;
    w.deliver(question);
    w.edit(|db| {
        db.execute(
            "DELETE FROM question_binding WHERE question_id=?",
            [question],
        )
        .unwrap();
    });

    let waits = w.ledger.waiting_questions(w.project).unwrap();
    let wait = waits.iter().find(|wait| wait.id == question).unwrap();
    assert_eq!(wait.task, Some(topic));
    assert_eq!(
        wait.held_task, None,
        "the old question does not acquire a reconstructed holding"
    );
    w.ledger
        .record_finished(w.project, held, "Done", (0, 0))
        .unwrap();
    assert_ne!(
        w.message(question).state,
        "cancelled",
        "the old topic path still applies"
    );
}

#[test]
fn a13_cancelling_a_topic_keeps_the_other_holdings_queued_question() {
    let mut w = world();
    let (held, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let topic = w.open("The subject").task.number;
    let other_brief = w.assign(topic, "diana").message.unwrap().id;
    w.deliver(other_brief);
    let question = w.ask(&session, topic, "About that subject").id;
    assert_eq!(w.message(question).state, "queued");

    w.ledger.cancel_task(w.project, topic, "human").unwrap();
    assert_eq!(w.state(held), "waiting");
    assert_eq!(w.message(question).state, "queued");
    w.deliver(question);
    let wait = w.ledger.waiting_questions(w.project).unwrap();
    let wait = wait.iter().find(|wait| wait.id == question).unwrap();
    assert_eq!((wait.task, wait.held_task), (Some(topic), Some(held)));
    let answer = w.answer(question, "The answer still reaches its window").id;
    assert_eq!(w.message(answer).task_number, Some(held));
    assert_eq!(
        w.ledger
            .next_delivery_of(w.id(&session), answer)
            .unwrap()
            .map(|m| m.id),
        Some(answer)
    );
}

#[test]
fn a13_failing_a_topic_keeps_the_other_holdings_gated_question() {
    for give_up in [false, true] {
        let mut w = gated_world();
        let (held, session, brief) = assigned(&mut w, true);
        w.deliver(brief);
        let topic = w.open("The subject").task.number;
        let other_brief = w.assign(topic, "diana").message.unwrap().id;
        w.ledger.approve_message(other_brief, "human").unwrap();
        w.deliver(other_brief);
        let question = w.ask(&session, topic, "About that subject").id;
        assert_eq!(w.message(question).state, "gated");

        if give_up {
            w.ledger.give_up_task(w.project, topic, "Stopped").unwrap();
        } else {
            w.ledger.fail_task(w.project, topic, "Stopped").unwrap();
        }
        assert_eq!(w.state(held), "waiting");
        assert_eq!(w.message(question).state, "gated", "give_up={give_up}");
        w.ledger.approve_message(question, "human").unwrap();
        w.deliver(question);
        let wait = w.ledger.waiting_questions(w.project).unwrap();
        let wait = wait.iter().find(|wait| wait.id == question).unwrap();
        assert_eq!((wait.task, wait.held_task), (Some(topic), Some(held)));
    }
}

#[test]
fn a13_cancelling_a_topic_keeps_the_other_holdings_queued_reminder() {
    use cf_ledger::{HandOver, NewNote};
    let mut w = world();
    let (held, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let topic = w.open("The subject").task.number;
    let other_brief = w.assign(topic, "diana").message.unwrap().id;
    w.deliver(other_brief);
    let question = w.ask(&session, topic, "About that subject").id;
    w.deliver(question);
    let reminder = w
        .ledger
        .remind_question(
            w.project,
            question,
            None,
            &NewNote {
                from: None,
                to: "chief".into(),
                task: Some(topic),
                body: "A reminder".into(),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(w.message(reminder.id).state, "queued", "before topic ends");

    w.ledger.cancel_task(w.project, topic, "human").unwrap();
    assert_eq!(w.state(held), "waiting");
    assert_eq!(w.message(question).state, "delivered");
    assert_eq!(
        w.message(reminder.id).state,
        "queued",
        "{:?}",
        w.message(reminder.id).reason
    );
    w.begin(reminder.id);
    assert_eq!(
        w.ledger.admit(reminder.id, &[]).unwrap(),
        HandOver::Admitted
    );
}

#[test]
fn a13_cancelling_a_topic_keeps_legacy_and_idle_question_paths() {
    for idle in [false, true] {
        let mut w = world();
        let (held, session, brief) = assigned(&mut w, false);
        w.deliver(brief);
        if idle {
            w.ledger.release_task(w.project, held, "Reassign").unwrap();
        }
        let topic = w.open("The subject").task.number;
        let other_brief = w.assign(topic, "diana").message.unwrap().id;
        w.deliver(other_brief);
        let question = w.ask(&session, topic, "About that subject").id;
        if !idle {
            w.edit(|db| {
                db.execute(
                    "DELETE FROM question_binding WHERE question_id=?",
                    [question],
                )
                .unwrap();
            });
        }
        assert_eq!(w.message(question).state, "queued");
        w.ledger.cancel_task(w.project, topic, "human").unwrap();
        assert_eq!(w.message(question).state, "cancelled", "idle={idle}");
    }
}

#[test]
fn a13_finishing_the_chiefs_own_topic_keeps_another_holdings_question_and_reminder() {
    use cf_ledger::NewNote;
    for remind in [false, true] {
        let mut w = world();
        let (held, session, brief) = assigned(&mut w, false);
        w.deliver(brief);
        let topic = w.give("chief", "The chief's own subject");
        w.deliver(topic.message.unwrap().id);
        let question = w.ask(&session, topic.task.number, "About the subject").id;
        let protected = if remind {
            w.deliver(question);
            w.ledger
                .remind_question(
                    w.project,
                    question,
                    None,
                    &NewNote {
                        from: None,
                        to: "chief".into(),
                        task: Some(topic.task.number),
                        body: "A reminder".into(),
                    },
                )
                .unwrap()
                .unwrap()
                .id
        } else {
            question
        };
        assert_eq!(w.message(protected).state, "queued");
        let finished = w
            .ledger
            .record_finished(w.project, topic.task.number, "Done", (0, 0))
            .unwrap();
        assert_eq!(w.state(held), "waiting");
        assert_eq!(w.message(protected).state, "queued", "remind={remind}");
        assert!(!finished.withdrawn.iter().any(|m| m.id == protected));
    }
}

#[test]
fn a13_finishing_an_unreceived_chief_step_releases_another_holdings_reminder() {
    use cf_ledger::NewNote;
    let mut w = world();
    let (held, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let topic = w.give("chief", "The chief's own subject");
    let carrier = topic.message.unwrap().id;
    let question = w.ask(&session, topic.task.number, "About the subject").id;
    w.deliver(question);
    let reminder = w
        .ledger
        .remind_question(
            w.project,
            question,
            None,
            &NewNote {
                from: None,
                to: "chief".into(),
                task: Some(topic.task.number),
                body: "A reminder".into(),
            },
        )
        .unwrap()
        .unwrap();
    w.edit(|db| {
        let carried_by: Option<i64> = db
            .query_row(
                "SELECT carried_by FROM message WHERE id=?",
                [reminder.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(carried_by, Some(carrier));
    });

    let finished = w
        .ledger
        .record_finished(w.project, topic.task.number, "Done", (0, 0))
        .unwrap();
    assert_eq!(w.state(held), "waiting");
    assert_eq!(w.message(reminder.id).state, "queued");
    assert!(!finished.withdrawn.iter().any(|m| m.id == reminder.id));
    assert_eq!(
        w.ledger
            .next_delivery_of(w.id("chief"), reminder.id)
            .unwrap()
            .map(|m| m.id),
        Some(reminder.id),
        "the cancelled carrier must not strand the reminder"
    );
}

#[test]
fn a13_ending_an_unreceived_chief_step_releases_another_holdings_reminder() {
    use cf_ledger::NewNote;
    for ending in ["cancel", "fail", "give up"] {
        let mut w = world();
        let (held, session, brief) = assigned(&mut w, false);
        w.deliver(brief);
        let topic = w.give("chief", "The chief's own subject");
        let carrier = topic.message.unwrap().id;
        let question = w.ask(&session, topic.task.number, "About the subject").id;
        w.deliver(question);
        let reminder = w
            .ledger
            .remind_question(
                w.project,
                question,
                None,
                &NewNote {
                    from: None,
                    to: "chief".into(),
                    task: Some(topic.task.number),
                    body: "A reminder".into(),
                },
            )
            .unwrap()
            .unwrap();
        w.edit(|db| {
            let carried_by: Option<i64> = db
                .query_row(
                    "SELECT carried_by FROM message WHERE id=?",
                    [reminder.id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(carried_by, Some(carrier));
        });
        match ending {
            "cancel" => {
                w.ledger
                    .cancel_task(w.project, topic.task.number, "human")
                    .unwrap();
            }
            "fail" => {
                w.ledger
                    .fail_task(w.project, topic.task.number, "Stopped")
                    .unwrap();
            }
            _ => {
                w.ledger
                    .give_up_task(w.project, topic.task.number, "Stopped")
                    .unwrap();
            }
        }
        assert_eq!(w.state(held), "waiting");
        assert_eq!(
            w.message(reminder.id).state,
            "queued",
            "ending={ending}, reason={:?}",
            w.message(reminder.id).reason
        );
        assert_eq!(
            w.ledger
                .next_delivery_of(w.id("chief"), reminder.id)
                .unwrap()
                .map(|m| m.id),
            Some(reminder.id),
            "ending={ending}: the cancelled carrier must not strand it"
        );
    }
}

#[test]
fn a13_finishing_a_questions_holding_withdraws_its_cross_task_reminder() {
    use cf_ledger::NewNote;
    let mut w = world();
    let (held, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let topic = w.open("Other worker's task").task.number;
    let other_brief = w.assign(topic, "diana").message.unwrap().id;
    w.deliver(other_brief);
    let question = w.ask(&session, topic, "About the other task").id;
    w.deliver(question);
    let reminder = w
        .ledger
        .remind_question(
            w.project,
            question,
            None,
            &NewNote {
                from: None,
                to: "chief".into(),
                task: Some(topic),
                body: "A reminder".into(),
            },
        )
        .unwrap()
        .unwrap();
    w.begin(reminder.id);

    w.ledger
        .record_finished(w.project, held, "Done", (0, 0))
        .unwrap();
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(
        w.message(reminder.id).state,
        "cancelled",
        "an unhanded reminder ends with its question"
    );
}

#[test]
fn t1803_finding_07_a_door_follows_the_holding_not_a_paused_topic() {
    let mut w = world();
    let (_, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let other = w.open("Another worker's task").task.number;
    let other_brief = w.assign(other, "diana").message.unwrap().id;
    w.deliver(other_brief);
    w.pause(other);
    let question = w.ask_with_options(&session, other).id;
    let window = w.id(&session);
    assert_eq!(
        w.ledger.claim_answer(question, window).unwrap(),
        Claim::Waiting
    );
    w.choose(question, "red");
    assert!(matches!(
        w.ledger.claim_answer(question, window).unwrap(),
        Claim::Answered(_)
    ));
}

#[test]
fn t1803_finding_07_accepting_a_question_topic_keeps_the_current_holding_open() {
    let mut w = world();
    let (previous, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let current = w.give_to(&session, &[], "Next task").unwrap();
    w.deliver(current.message.unwrap().id);
    let question = w.ask_with_options(&session, previous).id;
    w.deliver(question);
    assert_eq!(w.state(current.task.number), "waiting");
    w.ledger.accept_task(w.project, previous, "human").unwrap();
    assert_eq!(
        w.ledger.claim_answer(question, w.id(&session)).unwrap(),
        Claim::Waiting
    );
    w.choose(question, "red");
    assert!(matches!(
        w.ledger.claim_answer(question, w.id(&session)).unwrap(),
        Claim::Answered(_)
    ));
}

#[test]
fn t1803_finding_07_accepting_a_topic_keeps_another_holdings_question_at_the_gate() {
    let mut w = gated_world();
    let (previous, session, brief) = assigned(&mut w, true);
    w.deliver(brief);
    w.ledger.record_result(w.project, previous, "Done").unwrap();
    let current = w.give_to(&session, &[], "Next task").unwrap();
    let next_brief = current.message.unwrap().id;
    w.ledger.approve_message(next_brief, "human").unwrap();
    w.deliver(next_brief);
    let question = w.ask(&session, previous, "About the previous task").id;
    assert_eq!(w.message(question).state, "gated");
    w.ledger.accept_task(w.project, previous, "human").unwrap();
    assert_eq!(w.message(question).state, "gated");
    w.ledger.approve_message(question, "human").unwrap();
    w.deliver(question);
    let answer = w.answer(question, "Current window answer").id;
    w.ledger.approve_message(answer, "human").unwrap();
    assert_eq!(
        w.ledger
            .next_delivery_of(w.id(&session), answer)
            .unwrap()
            .map(|m| m.id),
        Some(answer)
    );
}

#[test]
fn t1813_a_queued_brief_that_moved_never_makes_the_idle_window_its_former_holder() {
    let mut w = world();
    let (task, session, _brief) = assigned(&mut w, false);
    let window = w.id(&session);
    assert!(w.ledger.question_holding(window).unwrap().is_none());
    w.ledger.release_task(w.project, task, "Reassign").unwrap();
    let moved = w.assign(task, "diana");
    w.deliver(moved.message.unwrap().id);
    let question = w
        .ledger
        .ask(
            w.project,
            &NewQuestion {
                from: Some(session.clone()),
                to: "chief".into(),
                task: Some(task),
                body: Some("Idle question about the task".into()),
                ..NewQuestion::default()
            },
        )
        .expect("a queued brief was never this idle window's holding")
        .id;
    w.deliver(question);
    let answer = w.answer(question, "Reply to idle session").id;
    assert_eq!(
        w.ledger
            .next_delivery_of(window, answer)
            .unwrap()
            .map(|m| m.id),
        Some(answer)
    );
}

#[test]
fn t1803_finding_07_open_does_not_retire_a_question_when_its_prior_brief_conflicts() {
    use cf_ledger::{NewChief, NewProject};
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let foreign_project = w
        .ledger
        .create_project(&NewProject {
            directory: "/work/B".into(),
            name: "B".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: None,
                effort: None,
                model_name: None,
            },
            staff: Vec::new(),
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
            prompt_suggestion: false,
            own_hands: false,
        })
        .unwrap()
        .id;
    let other = w.open("Second worker's task").task.number;
    let other_brief = w.assign(other, "diana").message.unwrap().id;
    w.deliver(other_brief);
    let question = w
        .ask(&session, other, "Question about the other worker's task")
        .id;
    w.deliver(question);
    let task_id = w.ledger.task(w.project, task).unwrap().unwrap().task.id;
    let other_id = w.ledger.task(w.project, other).unwrap().unwrap().task.id;
    let project = w.project;
    w.edit(|db| {
        db.execute(
            "UPDATE message SET project_id = ?1, task_id = ?2 WHERE id = ?3",
            rusqlite::params![foreign_project, other_id, brief],
        )
        .unwrap();
    });
    assert_eq!(w.message(question).state, "delivered");
    assert_eq!(
        word_notices(&w, "conflicting project records"),
        0,
        "item 8: opening creates no repair notices"
    );
    w.edit(|db| {
        db.execute(
            "UPDATE message SET project_id = ?1, task_id = ?2 WHERE id = ?3",
            rusqlite::params![project, task_id, brief],
        )
        .unwrap();
    });
    assert_ne!(
        w.answer(question, "Reply after correction").state,
        "cancelled"
    );
}

#[test]
fn t1800_finding_07_conflicting_answer_author_refuses_the_move_without_copying_words() {
    use cf_ledger::{NewChief, NewProject};
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Question").id;
    w.deliver(question);
    let answer = w.answer(question, "Private fixture words").id;
    let foreign = w
        .ledger
        .create_project(&NewProject {
            directory: "/work/B".into(),
            name: "B".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: None,
                effort: None,
                model_name: None,
            },
            staff: Vec::new(),
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
            prompt_suggestion: false,
            own_hands: false,
        })
        .unwrap();
    let foreign_chief = foreign
        .participants
        .iter()
        .find(|p| p.handle == "chief")
        .unwrap()
        .id;
    w.edit(|db| {
        db.execute(
            "UPDATE message SET sender_id = ?1 WHERE id = ?2",
            rusqlite::params![foreign_chief, answer],
        )
        .unwrap();
    });
    let moved = w.ledger.release_task(w.project, task, "by human");
    assert!(
        moved.is_err(),
        "a conflicting author cannot leave an answer silently stranded"
    );
    assert_eq!(w.message(answer).state, "queued");
    for chief in [w.id("chief"), foreign_chief] {
        assert!(w
            .ledger
            .inbox(chief, 100)
            .unwrap()
            .iter()
            .all(|m| m.kind != "note" || !m.body.contains("Private fixture words")));
    }
}

#[test]
fn t1800_finding_07_a_foreign_window_binding_cannot_receive_an_answer() {
    use cf_ledger::{NewChief, NewMember, NewProject, NewTask};

    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Private question").id;
    w.deliver(question);
    let foreign = w
        .ledger
        .create_project(&NewProject {
            directory: "/work/B".into(),
            name: "B".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: None,
                effort: None,
                model_name: None,
            },
            staff: vec![NewMember {
                harness: "claude-code".into(),
                model: None,
                effort: None,
                model_name: Some("hera".into()),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
            advisor_tool: false,
            prompt_suggestion: false,
            own_hands: false,
        })
        .unwrap();
    let member = foreign
        .participants
        .iter()
        .find(|p| p.handle == "hera")
        .unwrap()
        .id;
    let foreign_task = w
        .ledger
        .create_task(
            foreign.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Foreign task".into(),
                ..NewTask::default()
            },
        )
        .unwrap();
    let given = w
        .ledger
        .assign_task(foreign.id, foreign_task.task.number, member)
        .unwrap();
    let foreign_window = w
        .ledger
        .project(foreign.id)
        .unwrap()
        .unwrap()
        .participants
        .into_iter()
        .find(|p| p.handle == given.task.assignee.as_deref().unwrap())
        .unwrap()
        .id;
    let foreign_brief = given.message.unwrap().id;
    w.deliver(foreign_brief);
    let foreign_task_id = foreign_task.task.id;
    w.edit(|db| {
        db.execute("UPDATE question_binding SET participant_id = ?1, task_id = ?2, first_brief_id = ?3 WHERE question_id = ?4",
            rusqlite::params![foreign_window, foreign_task_id, foreign_brief, question]).unwrap();
    });

    let result = w.ledger.answer(
        question,
        w.id("chief"),
        Some(&json!("Private answer")),
        None,
    );
    assert!(
        result.is_err(),
        "a foreign holding must never accept this answer"
    );
    for chief in [
        w.id("chief"),
        foreign
            .participants
            .iter()
            .find(|p| p.handle == "chief")
            .unwrap()
            .id,
    ] {
        assert!(w
            .ledger
            .inbox(chief, 100)
            .unwrap()
            .iter()
            .all(|m| !m.body.contains("Private answer")));
    }
}

#[test]
fn t1800_finding_04_failure_withdraws_the_question_and_closes_the_door() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask_with_options(&session, task).id;
    w.deliver(question);
    w.ledger
        .give_up_task(w.project, task, "Window failed")
        .unwrap();
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(
        w.ledger.claim_answer(question, w.id(&session)).unwrap(),
        Claim::Closed
    );
}

#[test]
fn a_refunded_reopen_keeps_the_same_tasks_original_holding() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "First question").id;
    w.deliver(question);
    w.ledger.record_result(w.project, task, "Result").unwrap();
    let reopen = w
        .ledger
        .reopen_task(w.project, task, "human", "Continue")
        .unwrap()
        .message
        .unwrap()
        .id;
    w.begin(reopen);
    assert_eq!(
        w.ledger.admit(reopen, &[]).unwrap(),
        cf_ledger::HandOver::Admitted
    );
    w.ledger
        .retry_delivery(reopen, "Handover was refused", true)
        .unwrap();
    let answer = w.answer(question, "Same window answer").id;
    assert_ne!(w.message(answer).state, "cancelled");
}

#[test]
fn a_refunded_first_brief_does_not_create_a_task_holding() {
    let mut w = world();
    let (_task, session, brief) = assigned(&mut w, false);
    w.begin(brief);
    assert_eq!(
        w.ledger.admit(brief, &[]).unwrap(),
        cf_ledger::HandOver::Admitted
    );
    w.ledger
        .retry_delivery(brief, "Handover was refused", true)
        .unwrap();
    assert_eq!(w.ledger.question_holding(w.id(&session)).unwrap(), None);
}

#[test]
fn a13_handoff_keeps_a_tracked_question_whose_subject_is_accepted_but_holding_waits() {
    let mut w = world();
    let (subject, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    w.ledger.record_result(w.project, subject, "Done").unwrap();
    w.ledger.accept_task(w.project, subject, "human").unwrap();
    let current = w.give_to(&session, &[], "Next task").unwrap();
    w.deliver(current.message.unwrap().id);
    let question = w.ask(&session, subject, "About the previous task").id;
    w.deliver(question);
    assert_eq!(w.state(current.task.number), "waiting");
    assert_eq!(w.message(question).state, "delivered");

    let open = w.ledger.chief_open_work(w.project).unwrap();
    let listed = open.questions.iter().find(|q| q.id == question).expect(
        "the handoff includes the question that the asking window's current task waits for",
    );
    assert_eq!(
        listed.task_number,
        Some(subject),
        "its subject stays in the words"
    );

    w.ledger
        .record_finished(w.project, current.task.number, "Done waiting", (0, 0))
        .unwrap();
    assert!(w
        .ledger
        .chief_open_work(w.project)
        .unwrap()
        .questions
        .iter()
        .all(|q| q.id != question));
}

#[test]
fn a13_handoff_keeps_the_old_subject_rule_for_an_untracked_question() {
    let mut w = world();
    let (subject, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    w.ledger.record_result(w.project, subject, "Done").unwrap();
    let current = w.give_to(&session, &[], "Next task").unwrap();
    w.deliver(current.message.unwrap().id);
    let question = w.ask(&session, subject, "An old question").id;
    w.deliver(question);
    w.edit(|db| {
        db.execute(
            "DELETE FROM question_binding WHERE question_id=?",
            [question],
        )
        .unwrap();
    });
    w.ledger.accept_task(w.project, subject, "human").unwrap();
    assert_eq!(w.message(question).state, "delivered");
    assert!(w
        .ledger
        .chief_open_work(w.project)
        .unwrap()
        .questions
        .iter()
        .all(|q| q.id != question));
}

#[test]
fn a13_handoff_keeps_the_subject_rule_for_a_taskless_window() {
    let mut w = world();
    let (subject, session, _brief) = assigned(&mut w, false);
    w.ledger
        .release_task(w.project, subject, "Reassign")
        .unwrap();
    let moved = w.assign(subject, "diana");
    w.deliver(moved.message.unwrap().id);
    let question = w.ask(&session, subject, "Idle question").id;
    w.deliver(question);
    w.ledger.record_result(w.project, subject, "Done").unwrap();
    w.ledger.accept_task(w.project, subject, "human").unwrap();
    assert_eq!(w.message(question).state, "delivered");
    assert!(w
        .ledger
        .chief_open_work(w.project)
        .unwrap()
        .questions
        .iter()
        .all(|q| q.id != question));
}

#[test]
fn t1859_a_gated_question_notice_stays_gated_even_when_the_gate_was_switched_off() {
    let mut w = gated_world();
    let (task, session, brief) = assigned(&mut w, true);
    w.deliver(brief);
    let question = w.ask(&session, task, "Unapproved question words").id;
    assert_eq!(w.message(question).state, "gated");
    w.ledger.set_gate(w.project, false).unwrap();
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
    let notes = |w: &World| -> Vec<i64> {
        w.ledger.sqlite().prepare("SELECT id FROM message WHERE kind='note' AND body LIKE '%Unapproved question words%'").unwrap()
            .query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    };
    let ids = notes(&w);
    assert_eq!(ids.len(), 1);
    assert_eq!(
        w.message(ids[0]).state,
        "gated",
        "the question must not bypass approval"
    );
    assert_eq!(
        word_notices(&w, "Unapproved question words"),
        0,
        "the chief cannot receive it yet"
    );
    w.ledger.close_questions_of_ended_engine().unwrap();
    assert_eq!(notes(&w).len(), 1);
    w.ledger.approve_message(ids[0], "human").unwrap();
    assert_eq!(w.message(ids[0]).state, "queued");
}

#[test]
fn t1859_a_gated_answer_is_the_only_notice_when_approved_after_restart() {
    let mut w = gated_world();
    let (task, session, brief) = assigned(&mut w, true);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which grammar?").id;
    w.ledger.approve_message(question, "human").unwrap();
    w.deliver(question);
    let answer = w.answer(question, "Use ANSI later").id;
    assert_eq!(w.message(answer).state, "gated");
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
    assert_eq!(word_notices(&w, &format!("m-{question}")), 0);
    w.ledger.approve_message(answer, "human").unwrap();
    assert_eq!(
        w.message(question).reason.as_deref(),
        Some("ConsensFlow restarted")
    );
    let chief = w.id("chief");
    let notes: Vec<_> = w
        .ledger
        .inbox(chief, 100)
        .unwrap()
        .into_iter()
        .filter(|m| m.kind == "note" && m.body.contains(&format!("m-{question}")))
        .collect();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].body.contains("Use ANSI later"));
    assert!(notes[0].body.contains(&format!("cf tell T-{task} \"…\"")));
    assert!(notes[0].body.contains(&format!("cf task finish T-{task}")));
}

#[test]
fn t1869_receiving_words_releases_only_their_own_windows_wait() {
    let mut w = world();
    let (first, zeus, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let second = w.open("Another task").task.number;
    let given = w.assign(second, "diana");
    let diana = given.task.assignee.unwrap();
    w.deliver(given.message.unwrap().id);
    w.ask(&zeus, first, "First question");
    w.ask(&diana, second, "Second question");
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
    assert_eq!([w.state(first), w.state(second)], ["waiting", "waiting"]);
    let told = w
        .ledger
        .tell_queued(w.project, first, "chief", "First answer")
        .unwrap();
    assert_eq!([w.state(first), w.state(second)], ["waiting", "waiting"]);
    w.deliver(told.id);
    assert_eq!([w.state(first), w.state(second)], ["working", "waiting"]);
}

#[test]
fn t1869_a_tell_read_in_the_inbox_waits_for_a_subsequent_answer() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    w.ask(&session, task, "Question");
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
    assert_eq!(w.state(task), "waiting");
    let told = w
        .ledger
        .tell_queued(w.project, task, "chief", "Use ANSI")
        .unwrap();
    w.ledger
        .receive_read(
            w.id(&session),
            &[told.id],
            cf_ledger::Read::Inbox,
            &cf_proto::cf_word::CfWord::bare(),
        )
        .unwrap();
    assert_eq!(w.message(told.id).state, "read");
    assert_eq!(w.state(task), "waiting");
}

#[test]
fn t1869_a_tell_confirmed_after_close_does_not_release_the_old_window_wait() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which grammar?").id;
    w.deliver(question);
    let tell = w
        .ledger
        .tell_queued(w.project, task, "chief", "Use ANSI")
        .unwrap()
        .id;
    w.begin(tell);
    w.ledger.set_project_state(w.project, "suspended").unwrap();
    let window = w.id(&session);
    w.ledger.close_question_window(window, Some(tell)).unwrap();
    w.ledger
        .pause_with_project(w.project, task, "project closed")
        .unwrap();
    w.confirm(tell);
    w.ledger.set_project_state(w.project, "open").unwrap();
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(
        w.state(task),
        "waiting",
        "confirmation of the old window's paste is not a new turn"
    );
}

#[test]
fn t1869_an_in_flight_answer_proved_delivered_at_close_does_not_leave_a_wait() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which grammar?").id;
    w.deliver(question);
    let answer = w.answer(question, "Use ANSI").id;
    w.begin(answer);
    w.ledger.set_project_state(w.project, "suspended").unwrap();
    let window = w.id(&session);
    let deferred = w
        .ledger
        .close_question_window(window, Some(answer))
        .unwrap();
    w.ledger
        .pause_with_project(w.project, task, "project closed")
        .unwrap();
    w.confirm(answer);
    w.ledger.settle_closed_window_answers(&deferred).unwrap();
    w.ledger.set_project_state(w.project, "open").unwrap();
    assert_eq!(w.message(answer).state, "delivered");
    assert_eq!(w.state(task), "working", "the answer reached its window");
    assert_eq!(word_notices(&w, "Use ANSI"), 0);
}

#[test]
fn t1869_a_question_closed_after_the_project_pause_still_holds_its_task() {
    for answered in [false, true] {
        let mut w = world();
        let (task, session, brief) = assigned(&mut w, false);
        w.deliver(brief);
        let question = w.ask(&session, task, "Which grammar?").id;
        w.deliver(question);
        if answered {
            w.answer(question, "Use ANSI");
        }
        w.ledger.set_project_state(w.project, "suspended").unwrap();
        w.ledger
            .pause_with_project(w.project, task, "project closed")
            .unwrap();
        let window = w.id(&session);
        w.ledger.close_question_window(window, None).unwrap();
        w.ledger.set_project_state(w.project, "open").unwrap();
        assert_eq!(w.message(question).state, "cancelled");
        assert_eq!(w.state(task), "waiting", "answered={answered}");
    }
}

#[test]
fn t1859_a_taskless_question_keeps_its_prior_restart_path() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    w.ledger.record_result(w.project, task, "Result").unwrap();
    w.ledger.accept_task(w.project, task, "human").unwrap();
    let question = w
        .ledger
        .ask(
            w.project,
            &NewQuestion {
                from: Some(session),
                to: "chief".into(),
                body: Some("Idle question".into()),
                ..NewQuestion::default()
            },
        )
        .unwrap()
        .id;
    w.deliver(question);
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(
        w.message(question).reason.as_deref(),
        Some("the task moved to another window")
    );
    assert_eq!(word_notices(&w, &format!("m-{question}")), 0);
}

#[test]
fn t1859_a_question_read_while_delivering_is_named_by_number_after_restart() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Already read in flight").id;
    w.begin(question);
    let chief = w.id("chief");
    w.ledger
        .receive_read(
            chief,
            &[question],
            cf_ledger::Read::Inbox,
            &cf_proto::cf_word::CfWord::bare(),
        )
        .unwrap();
    assert_eq!(w.message(question).state, "delivering");
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
    let notes: Vec<_> = w
        .ledger
        .inbox(chief, 100)
        .unwrap()
        .into_iter()
        .filter(|m| m.kind == "note" && m.body.contains(&format!("m-{question}")))
        .collect();
    assert_eq!(notes.len(), 1);
    assert!(!notes[0].body.contains("Already read in flight"));
}

#[test]
fn t1859_reassign_after_a_returned_answer_keeps_the_restart_reason() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Before restart").id;
    w.deliver(question);
    let answer = w.answer(question, "Returned once").id;
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(
        w.message(question).reason.as_deref(),
        Some("ConsensFlow restarted")
    );
    w.ledger.release_task(w.project, task, "Reassign").unwrap();
    assert_eq!(
        w.message(question).reason.as_deref(),
        Some("ConsensFlow restarted")
    );
    assert_eq!(word_notices(&w, "Returned once"), 1);
}

#[test]
fn t1859_reopen_does_not_ask_the_chief_to_answer_a_closed_question_again() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Before restart").id;
    w.deliver(question);
    w.answer(question, "Returned once");
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
    w.ledger.record_result(w.project, task, "Done").unwrap();
    w.ledger
        .reopen_task(w.project, task, "chief", "Go on")
        .unwrap();
    let notes = w.ledger.inbox(w.id("chief"), 100).unwrap();
    assert!(
        !notes
            .iter()
            .any(|m| m.kind == "note" && m.body.contains(&format!("cf answer m-{question}"))),
        "{notes:?}"
    );
    assert_eq!(word_notices(&w, "Returned once"), 1);
}

fn t1859_state_notice(w: &World, question: i64) -> String {
    let notes: Vec<_> = w
        .ledger
        .inbox(w.id("chief"), 100)
        .unwrap()
        .into_iter()
        .filter(|m| m.kind == "note" && m.body.contains(&format!("m-{question}")))
        .collect();
    assert_eq!(notes.len(), 1, "exactly one notice: {notes:?}");
    notes[0].body.clone()
}

fn t1859_review_only(note: &str, expected: &str) {
    assert!(note.contains(expected), "{note}");
    for command in [
        "cf tell",
        "cf task finish",
        "cf task accept",
        "cf task reopen",
    ] {
        assert!(!note.contains(command), "unexpected {command}: {note}");
    }
}

#[test]
fn t1859_state_done_question_at_restart_or_close() {
    for restart in [true, false] {
        for received in [true, false] {
            let mut w = world();
            let (task, session, brief) = assigned(&mut w, false);
            w.deliver(brief);
            let question = w.ask(&session, task, "Which dialect remains?").id;
            if received {
                w.deliver(question);
            }
            w.ledger
                .record_result(w.project, task, "Done before the window closed")
                .unwrap();
            assert_eq!(w.state(task), "done");
            if restart {
                w.ledger.begin_question_engine();
                w.ledger.close_questions_of_ended_engine().unwrap();
            } else {
                w.ledger.set_project_state(w.project, "suspended").unwrap();
                let window = w.id(&session);
                w.ledger.close_question_window(window, None).unwrap();
                w.ledger.set_project_state(w.project, "open").unwrap();
            }
            let note = t1859_state_notice(&w, question);
            t1859_review_only(&note, &format!("T-{task} is done; review its result"));
            assert_eq!(note.contains("Which dialect remains?"), !received, "{note}");
        }
    }
}

#[test]
fn t1859_state_returned_answer_of_done_task() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which dialect?").id;
    w.deliver(question);
    let answer = w.answer(question, "Use ANSI").id;
    w.ledger.record_result(w.project, task, "Done").unwrap();
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    let note = t1859_state_notice(&w, question);
    assert!(note.contains("Use ANSI"), "{note}");
    t1859_review_only(&note, &format!("T-{task} is done; review its result"));
}

#[test]
fn t1859_state_returned_answer_when_task_fails() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which dialect?").id;
    w.deliver(question);
    let answer = w.answer(question, "Use ANSI").id;
    w.ledger
        .fail_task(w.project, task, "Window failed")
        .unwrap();
    assert_eq!(w.state(task), "failed");
    assert_eq!(w.message(answer).state, "cancelled");
    let note = t1859_state_notice(&w, question);
    assert!(note.contains("Use ANSI"), "{note}");
    t1859_review_only(&note, &format!("T-{task} failed; review its result"));
}

#[test]
fn t1859_state_failed_or_queued_question_left_in_an_older_ledger() {
    for state in ["failed", "queued"] {
        for received in [true, false] {
            let mut w = world();
            let (task, session, brief) = assigned(&mut w, false);
            w.deliver(brief);
            let question = w.ask(&session, task, "Unanswered old question").id;
            if received {
                w.deliver(question);
            }
            // A persisted task state from an older ledger, with a still-open binding.
            w.ledger
                .sqlite()
                .execute(
                    "UPDATE task SET state=?1 WHERE project_id=?2 AND number=?3",
                    rusqlite::params![state, w.project, task],
                )
                .unwrap();
            w.ledger.begin_question_engine();
            w.ledger.close_questions_of_ended_engine().unwrap();
            let note = t1859_state_notice(&w, question);
            assert_eq!(
                note.contains("Unanswered old question"),
                !received,
                "{note}"
            );
            if state == "failed" {
                t1859_review_only(&note, &format!("T-{task} failed; review its result"));
            } else {
                for command in [
                    "cf tell",
                    "cf task finish",
                    "cf task accept",
                    "cf task reopen",
                ] {
                    assert!(!note.contains(command), "{note}");
                }
            }
        }
    }
}

#[test]
fn t1859_state_returned_answer_after_failure_was_recorded() {
    let mut w = gated_world();
    let (task, session, brief) = assigned(&mut w, true);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which dialect?").id;
    w.ledger.approve_message(question, "human").unwrap();
    w.deliver(question);
    let answer = w.answer(question, "Use ANSI later").id;
    w.ledger
        .fail_task(w.project, task, "Window failed")
        .unwrap();
    w.ledger.approve_message(answer, "human").unwrap();
    let note = t1859_state_notice(&w, question);
    assert!(note.contains("Use ANSI later"), "{note}");
    t1859_review_only(&note, &format!("T-{task} failed; review its result"));
}

#[test]
fn t1859_state_paused_question_offers_resume_only() {
    for restart in [true, false] {
        let mut w = world();
        let (task, session, brief) = assigned(&mut w, false);
        w.deliver(brief);
        let question = w.ask(&session, task, "Which dialect?").id;
        w.deliver(question);
        w.ledger
            .pause_task(w.project, task, Some("chief"), None)
            .unwrap();
        assert_eq!(w.state(task), "paused");
        t1859_end_paused_window(&mut w, &session, restart);
        let note = t1859_state_notice(&w, question);
        assert!(
            note.contains(&format!("cf task resume T-{task} \"…\"")),
            "{note}"
        );
        assert!(
            !note.contains("cf tell") && !note.contains("cf task finish"),
            "{note}"
        );
        let resumed = w
            .ledger
            .resume_task(w.project, task, Some("chief"), "Use ANSI")
            .unwrap();
        assert!(resumed.message.unwrap().body.contains("Use ANSI"));
    }
}

#[test]
fn t1859_state_paused_returned_answer_offers_resume_only() {
    for restart in [true, false] {
        let mut w = world();
        let (task, session, brief) = assigned(&mut w, false);
        w.deliver(brief);
        let question = w.ask(&session, task, "Which dialect?").id;
        w.deliver(question);
        let answer = w.answer(question, "Use ANSI").id;
        w.ledger
            .pause_task(w.project, task, Some("chief"), None)
            .unwrap();
        assert_eq!(w.state(task), "paused");
        t1859_end_paused_window(&mut w, &session, restart);
        assert_eq!(w.message(answer).state, "cancelled");
        let note = t1859_state_notice(&w, question);
        assert!(note.contains("Use ANSI"), "{note}");
        assert!(
            note.contains(&format!("cf task resume T-{task} \"…\"")),
            "{note}"
        );
        assert!(
            !note.contains("cf tell") && !note.contains("cf task finish"),
            "{note}"
        );
        let resumed = w
            .ledger
            .resume_task(w.project, task, Some("chief"), "Use ANSI")
            .unwrap();
        assert!(resumed.message.unwrap().body.contains("Use ANSI"));
    }
}

fn t1859_end_paused_window(w: &mut World, session: &str, restart: bool) {
    if restart {
        w.ledger.begin_question_engine();
        w.ledger.close_questions_of_ended_engine().unwrap();
    } else {
        w.ledger.set_project_state(w.project, "suspended").unwrap();
        let window = w.id(session);
        w.ledger.close_question_window(window, None).unwrap();
        w.ledger.set_project_state(w.project, "open").unwrap();
    }
}

#[test]
fn t1859_state_a_moved_task_does_not_offer_the_old_window_commands() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w, false);
    w.deliver(brief);
    let question = w.ask(&session, task, "Question in the old window").id;
    let replacement = w.id("diana");
    // Model a persisted old binding whose task was moved by another ledger.
    w.ledger
        .sqlite()
        .execute(
            "UPDATE task SET assignee_id=?1 WHERE project_id=?2 AND number=?3",
            rusqlite::params![replacement, w.project, task],
        )
        .unwrap();
    w.ledger.begin_question_engine();
    w.ledger.close_questions_of_ended_engine().unwrap();
    let note = t1859_state_notice(&w, question);
    assert!(note.contains("Question in the old window"), "{note}");
    assert!(
        !note.contains("cf tell") && !note.contains("cf task finish"),
        "{note}"
    );
}

#[test]
fn t1859_state_tell_and_finish_are_accepted_for_active_tasks() {
    for state in ["working", "waiting"] {
        for tell in [true, false] {
            let mut w = world();
            let (task, session, brief) = assigned(&mut w, false);
            w.deliver(brief);
            if state == "waiting" {
                let question = w.ask(&session, task, "Please decide").id;
                w.deliver(question);
            }
            assert_eq!(w.state(task), state);
            if tell {
                // `cf tell T-n` routes through tell_queued, not an urgent question.
                let told = w
                    .ledger
                    .tell_queued(w.project, task, "chief", "Use ANSI")
                    .unwrap_or_else(|error| panic!("tell in {state}: {error}"));
                assert_eq!(told.kind, "note");
                assert_eq!(w.state(task), state, "tell does not stop the task");
            } else {
                w.ledger
                    .record_finished(w.project, task, "Finish", (0, 0))
                    .unwrap_or_else(|error| panic!("finish in {state}: {error}"));
            }
        }
    }
}

#[test]
fn t1859_state_tell_and_finish_are_refused_for_done_or_failed_tasks() {
    for state in ["done", "failed"] {
        let mut w = world();
        let (task, _, brief) = assigned(&mut w, false);
        w.deliver(brief);
        if state == "done" {
            w.ledger.record_result(w.project, task, "Result").unwrap();
        } else {
            w.ledger
                .fail_task(w.project, task, "Window failed")
                .unwrap();
        }
        assert_eq!(w.state(task), state);
        assert!(w
            .ledger
            .tell_queued(w.project, task, "chief", "Use ANSI")
            .is_err());
        assert!(w
            .ledger
            .record_finished(w.project, task, "Finish", (0, 0))
            .is_err());
    }
}
