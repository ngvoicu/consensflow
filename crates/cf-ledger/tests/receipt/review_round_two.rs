//! Reproductions from the T-1800 review of moved answers.

use crate::fixture::{world, World};
use cf_ledger::{Claim, HandOver, NewChief, NewProject};
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
fn review_02_accept_reports_a_queued_answer() {
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
fn review_02_accept_cannot_admit_an_answer_already_in_flight() {
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
fn review_03_removing_a_paused_member_keeps_the_answer_for_resume() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    let answer = w.answer(question, "Use JSON").id;
    w.pause(task);
    w.ledger.remove_member(w.project, "zeus").unwrap();
    w.resume(task, "Continue");
    let body = w.ledger.task(w.project, task).unwrap().unwrap().task.body;
    assert!(
        body.contains("Use JSON"),
        "answer m-{answer} must be in the new brief: {body}"
    );
}

#[test]
fn review_04_failed_brief_shuts_the_question_door() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.begin(brief);
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
fn review_04_failed_tell_shuts_the_answered_door() {
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
fn review_05_own_done_then_reopen_in_same_session_keeps_the_question() {
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

fn another_project(w: &mut World) -> i64 {
    let other = w
        .ledger
        .create_project(&NewProject {
            directory: "/work/other".into(),
            name: "other".into(),
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
    other
        .participants
        .iter()
        .find(|p| p.handle == "chief")
        .unwrap()
        .id
}

#[test]
fn review_07_open_leaves_a_question_alone_when_its_holder_belongs_to_another_project() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    let other = another_project(&mut w);
    w.ledger
        .sqlite()
        .execute(
            "UPDATE task SET assignee_id = ? WHERE project_id = ? AND number = ?",
            rusqlite::params![other, w.project, task],
        )
        .unwrap();
    w.edit(|_| {});
    assert_eq!(w.message(question).state, "queued");
    assert_eq!(notices(&w, "conflict").len(), 1);
    w.edit(|_| {});
    assert_eq!(notices(&w, "conflict").len(), 1);
}

#[test]
fn review_07_open_never_copies_words_when_answer_sender_conflicts() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    w.ledger.release_task(w.project, task, "by human").unwrap();
    w.ledger
        .sqlite()
        .execute(
            "UPDATE message SET state = 'delivered', reason = NULL WHERE id = ?",
            [question],
        )
        .unwrap();
    let answer = w.answer(question, "Other project's words").id;
    let other = another_project(&mut w);
    w.ledger
        .sqlite()
        .execute(
            "UPDATE message SET sender_id = ? WHERE id = ?",
            rusqlite::params![other, answer],
        )
        .unwrap();
    w.edit(|_| {});
    assert_eq!(w.message(answer).state, "queued");
    assert!(!notices(&w, "Other project's words")
        .iter()
        .any(|s| s.contains("not received")));
    assert_eq!(notices(&w, "conflict").len(), 1);
    w.edit(|_| {});
    assert_eq!(notices(&w, "conflict").len(), 1);
}

#[test]
fn review_08_a_deleted_failed_task_is_not_suggested_for_reopen() {
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
fn review_02_declining_after_done_does_not_invite_a_stranded_replacement() {
    let mut w = crate::fixture::gated_world();
    let (task, session, brief) = assigned(&mut w);
    w.ledger.approve_message(brief, "human").unwrap();
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    w.ledger.approve_message(question, "human").unwrap();
    let answer = w.answer(question, "Use JSON").id;
    w.ledger.record_result(w.project, task, "Done").unwrap();
    w.ledger.decline_message(answer, "human").unwrap();
    let chief = w.id("chief");
    let refused = w
        .ledger
        .answer(question, chief, Some(&json!("Another")), None)
        .unwrap_err();
    assert!(refused.to_string().contains("reopen"));
    let notice = notices(&w, "declined your answer").pop().unwrap();
    assert!(
        notice.contains("reopen") && !notice.contains("Answer it again"),
        "{notice}"
    );
}

#[test]
fn review_02_own_done_stops_an_unhanded_answer_until_the_task_is_reopened() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    let answer = w.answer(question, "Use JSON").id;
    w.begin(answer);
    w.ledger.record_result(w.project, task, "Done").unwrap();
    assert_ne!(w.ledger.admit(answer, &[]).unwrap(), HandOver::Admitted);
    w.ledger.retry_delivery(answer, "not handed", true).unwrap();
    let reopened = w
        .ledger
        .reopen_task(w.project, task, "chief", "Continue")
        .unwrap();
    let begun = w.deliver(reopened.message.unwrap().id);
    assert!(begun.carried.iter().any(|message| message.id == answer));
}

#[test]
fn a_legacy_answer_in_flight_after_a_move_is_returned_before_handover() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    w.ledger.release_task(w.project, task, "by human").unwrap();
    w.ledger
        .sqlite()
        .execute(
            "UPDATE message SET state = 'delivered', reason = NULL WHERE id = ?",
            [question],
        )
        .unwrap();
    let answer = w.answer(question, "Legacy words").id;
    w.ledger
        .sqlite()
        .execute(
            "UPDATE message SET state = 'delivering' WHERE id = ?",
            [answer],
        )
        .unwrap();
    w.edit(|_| {});
    assert_ne!(w.ledger.admit(answer, &[]).unwrap(), HandOver::Admitted);
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(notices(&w, "Legacy words").len(), 1);
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
fn a_former_window_cannot_claim_an_old_answer_while_its_chief_is_away() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let window = w.id(&session);
    let chief = w.id("chief");
    let question = w.ask_with_options(&session, task).id;
    w.ledger.release_task(w.project, task, "by human").unwrap();
    w.ledger.sqlite().execute("UPDATE message SET state = 'delivered', reason = NULL, door_closed_at = NULL WHERE id = ?", [question]).unwrap();
    let answer = w.choose(question, "red").id;
    w.ledger
        .sqlite()
        .execute(
            "UPDATE participant SET left_at = '2026-01-01T00:00:00.000Z' WHERE id = ?",
            [chief],
        )
        .unwrap();
    w.edit(|_| {});
    assert_eq!(w.message(answer).state, "queued");
    assert_eq!(
        w.ledger.claim_answer(question, window).unwrap(),
        Claim::Closed
    );
}

#[test]
fn a_legacy_answer_returned_to_queue_after_restart_goes_back_to_its_author() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    w.ledger.release_task(w.project, task, "by human").unwrap();
    w.ledger
        .sqlite()
        .execute(
            "UPDATE message SET state = 'delivered', reason = NULL WHERE id = ?",
            [question],
        )
        .unwrap();
    let answer = w.answer(question, "Restart words").id;
    w.ledger
        .sqlite()
        .execute(
            "UPDATE message SET state = 'delivering' WHERE id = ?",
            [answer],
        )
        .unwrap();
    w.edit(|_| {});
    let returned = w.ledger.retry_delivery(answer, "restart", true).unwrap();
    assert_eq!(returned.state, "cancelled");
    assert_eq!(notices(&w, "Restart words").len(), 1);
}

#[test]
fn recovery_does_not_reconcile_a_task_with_conflicting_answer_ownership() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    let answer = w.answer(question, "Conflicting words").id;
    let other = another_project(&mut w);
    w.ledger
        .sqlite()
        .execute(
            "UPDATE message SET sender_id = ? WHERE id = ?",
            rusqlite::params![other, answer],
        )
        .unwrap();
    w.ledger
        .sqlite()
        .execute(
            "UPDATE task SET state = 'waiting' WHERE project_id = ? AND number = ?",
            rusqlite::params![w.project, task],
        )
        .unwrap();
    w.edit(|_| {});
    assert_eq!(w.state(task), "waiting");
    assert_eq!(w.message(answer).state, "queued");
    assert_eq!(notices(&w, "conflict").len(), 1);
    w.edit(|_| {});
    assert_eq!(notices(&w, "conflict").len(), 1);
}

#[test]
fn recovery_leaves_a_conflicting_answer_under_its_existing_carrier() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    let answer = w.answer(question, "Conflicting words").id;
    let other = another_project(&mut w);
    w.ledger.sqlite().execute("UPDATE message SET sender_id = ?, carried_by = ?, handed_at = '2026-01-01' WHERE id = ?", rusqlite::params![other, brief, answer]).unwrap();
    w.edit(|_| {});
    let carrier: (Option<i64>, Option<String>) = w
        .ledger
        .sqlite()
        .query_row(
            "SELECT carried_by, handed_at FROM message WHERE id = ?",
            [answer],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(carrier, (Some(brief), Some("2026-01-01".into())));
    assert_eq!(notices(&w, "conflict").len(), 1);
}

#[test]
fn an_older_own_done_reopened_in_the_same_session_keeps_its_delivered_question() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    w.deliver(question);
    w.ledger.record_result(w.project, task, "Done").unwrap();
    // The reviewed build had no columns to retain the prior question state.
    w.ledger.sqlite().execute("UPDATE message SET done_question_state = NULL, done_question_reason = NULL WHERE id = ?", [question]).unwrap();
    w.edit(|_| {});
    let reopened = w
        .ledger
        .reopen_task(w.project, task, "chief", "Go on")
        .unwrap();
    w.deliver(reopened.message.unwrap().id);
    let chief = w.id("chief");
    assert!(w
        .ledger
        .answer(question, chief, Some(&json!("Use JSON")), None)
        .is_ok());
}

#[test]
fn removing_a_paused_backup_keeps_its_answer_for_the_fresh_brief() {
    let mut w = world();
    let backup = w
        .ledger
        .add_backup(
            w.project,
            "zeus",
            &cf_ledger::NewMember {
                harness: "claude-code".into(),
                model: None,
                effort: None,
                model_name: Some("backup".into()),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            },
        )
        .unwrap();
    let task = w.open("Parser").task.number;
    let moved = w.ledger.assign_task(w.project, task, backup.id).unwrap();
    let window = moved.task.assignee.unwrap();
    w.deliver(moved.message.unwrap().id);
    let question = w.ask(&window, task, "Which?").id;
    w.answer(question, "Use JSON");
    w.pause(task);
    w.ledger.remove_backup(w.project, "zeus").unwrap();
    w.resume(task, "Continue");
    let body = w.ledger.task(w.project, task).unwrap().unwrap().task.body;
    assert!(body.contains("Use JSON"), "{body}");
}

#[test]
fn a_recovered_answer_for_a_deleted_failed_task_does_not_suggest_reopening_it() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    w.ledger.release_task(w.project, task, "by human").unwrap();
    w.ledger
        .sqlite()
        .execute(
            "UPDATE message SET state = 'delivered', reason = NULL WHERE id = ?",
            [question],
        )
        .unwrap();
    let answer = w.answer(question, "Use JSON").id;
    let next = w.assign(task, "diana");
    w.deliver(next.message.unwrap().id);
    w.ledger.fail_task(w.project, task, "failed").unwrap();
    w.ledger.delete_tasks(w.project, &[task]).unwrap();
    // A historical ledger may still have the old answer queued at startup.
    w.ledger
        .sqlite()
        .execute(
            "UPDATE message SET state = 'queued', reason = NULL WHERE id = ?",
            [answer],
        )
        .unwrap();
    w.edit(|_| {});
    let notices: Vec<_> = notices(&w, "Use JSON")
        .into_iter()
        .filter(|body| body.contains("At recovery"))
        .collect();
    assert_eq!(notices.len(), 1);
    assert!(
        notices[0].contains("new task") && !notices[0].contains("cf task reopen"),
        "{}",
        notices[0]
    );
}

#[test]
fn an_earlier_chief_finish_is_not_undone_by_a_later_workers_done() {
    let mut w = world();
    let (task, session, brief) = assigned(&mut w);
    w.deliver(brief);
    let question = w.ask(&session, task, "Which?").id;
    w.deliver(question);
    w.ledger
        .record_finished(w.project, task, "Finished by chief", (0, 0))
        .unwrap();
    let reopened = w
        .ledger
        .reopen_task(w.project, task, "chief", "Continue")
        .unwrap();
    w.deliver(reopened.message.unwrap().id);
    w.ledger
        .record_result(w.project, task, "Worker done")
        .unwrap();
    let reopened = w
        .ledger
        .reopen_task(w.project, task, "chief", "Continue again")
        .unwrap();
    w.deliver(reopened.message.unwrap().id);
    let chief = w.id("chief");
    assert_eq!(
        w.ledger
            .answer(question, chief, Some(&json!("Use JSON")), None)
            .unwrap_err()
            .code(),
        Some("question-withdrawn")
    );
}

#[test]
fn a_legacy_answer_to_the_former_window_is_returned_if_the_new_task_ends() {
    for decision in ["failed", "cancelled", "accepted"] {
        let mut w = world();
        let (task, session, brief) = assigned(&mut w);
        w.deliver(brief);
        let question = w.ask(&session, task, "Which?").id;
        w.ledger.release_task(w.project, task, "by human").unwrap();
        w.ledger
            .sqlite()
            .execute(
                "UPDATE message SET state = 'delivered', reason = NULL WHERE id = ?",
                [question],
            )
            .unwrap();
        let answer = w.answer(question, "Legacy answer").id;
        let next = w.assign(task, "diana");
        w.deliver(next.message.unwrap().id);
        match decision {
            "failed" => {
                w.ledger.fail_task(w.project, task, "failed").unwrap();
            }
            "cancelled" => {
                w.ledger.cancel_task(w.project, task, "human").unwrap();
            }
            _ => {
                w.ledger.record_result(w.project, task, "Done").unwrap();
                w.ledger.accept_task(w.project, task, "chief").unwrap();
            }
        }
        assert_eq!(w.message(answer).state, "cancelled");
        assert_eq!(notices(&w, "Legacy answer").len(), 1, "{decision}");
    }
}

#[test]
fn a_legacy_answer_is_returned_to_its_author_without_copying_it_to_another_member() {
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
    w.ledger.release_task(w.project, task, "by human").unwrap();
    w.ledger
        .sqlite()
        .execute(
            "UPDATE message SET state = 'delivered', reason = NULL WHERE id = ?",
            [question],
        )
        .unwrap();
    let diana = w.id("diana");
    let answer = w
        .ledger
        .answer(question, diana, Some(&json!("Private answer")), None)
        .unwrap()
        .id;
    w.edit(|_| {});
    assert_eq!(w.message(answer).state, "cancelled");
    let returned = w
        .ledger
        .inbox(diana, 100)
        .unwrap()
        .into_iter()
        .filter(|m| m.kind == "note" && m.body.contains("Private answer"))
        .count();
    assert_eq!(returned, 1);
    assert!(notices(&w, "Private answer").is_empty());
    assert!(
        w.next("diana").is_some(),
        "the author can be delivered the notice"
    );
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
