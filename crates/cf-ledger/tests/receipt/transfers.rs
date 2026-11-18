//! What a window kept for a task goes with the task when the window is gone:
//! into the brief of the next one, once, when its session ended or the task
//! was taken back from it.

use crate::fixture::{gated_world, world, World};
use cf_ledger::{Claim, Read};
use serde_json::json;

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
        Some("the task moved to another window".to_owned()),
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

/// A tier task whose first window has its brief.
fn asking_window(w: &mut World) -> (i64, String) {
    let task = w.open("Parser").task.number;
    let assigned = w.assign(task, "zeus");
    let window = assigned.task.assignee.unwrap();
    let brief = assigned.message.unwrap().id;
    if w.message(brief).state == "gated" {
        w.ledger.approve_message(brief, "human").unwrap();
    }
    w.deliver(brief);
    (task, window)
}

fn moved_refusal(w: &mut World, question: i64) -> String {
    let chief = w.id("chief");
    let error = w
        .ledger
        .answer(question, chief, Some(&json!("JSON")), None)
        .unwrap_err();
    assert_eq!(error.code(), Some("question-moved"));
    error.to_string()
}

/// Recreate what an older release followed by an answer left: the answer was
/// queued only after the task had left its former window, so it was not in
/// the transferred brief. The current ledger's withdrawal is removed to
/// reproduce the older ledger's question before answering it.
fn older_answer_after_release(w: &mut World, words: &str) -> (i64, String, i64, i64) {
    let (task, old) = asking_window(w);
    let question = w.ask(&old, task, "Which format?").id;
    if w.message(question).state == "gated" {
        w.ledger.approve_message(question, "human").unwrap();
    }
    w.deliver(question);
    w.ledger.release_task(w.project, task, "by @human").unwrap();
    w.ledger.sqlite().execute(
        "UPDATE message SET state = 'delivered', reason = NULL, door_closed_at = NULL WHERE id = ?",
        [question],
    ).unwrap();
    let answer = w.answer(question, words).id;
    (task, old, question, answer)
}

fn recovery_notices(w: &World, answer: i64) -> Vec<cf_ledger::MessageView> {
    w.ledger
        .inbox(w.id("chief"), 100)
        .unwrap()
        .into_iter()
        .filter(|m| {
            m.kind == "note"
                && m.body.contains(&format!("answer m-{answer}"))
                && m.body.contains("not received")
        })
        .collect()
}

#[test]
fn opening_recovers_an_older_answer_queued_after_its_task_moved_once() {
    for assigned in [false, true] {
        let mut w = world();
        let (task, old, question, answer) =
            older_answer_after_release(&mut w, "Use JSON\nKeep the tests.");
        assert_eq!(w.message(answer).state, "queued");
        if assigned {
            let next = w.assign(task, "diana");
            w.deliver(next.message.unwrap().id);
        }
        w.edit(|_| {});
        assert_eq!(w.message(answer).state, "cancelled");
        assert_eq!(w.message(question).state, "cancelled");
        let notes = recovery_notices(&w, answer);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].body.contains("Use JSON\nKeep the tests."));
        assert!(notes[0].body.contains("T-1"));
        assert!(notes[0]
            .body
            .contains(if assigned { "diana" } else { "standard" }));
        assert!(w
            .ledger
            .pending(w.id(&old))
            .unwrap()
            .iter()
            .all(|m| m.id != answer));
        w.edit(|_| {});
        assert_eq!(
            recovery_notices(&w, answer).len(),
            1,
            "a second open says nothing twice"
        );
    }
}

#[test]
fn opening_recovers_each_stranded_answer_once() {
    let mut w = world();
    let (_, _, _, first) = older_answer_after_release(&mut w, "First answer");
    let (_, _, _, second) = older_answer_after_release(&mut w, "Second answer");
    w.edit(|_| {});
    for (answer, words) in [(first, "First answer"), (second, "Second answer")] {
        assert_eq!(w.message(answer).state, "cancelled");
        let notes = recovery_notices(&w, answer);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].body.contains(words));
    }
    w.edit(|_| {});
    assert_eq!(recovery_notices(&w, first).len(), 1);
    assert_eq!(recovery_notices(&w, second).len(), 1);
}

#[test]
fn opening_keeps_an_answer_held_for_approval_and_an_answer_already_received() {
    let mut gated = gated_world();
    let (_, _, _, answer) = older_answer_after_release(&mut gated, "Needs approval");
    assert_eq!(gated.message(answer).state, "gated");
    gated.edit(|_| {});
    assert_eq!(gated.message(answer).state, "gated");
    assert!(recovery_notices(&gated, answer).is_empty());

    let mut delivered = world();
    let (task, old) = asking_window(&mut delivered);
    let question = delivered.ask(&old, task, "Which format?").id;
    let answer = delivered.answer(question, "Already received").id;
    delivered.deliver(answer);
    delivered
        .ledger
        .release_task(delivered.project, task, "by @human")
        .unwrap();
    delivered.edit(|_| {});
    assert_eq!(delivered.message(answer).state, "delivered");
    assert!(recovery_notices(&delivered, answer).is_empty());
}

#[test]
fn passing_an_old_held_answer_on_returns_it_to_the_chief() {
    for reopened in [false, true] {
        let mut w = gated_world();
        let (_, old, question, answer) = older_answer_after_release(&mut w, "Use JSON");
        if reopened {
            w.edit(|_| {});
        }
        assert_eq!(w.message(answer).state, "gated");
        let passed = w.ledger.approve_message(answer, "human").unwrap();
        assert_eq!(passed.state, "cancelled");
        assert_eq!(
            w.message(question).reason.as_deref(),
            Some("the task moved to another window")
        );
        assert!(w
            .ledger
            .pending(w.id(&old))
            .unwrap()
            .iter()
            .all(|m| m.id != answer));
        assert_eq!(recovery_notices(&w, answer).len(), 1);
        w.edit(|_| {});
        assert_eq!(recovery_notices(&w, answer).len(), 1);
    }
}

#[test]
fn declining_an_old_held_answer_does_not_reopen_the_moved_question() {
    let mut w = gated_world();
    let (_, _, question, answer) = older_answer_after_release(&mut w, "Use JSON");
    w.edit(|_| {});
    assert_eq!(w.message(answer).state, "gated");
    w.ledger.decline_message(answer, "human").unwrap();
    assert_eq!(
        w.message(question).reason.as_deref(),
        Some("the task moved to another window")
    );
    assert!(moved_refusal(&mut w, question).contains("cf tell T-1"));
    assert!(
        recovery_notices(&w, answer).is_empty(),
        "declined words are not passed on"
    );
    let notice = w
        .ledger
        .inbox(w.id("chief"), 100)
        .unwrap()
        .into_iter()
        .find(|m| m.body.contains("declined your answer"))
        .expect("the chief hears the decline");
    assert!(notice.body.contains("cf tell T-1"), "{}", notice.body);
}

#[test]
fn opening_keeps_an_answer_to_a_different_sessions_question_about_the_task() {
    let mut w = world();
    let (task, _) = asking_window(&mut w);
    let other_task = w.open("Review").task.number;
    let other = w.assign(other_task, "diana");
    let other_window = other.task.assignee.unwrap();
    w.deliver(other.message.unwrap().id);
    let question = w.ask(&other_window, task, "May I help?").id;
    let answer = w.answer(question, "Yes").id;
    w.edit(|_| {});
    assert_eq!(w.message(answer).state, "queued");
    assert!(recovery_notices(&w, answer).is_empty());
}

#[test]
fn opening_keeps_the_current_windows_queued_answer() {
    let mut w = world();
    let (task, window) = asking_window(&mut w);
    let question = w.ask(&window, task, "Which format?").id;
    let answer = w.answer(question, "Use JSON").id;
    w.edit(|_| {});
    assert_eq!(w.message(answer).state, "queued");
    assert!(recovery_notices(&w, answer).is_empty());
}

#[test]
fn opening_does_not_move_a_mismatched_project_answer_to_another_chief() {
    for mismatch in ["answer", "holder"] {
        let mut w = world();
        let (task, _, _, answer) =
            older_answer_after_release(&mut w, "Private to the first project");
        let other = w
            .ledger
            .create_project(&cf_ledger::NewProject {
                directory: "/work/other".into(),
                name: "other".into(),
                chief: cf_ledger::NewChief {
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
        w.ledger
            .create_task(
                other.id,
                &cf_ledger::NewTask {
                    from: "human".into(),
                    to: Some("chief".into()),
                    body: "Other work".into(),
                    ..cf_ledger::NewTask::default()
                },
            )
            .unwrap();
        let other_chief = other
            .participants
            .iter()
            .find(|p| p.handle == "chief")
            .unwrap()
            .id;
        // Individual foreign keys do not prove every row belongs to the same project.
        if mismatch == "answer" {
            w.ledger
                .sqlite()
                .execute(
                    "UPDATE message SET project_id = ? WHERE id = ?",
                    rusqlite::params![other.id, answer],
                )
                .unwrap();
        } else {
            w.ledger
                .sqlite()
                .execute(
                    "UPDATE task SET assignee_id = ? WHERE project_id = ? AND number = ?",
                    rusqlite::params![other_chief, w.project, task],
                )
                .unwrap();
        }
        w.edit(|_| {});
        assert_eq!(w.message(answer).state, "queued", "{mismatch}");
        assert!(recovery_notices(&w, answer).is_empty(), "{mismatch}");
        assert!(w
            .ledger
            .inbox(other_chief, 100)
            .unwrap()
            .iter()
            .all(|m| !m.body.contains("Private to the first project")));
    }
}

#[test]
fn opening_recovers_the_full_words_of_a_maximum_length_answer() {
    let mut w = world();
    let words = "🟦".repeat(cf_ledger::model::MAX_BODY / 2);
    let (_, _, _, answer) = older_answer_after_release(&mut w, &words);
    w.edit(|_| {});
    let notes = recovery_notices(&w, answer);
    assert_eq!(notes.len(), 1);
    assert!(notes[0].body.ends_with(&words));
    let begun = w.ledger.begin_delivery(notes[0].id).unwrap();
    assert!(begun.message.body.ends_with(&words));
}

#[test]
fn opening_names_the_backup_that_now_holds_the_task() {
    let mut w = world();
    let (task, _, _, answer) = older_answer_after_release(&mut w, "Use JSON");
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
    let moved = w.ledger.assign_task(w.project, task, backup.id).unwrap();
    w.deliver(moved.message.unwrap().id);
    w.edit(|_| {});
    let notes = recovery_notices(&w, answer);
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].body.contains("backup")
            && notes[0].body.contains(&backup.handle)
            && notes[0].body.contains("zeus"),
        "{}",
        notes[0].body
    );
}

#[test]
fn a_failed_recovery_rolls_back_the_notice_and_the_withdrawal() {
    let mut w = world();
    let (_, _, _, answer) = older_answer_after_release(&mut w, "Use JSON");
    let chief = w.id("chief");
    let file = w.dir.path().join("consensflow.db");
    w.ledger.close_in_place().unwrap();
    let db = rusqlite::Connection::open(&file).unwrap();
    db.execute_batch(&format!(
        "CREATE TRIGGER reject_recovery BEFORE UPDATE OF state ON message
         WHEN OLD.id = {answer} AND NEW.state = 'cancelled'
         BEGIN SELECT RAISE(ABORT, 'test recovery rollback'); END;"
    ))
    .unwrap();
    db.close().unwrap();
    assert!(cf_ledger::open_ledger(&file, cf_ledger::Options::default()).is_err());

    let db = rusqlite::Connection::open(&file).unwrap();
    let state: String = db
        .query_row("SELECT state FROM message WHERE id = ?", [answer], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(state, "queued");
    let notices: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM message WHERE recipient_id = ? AND kind = 'note' AND body LIKE ?",
            rusqlite::params![chief, format!("%answer m-{answer}%not received%")],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(notices, 0, "the notice rolls back with the withdrawal");
    db.execute_batch("DROP TRIGGER reject_recovery").unwrap();
    db.close().unwrap();

    let ledger = cf_ledger::open_ledger(&file, cf_ledger::Options::default()).unwrap();
    let notices = ledger
        .inbox(chief, 100)
        .unwrap()
        .into_iter()
        .filter(|m| {
            m.kind == "note"
                && m.body.contains(&format!("answer m-{answer}"))
                && m.body.contains("not received")
        })
        .count();
    assert_eq!(notices, 1);
}

#[test]
fn opening_retires_a_recovered_question_even_if_the_new_task_failed() {
    let mut w = world();
    let (task, _, question, answer) = older_answer_after_release(&mut w, "Use JSON");
    let next = w.assign(task, "diana");
    w.deliver(next.message.unwrap().id);
    w.ledger
        .fail_task(w.project, task, "the new window failed")
        .unwrap();
    // An older ledger could still have had this old answer queued at restart.
    w.ledger
        .sqlite()
        .execute(
            "UPDATE message SET state = 'queued', reason = NULL WHERE id = ?",
            [answer],
        )
        .unwrap();
    w.edit(|_| {});
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(
        w.message(question).reason.as_deref(),
        Some("the task moved to another window")
    );
    let notes = recovery_notices(&w, answer);
    assert_eq!(notes.len(), 1);
    assert!(notes[0].body.contains("failed") && notes[0].body.contains("diana"));
    assert!(moved_refusal(&mut w, question).contains("failed"));
}

#[test]
fn a_departed_window_cannot_ask_its_old_task_after_the_release() {
    let mut w = world();
    let (task, old) = asking_window(&mut w);
    w.ledger.release_task(w.project, task, "by @human").unwrap();
    let error = w
        .ledger
        .ask(
            w.project,
            &cf_ledger::NewQuestion {
                from: Some(old),
                to: "chief".to_owned(),
                task: Some(task),
                body: Some("Which format?".to_owned()),
                ..cf_ledger::NewQuestion::default()
            },
        )
        .expect_err("the old window no longer has the task");
    assert_eq!(error.code(), Some("question-moved"));
    assert!(error.to_string().contains("T-1"));
}

#[test]
fn a_window_cannot_ask_its_old_task_after_it_reports_done() {
    let mut w = world();
    let (task, old) = asking_window(&mut w);
    w.ledger.record_result(w.project, task, "Done").unwrap();
    let error = w
        .ledger
        .ask(
            w.project,
            &cf_ledger::NewQuestion {
                from: Some(old),
                to: "chief".to_owned(),
                task: Some(task),
                body: Some("Which format?".to_owned()),
                ..cf_ledger::NewQuestion::default()
            },
        )
        .expect_err("a finished window no longer waits for a new answer");
    assert!(error.to_string().contains("T-1"));
}

fn moved_question_in_a_new_window(w: &mut World) -> (i64, i64) {
    let (task, old) = asking_window(w);
    let question = w.ask(&old, task, "Which format?").id;
    w.ledger.release_task(w.project, task, "by @human").unwrap();
    let next = w.assign(task, "diana");
    w.deliver(next.message.unwrap().id);
    (task, question)
}

#[test]
fn a_moved_question_after_the_new_window_fails_explains_how_to_pass_the_answer() {
    let mut w = world();
    let (task, question) = moved_question_in_a_new_window(&mut w);
    w.ledger
        .fail_task(w.project, task, "the window failed")
        .unwrap();

    let words = moved_refusal(&mut w, question);
    assert!(
        words.contains("failed") && words.contains("diana"),
        "{words}"
    );
    assert!(words.contains("starts over"), "{words}");
    assert!(
        words.contains("reopen") && words.contains("cf tell T-1"),
        "{words}"
    );
}

#[test]
fn a_moved_question_after_the_task_is_cancelled_explains_the_move_and_the_end() {
    let mut w = world();
    let (task, question) = moved_question_in_a_new_window(&mut w);
    w.ledger.cancel_task(w.project, task, "chief").unwrap();

    let words = moved_refusal(&mut w, question);
    assert!(
        words.contains("cancelled") && words.contains("starts over"),
        "{words}"
    );
    assert!(
        words.contains("new task") && words.contains("cf tell"),
        "{words}"
    );
}

#[test]
fn a_moved_question_after_the_task_is_accepted_explains_the_move_and_the_end() {
    let mut w = world();
    let (task, question) = moved_question_in_a_new_window(&mut w);
    w.ledger.record_result(w.project, task, "Done").unwrap();
    w.ledger.accept_task(w.project, task, "chief").unwrap();

    let words = moved_refusal(&mut w, question);
    assert!(
        words.contains("accepted") && words.contains("starts over"),
        "{words}"
    );
    assert!(
        words.contains("new task") && words.contains("cf tell"),
        "{words}"
    );
}

#[test]
fn release_withdraws_questions_in_every_delivery_state_and_closes_their_doors() {
    for state in [
        "queued",
        "delivering",
        "delivered",
        "read",
        "gated",
        "failed",
    ] {
        let mut w = if state == "gated" {
            gated_world()
        } else {
            world()
        };
        let (task, old) = asking_window(&mut w);
        let question = w.ask_with_options(&old, task).id;
        match state {
            "delivering" => {
                w.begin(question);
            }
            "delivered" => {
                w.deliver(question);
            }
            "failed" => {
                w.begin(question);
                w.ledger
                    .fail_delivery(question, "the chief's window closed")
                    .unwrap();
            }
            "read" => {
                let chief = w.id("chief");
                w.ledger
                    .receive_read(
                        chief,
                        &[question],
                        Read::Inbox,
                        &cf_proto::cf_word::CfWord::bare(),
                    )
                    .unwrap();
            }
            _ => {}
        }
        assert_eq!(w.message(question).state, state);
        let old_id = w.id(&old);
        w.ledger.release_task(w.project, task, "by @human").unwrap();
        assert_eq!(w.message(question).state, "cancelled", "{state}");
        assert_eq!(
            w.ledger.claim_answer(question, old_id).unwrap(),
            Claim::Closed,
            "{state}"
        );
        let words = moved_refusal(&mut w, question);
        assert!(
            words.contains("open") && words.contains("standard") && words.contains("cf tell T-1"),
            "{state}: {words}"
        );
        assert!(w
            .ledger
            .chief_open_work(w.project)
            .unwrap()
            .questions
            .iter()
            .all(|m| m.id != question));
        assert!(w
            .ledger
            .pending(old_id)
            .unwrap()
            .iter()
            .all(|m| m.reply_to != Some(question)));
    }
}

#[test]
fn a_carried_answer_is_kept_once_and_does_not_make_its_old_question_oblige_again() {
    let mut w = world();
    let Kept {
        session,
        task,
        question,
        answer,
        ..
    } = kept_for_a_session(&mut w);
    w.ledger.release_task(w.project, task, "by @human").unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(
        w.ledger
            .task(w.project, task)
            .unwrap()
            .unwrap()
            .task
            .body
            .matches("\nJSON\n")
            .count(),
        1
    );
    let words = moved_refusal(&mut w, question);
    assert!(words.contains("cf tell T-1"), "{words}");

    let new = w.assign(task, "diana");
    let new_window = new.task.assignee.unwrap();
    w.deliver(new.message.unwrap().id);
    assert_eq!(w.state(task), "working");
    let fresh = w.ask(&new_window, task, "Which parser?").id;
    assert_eq!(w.state(task), "waiting");
    let open = w.ledger.chief_open_work(w.project).unwrap().questions;
    assert!(open.iter().all(|m| m.id != question));
    assert!(open.iter().any(|m| m.id == fresh));
    let words = moved_refusal(&mut w, question);
    assert!(words.contains("diana"), "{words}");
    w.answer(fresh, "LALR");
    assert_eq!(w.state(task), "working");
    assert!(w
        .ledger
        .pending(w.id(&session))
        .unwrap()
        .iter()
        .all(|m| m.reply_to != Some(question)));
}

#[test]
fn a_delivered_answer_stays_answered_after_the_task_moves() {
    let mut w = world();
    let (task, old) = asking_window(&mut w);
    let question = w.ask(&old, task, "Which format?").id;
    let answer = w.answer(question, "JSON").id;
    w.deliver(answer);
    w.ledger.release_task(w.project, task, "by @human").unwrap();
    w.edit(|_| {});
    let chief = w.id("chief");
    let error = w
        .ledger
        .answer(question, chief, Some(&json!("Again")), None)
        .unwrap_err();
    assert_eq!(error.code(), Some("already-answered"));
    assert_eq!(w.message(answer).state, "delivered");
}

#[test]
fn an_older_withdrawal_and_a_carried_answer_get_the_moved_reason_on_open() {
    let mut w = world();
    let Kept {
        session,
        task,
        question,
        answer,
        ..
    } = kept_for_a_session(&mut w);
    let unseen = w.ask(&session, task, "And which version?").id;
    w.ledger.release_task(w.project, task, "by @human").unwrap();
    assert_eq!(w.message(answer).state, "cancelled");
    w.edit(|db| {
        db.execute(
            "UPDATE message SET state = 'read', reason = NULL WHERE id = ?",
            [question],
        )
        .unwrap();
        db.execute(
            "UPDATE message SET state = 'cancelled', reason = ? WHERE id = ?",
            rusqlite::params![format!("@{session} no longer has T-{task}"), unseen],
        )
        .unwrap();
    });
    assert!(moved_refusal(&mut w, question).contains("cf tell T-1"));
    assert!(moved_refusal(&mut w, unseen).contains("cf tell T-1"));
}

#[test]
fn a_window_given_the_task_again_does_not_wait_for_the_question_it_left_behind() {
    let mut w = world();
    let Kept {
        session,
        task,
        question,
        ..
    } = kept_for_a_session(&mut w);
    w.ledger.release_task(w.project, task, "by @human").unwrap();
    let assigned = w.assign(task, "diana");
    w.deliver(assigned.message.unwrap().id);
    // Exercise a legacy ledger that gave this task to its earlier session again.
    w.hand_to(task, &session);
    let fresh = w.ask(&session, task, "Which parser?").id;
    w.answer(fresh, "LALR");
    assert_eq!(
        w.state(task),
        "working",
        "the old question does not hold the task"
    );
    w.ledger.record_result(w.project, task, "Done").unwrap();
    w.ledger
        .reopen_task(w.project, task, "chief", "One more change")
        .unwrap();
    let notes = w.ledger.inbox(w.id("chief"), 100).unwrap();
    assert!(notes.iter().all(|m| !m
        .body
        .contains(&format!("m-{question} still waits for an answer"))));
}

#[test]
fn an_older_ledger_retires_a_delivered_question_left_behind_by_a_release() {
    let mut w = world();
    let (task, old) = asking_window(&mut w);
    let question = w.ask(&old, task, "Which format?").id;
    w.deliver(question);
    w.ledger.release_task(w.project, task, "by @human").unwrap();
    let new = w.assign(task, "diana");
    w.deliver(new.message.unwrap().id);
    // The old build left the delivered question as it was. Reopen the same file.
    w.edit(|db| {
        db.execute("UPDATE message SET state = 'delivered', reason = NULL, door_closed_at = NULL WHERE id = ?", [question]).unwrap();
    });
    assert_eq!(w.message(question).state, "cancelled");
    assert!(w
        .ledger
        .chief_open_work(w.project)
        .unwrap()
        .questions
        .iter()
        .all(|m| m.id != question));
    let words = moved_refusal(&mut w, question);
    assert!(
        words.contains("diana") && words.contains("cf tell T-1"),
        "{words}"
    );
}

#[test]
fn opening_the_ledger_keeps_the_current_windows_question_and_an_unrelated_one() {
    let mut w = world();
    let (task, window) = asking_window(&mut w);
    let own = w.ask(&window, task, "Which format?").id;
    let unrelated = w.ask("diana", task, "May I help?").id;
    w.edit(|_| {});
    assert_eq!(w.message(own).state, "queued");
    assert_eq!(w.message(unrelated).state, "queued");
}

#[test]
fn removing_a_member_withdraws_its_delivered_question_and_closes_its_door() {
    let mut w = world();
    let (task, window) = asking_window(&mut w);
    let question = w.ask_with_options(&window, task).id;
    w.deliver(question);
    let old = w.id(&window);
    assert_eq!(
        w.ledger.claim_answer(question, old).unwrap(),
        Claim::Waiting
    );
    w.ledger.remove_member(w.project, "zeus").unwrap();
    assert_eq!(w.state(task), "cancelled");
    assert_eq!(w.message(question).state, "cancelled");
    assert_eq!(w.ledger.claim_answer(question, old).unwrap(), Claim::Closed);
    let chief = w.id("chief");
    let error = w
        .ledger
        .answer(question, chief, Some(&json!("JSON")), None)
        .unwrap_err();
    assert_eq!(error.code(), Some("task-cancelled"));
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
