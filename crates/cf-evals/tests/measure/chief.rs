//! Measuring a chief from the ledger: what it controlled, what reached it, what it
//! said, and who asked it.

use cf_evals::measure::{measure, mechanics, Check, MemberQuestions, Options, Plumbing};
use cf_evals::scenarios;

use crate::support::{measured, measured_replacing, message, project_with, recorded, task, Folder};

/// `(name, ok)` of each check, to read them side by side with what is expected.
fn named(checks: &[Check]) -> Vec<(&str, bool)> {
    checks
        .iter()
        .map(|check| (check.name.as_str(), check.ok))
        .collect()
}

#[test]
fn counts_the_chiefs_controls_a_tell_and_its_answer_a_pause_and_a_resume_an_after_follow_up() {
    let mut metrics = measured("controls");
    let p = metrics.plumbing;
    assert_eq!(
        [
            p.tells,
            p.tells_answered,
            p.pauses,
            p.resumes,
            p.continuations
        ],
        [1, 1, 1, 1, 1]
    );
    assert_eq!(
        metrics.tasks[1].assignee, metrics.tasks[0].assignee,
        "the follow-up goes to the same window"
    );
    let tells: Vec<Check> = mechanics(&p, 2)
        .into_iter()
        .filter(|check| check.name.starts_with("every tell"))
        .collect();
    assert_eq!(
        named(&tells),
        [("every tell the chief sent was answered (1/1)", true)]
    );
    metrics.files_changed = vec!["site/notes.md".to_owned()];
    let trip = scenarios::by_id("control-trip").unwrap();
    assert_eq!(
        named(&trip.verdict(&metrics)),
        [
            ("the chief stops the task with cf tell", true),
            ("the worker answers the tell", true),
            ("the task is paused and resumed", true),
            ("a follow-up goes to the same window (--after)", true),
            ("both tasks are accepted", false),
            ("only site/notes.md is new", true),
            ("the owner is not asked anything", true),
        ],
        "the follow-up is not yet accepted here"
    );
}

#[test]
fn does_not_count_a_result_the_chief_decided_on_before_it_was_given_it_as_one_that_never_reached_the_chief(
) {
    let folder = Folder::new();
    let (db, project) = project_with(&folder, &["zeus", "diana"]);
    // The participants are the human (1), the chief (2), zeus (3) and diana (4).
    let at = |time: &str| format!("2026-10-08T{time}.000Z");
    // T-1's result waited behind the chief's turn, which accepted the task: the
    // decision withdrew it, never given.
    let one = task(&db, project, 1, 3, "accepted");
    message(
        &db,
        project,
        (3, 2),
        "task",
        one,
        "delivered",
        &at("10:00:00"),
        Some(&at("10:01:00")),
    );
    message(
        &db,
        project,
        (2, 3),
        "result",
        one,
        "cancelled",
        &at("10:00:00"),
        None,
    );
    // T-2's reached the chief.
    let two = task(&db, project, 2, 4, "done");
    message(
        &db,
        project,
        (4, 2),
        "task",
        two,
        "delivered",
        &at("10:00:00"),
        Some(&at("10:02:00")),
    );
    message(
        &db,
        project,
        (2, 4),
        "result",
        two,
        "delivered",
        &at("10:05:00"),
        None,
    );
    drop(db);

    let metrics = measure(&folder.ledger(), &Options::default()).unwrap();
    assert_eq!(
        [metrics.plumbing.results, metrics.plumbing.results_delivered],
        [1, 1],
        "the one withdrawn was not owed"
    );
    let results: Vec<Check> = mechanics(&metrics.plumbing, 2)
        .into_iter()
        .filter(|check| check.name.starts_with("every result"))
        .collect();
    assert_eq!(
        named(&results),
        [("every result reached the chief (1/1)", true)]
    );
}

#[test]
fn measures_long_messages_the_longest_result_of_each_kind_the_owners_answers_the_notes_whole() {
    let metrics = measured("long-messages");
    assert_eq!(
        (
            metrics.longest_result.worker,
            metrics.longest_result.advisor,
            metrics.longest_result.reviewer
        ),
        (9010, 12, 0)
    );
    assert_eq!(metrics.owner_messages, [11, 6000]);
    assert!(
        metrics.notes_text.ends_with("DELTA-5530 at the end"),
        "{}",
        metrics.notes_text
    );
}

#[test]
fn counts_tasks_parallel_work_advice_reviews_questions_notes_and_the_chiefs_own_edits() {
    let mut metrics = measured("chief-turns");
    assert_eq!(metrics.chief, "chief");
    let tasks: Vec<(i64, Option<&str>)> = metrics
        .tasks
        .iter()
        .map(|task| (task.number, task.pool.as_deref()))
        .collect();
    assert_eq!(
        tasks,
        [
            (1, None),
            (2, None),
            (3, Some("advisor")),
            (4, Some("reviewer"))
        ]
    );
    assert_eq!(
        [metrics.parallel, metrics.advice, metrics.reviews],
        [2, 1, 1],
        "two briefs delivered before either result"
    );
    assert_eq!(metrics.questions_on_board, 0);
    assert_eq!(
        metrics.ask_refused, 1,
        "the chief tried cf ask once and was sent to its terminal"
    );
    assert_eq!(metrics.notes_to_human.len(), 1);
    assert_eq!((metrics.chief_turns, metrics.chief_edits), (4, Some(2)));
    assert_eq!(
        metrics.chief_last_words,
        "Shall I translate it? Or wait for you?"
    );
    // What the owner was asked in the terminal: two questions, in one turn's end;
    // a question mid-turn ("Now the menu?") is not put to anyone.
    let owner = &metrics.owner_questions;
    assert_eq!(
        (owner.questions, owner.turns_asking, owner.pickers),
        (2, 1, 0)
    );
    assert!(
        metrics.files_changed.is_empty(),
        "no fixture given: nothing compared"
    );
    // Who asked the chief, by role and harness: every run tells it.
    assert_eq!(
        metrics.member_questions_by,
        [MemberQuestions {
            role: "worker".to_owned(),
            harness: Some("claude-code".to_owned()),
            asked: 1,
            answered: 1,
            delivered: 1,
            longest: 13,
        }]
    );
    // A task given by pool has no brief until a member takes it: two briefs.
    assert_eq!(
        metrics.plumbing,
        Plumbing {
            briefs: 2,
            briefs_delivered: 2,
            results: 2,
            results_delivered: 1,
            member_questions: 1,
            member_questions_answered: 1,
            answers_delivered: 1,
            accepted: 1,
            tells: 0,
            tells_answered: 0,
            placed: 4,
            pauses: 0,
            resumes: 0,
            continuations: 0,
        }
    );
    assert_eq!(metrics.task_count, 4);
    assert_eq!(
        named(&mechanics(&metrics.plumbing, 4)),
        [
            ("every task brief was delivered (2/2)", true),
            ("every result reached the chief (1/2)", false),
            (
                "every question a member asked the chief was answered (1/1)",
                true
            ),
            ("every answer reached the member (1/1)", true),
            ("every tell the chief sent was answered (0/0)", true),
            ("the board showed every task (4/4)", true),
        ]
    );
    assert!(
        !mechanics(&metrics.plumbing, 3).last().unwrap().ok,
        "a task the board did not list"
    );

    let decisions = scenarios::by_id("six-decisions").unwrap();
    assert_eq!(
        named(&decisions.verdict(&metrics)),
        [
            (
                "the owner is asked in the terminal, at least three questions",
                false
            ),
            ("nothing is asked on the board", true),
            ("the chief never tries cf ask", false),
            ("a finding reaches the owner as a note", true),
            ("at least two tasks go on the board", true),
            ("two tasks run side by side at some point", true),
            ("finished work goes to a review", true),
            (
                "the chief's own edits stay under ten (counted for a Claude chief only)",
                true
            ),
        ]
    );
    let trip = scenarios::by_id("round-trip").unwrap();
    metrics.files_changed = vec!["site/notes.md".to_owned()];
    metrics.owner_questions.questions = 0;
    metrics.owner_questions.turns_asking = 0;
    metrics.owner_questions.texts.clear();
    assert!(
        trip.verdict(&metrics).iter().all(|check| check.ok),
        "the round trip holds on this ledger once only notes.md is new and the owner was not asked"
    );
}

#[test]
fn the_human_inbox_is_its_newest_500_messages_which_a_report_reads_in_order() {
    let folder = Folder::new();
    let (db, project) = project_with(&folder, &[]);
    // The human is the first participant and the chief the second: 600 notes from one to the other.
    for n in 1..=600 {
        db.execute(
            "INSERT INTO message (project_id, recipient_id, sender_id, kind, body, state, created_at)
             VALUES (?1, 1, 2, 'note', ?2, 'delivered', '2026-10-08T10:00:00.000Z')",
            rusqlite::params![project, format!("note {n}")],
        )
        .unwrap();
    }
    drop(db);
    let metrics = measure(&folder.ledger(), &Options::default()).unwrap();
    let notes = &metrics.notes_to_human;
    assert_eq!(notes.len(), 500);
    assert_eq!((notes[0].id, notes[499].id), (101, 600));
    assert_eq!(notes[0].body, "note 101");
}

#[test]
fn a_note_is_kept_to_its_first_200_characters_in_the_list_and_whole_in_the_text() {
    let folder = Folder::new();
    let (db, project) = project_with(&folder, &[]);
    db.execute(
        "INSERT INTO message (project_id, recipient_id, sender_id, kind, body, state, created_at)
         VALUES (?1, 1, 2, 'note', ?2, 'delivered', '2026-10-08T10:00:00.000Z')",
        rusqlite::params![project, "ș".repeat(300)],
    )
    .unwrap();
    drop(db);
    let metrics = measure(&folder.ledger(), &Options::default()).unwrap();
    assert_eq!(metrics.notes_to_human[0].body.chars().count(), 200);
    assert_eq!(metrics.notes_text.chars().count(), 300);
}

#[test]
fn the_ledger_of_a_real_run_is_judged_as_the_run_judged_it_the_round_trip_held_and_so_did_the_boards_plumbing(
) {
    // `npm run eval -- --scenario round-trip --chief claude --staff claude`, on the native daemon.
    let mut metrics = measured("eval-round-trip");
    let trip = scenarios::by_id("round-trip").unwrap();
    // The files the run changed are the workspace's, which the ledger does not hold.
    metrics.files_changed = vec!["site/notes.md".to_owned()];
    let checks = trip.verdict(&metrics);
    assert_eq!(checks.len(), 7);
    assert!(
        checks.iter().all(|check| check.ok),
        "{:?}",
        checks.iter().filter(|check| !check.ok).collect::<Vec<_>>()
    );
    assert!(
        mechanics(&metrics.plumbing, 2).iter().all(|check| check.ok),
        "the plumbing held"
    );
    assert_eq!(
        [metrics.task_count, metrics.parallel, metrics.reviews],
        [2, 1, 1]
    );
    assert_eq!(
        metrics.member_questions_by[0].harness.as_deref(),
        Some("claude-code")
    );
}

#[test]
fn a_chief_switched_to_another_harness_is_read_the_chiefs_in_order_the_switches_the_history_read_and_its_words(
) {
    // The run's codeword is random; the recording holds the one it was made with.
    let codeword = "KITE-7391";
    let metrics = measured_replacing("chief-switch", &[("TERN-4821", codeword)]);
    assert_eq!(
        metrics.chiefs,
        [Some("claude-code".to_owned()), Some("codex".to_owned())]
    );
    assert_eq!(metrics.switches, 1);
    assert_eq!(
        metrics.history_reads,
        [serde_json::json!({ "page": 2, "find": null, "tools": false })]
    );
    assert_eq!(
        metrics.chief_words_now,
        format!("The codeword is {codeword}; it goes out on Friday.")
    );
    let switch = scenarios::chief_switch_with(codeword);
    assert!(switch.verdict(&metrics).iter().all(|check| check.ok));
    // Another chief's codeword is not the one this chief named.
    let other = scenarios::chief_switch_with("LARK-1000");
    assert!(!other.verdict(&metrics)[2].ok);
}

#[test]
fn a_recording_is_read_from_the_file_the_run_left_and_a_folder_with_none_is_an_error() {
    let folder = Folder::new();
    let file = recorded(&folder, "controls", &[]);
    assert!(measure(&file, &Options::default()).is_ok());
    let failed = measure(&folder.path().join("missing.db"), &Options::default()).unwrap_err();
    assert!(!failed.to_string().is_empty());
}
