//! The questions members put to the chief, counted from the ledger: which of
//! them the chief owed an answer to. The product's rule (`chief_open_work`, T-157)
//! is that a question on a task that is over (accepted, cancelled or failed) is
//! not open, whatever became of its answer; the counts agree with it (T-192).
//! An answer the cancel of its task took back was rightly never delivered, as a
//! result withdrawn with its task was (`chief.rs`).

use cf_evals::measure::{measure, mechanics, Check, MemberQuestions, Options};

use crate::support::{message, project_with, task, Folder};

/// `(name, ok)` of each check, to read them side by side with what is expected.
fn named(checks: &[Check]) -> Vec<(&str, bool)> {
    checks
        .iter()
        .map(|check| (check.name.as_str(), check.ok))
        .collect()
}

/// Zeus (3) asks the chief (2) a question on `task`, delivered; its answer, if
/// any, is in `answer`.
fn asked(db: &rusqlite::Connection, project: i64, task: i64, answer: Option<&str>) {
    let at = |time: &str| format!("2026-10-08T{time}.000Z");
    message(
        db,
        project,
        (2, 3),
        "question",
        task,
        "delivered",
        &at("10:03:00"),
        Some(&at("10:03:01")),
    );
    let question = db.last_insert_rowid();
    if let Some(state) = answer {
        db.execute(
            "INSERT INTO message (project_id, recipient_id, sender_id, kind, task_id, reply_to, body, state, created_at)
             VALUES (?1, 3, 2, 'answer', ?2, ?3, 'x', ?4, ?5)",
            rusqlite::params![project, task, question, state, at("10:04:00")],
        )
        .unwrap();
    }
}

#[test]
fn does_not_count_a_question_on_a_finished_task_as_owed_an_answer_unless_it_was_answered_for_good()
{
    let folder = Folder::new();
    let (db, project) = project_with(&folder, &["zeus", "diana"]);
    // The participants are the human (1), the chief (2), zeus (3) and diana (4).
    // T-1 was cancelled after the chief answered: the cancel took the answer back.
    let one = task(&db, project, 1, 3, "cancelled");
    asked(&db, project, one, Some("cancelled"));
    // T-2 was accepted with its question never answered: nobody waits for it.
    let two = task(&db, project, 2, 3, "accepted");
    asked(&db, project, two, None);
    // T-3 goes on: its answer was withdrawn, and the question is still owed one.
    let three = task(&db, project, 3, 3, "working");
    asked(&db, project, three, Some("cancelled"));
    // T-4 waits for its answer.
    let four = task(&db, project, 4, 3, "waiting");
    asked(&db, project, four, None);
    // T-5 failed, but its question was answered and the answer reached the window.
    let five = task(&db, project, 5, 3, "failed");
    asked(&db, project, five, Some("read"));
    drop(db);

    let metrics = measure(&folder.ledger(), &Options::default()).unwrap();
    let p = metrics.plumbing;
    assert_eq!(
        [
            p.member_questions,
            p.member_questions_answered,
            p.answers_delivered
        ],
        [3, 1, 1],
        "T-3, T-4 and T-5 are owed an answer; T-5 got one"
    );
    assert_eq!(
        metrics.member_questions_by,
        [MemberQuestions {
            role: "worker".to_owned(),
            harness: Some("claude-code".to_owned()),
            asked: 3,
            answered: 1,
            delivered: 1,
            longest: 1,
        }]
    );
    let questions: Vec<Check> = mechanics(&p, 5)
        .into_iter()
        .filter(|check| {
            check.name.starts_with("every question") || check.name.starts_with("every answer")
        })
        .collect();
    assert_eq!(
        named(&questions),
        [
            (
                "every question a member asked the chief was answered (1/3)",
                false
            ),
            ("every answer reached the member (1/1)", true),
        ]
    );
}

#[test]
fn holds_the_plumbing_when_every_question_on_a_finished_task_was_answered_and_taken_back_or_never_answered(
) {
    let folder = Folder::new();
    let (db, project) = project_with(&folder, &["zeus"]);
    let one = task(&db, project, 1, 3, "cancelled");
    asked(&db, project, one, Some("cancelled"));
    let two = task(&db, project, 2, 3, "accepted");
    asked(&db, project, two, None);
    let three = task(&db, project, 3, 3, "failed");
    asked(&db, project, three, None);
    drop(db);

    let metrics = measure(&folder.ledger(), &Options::default()).unwrap();
    let questions: Vec<Check> = mechanics(&metrics.plumbing, 3)
        .into_iter()
        .filter(|check| {
            check.name.starts_with("every question") || check.name.starts_with("every answer")
        })
        .collect();
    assert_eq!(
        named(&questions),
        [
            (
                "every question a member asked the chief was answered (0/0)",
                true
            ),
            ("every answer reached the member (0/0)", true),
        ]
    );
}
