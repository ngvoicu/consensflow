//! Each expectation holds on the numbers of a run that did what the scenario asks
//! and fails on those of a run that did not, by the name a report says it by.

use cf_evals::measure::{
    LongestResult, MemberQuestions, Metrics, Note, OwnerQuestions, Plumbing, Task,
};

use crate::scenario;

/// The names of the expectations of `id` that do not hold on `metrics`.
fn failing(id: &str, metrics: &Metrics) -> Vec<String> {
    scenario(id)
        .verdict(metrics)
        .into_iter()
        .filter(|check| !check.ok)
        .map(|check| check.name)
        .collect()
}

fn task(pool: Option<&str>, state: &str) -> Task {
    Task {
        pool: pool.map(str::to_owned),
        state: state.to_owned(),
        ..Task::default()
    }
}

fn asked(role: &str, longest: i64) -> MemberQuestions {
    MemberQuestions {
        role: role.to_owned(),
        harness: Some("pi".to_owned()),
        asked: 1,
        answered: 1,
        delivered: 1,
        longest,
    }
}

fn in_the_terminal(questions: i64, pickers: i64) -> OwnerQuestions {
    OwnerQuestions {
        questions,
        turns_asking: i64::from(questions > 0),
        pickers,
        texts: Vec::new(),
    }
}

#[test]
fn the_question_trip_holds_when_every_kind_of_member_asked_got_its_answer_and_the_long_question_was_read_whole(
) {
    let good = Metrics {
        tasks: vec![
            task(None, "accepted"),
            task(Some("advisor"), "accepted"),
            task(Some("reviewer"), "accepted"),
        ],
        member_questions_by: vec![
            asked("advisor", 60),
            asked("reviewer", 6250),
            asked("worker", 60),
        ],
        notes_text: "The reviewer asked about the English version; its code: PLOP-6142.".to_owned(),
        plumbing: Plumbing {
            accepted: 3,
            ..Plumbing::default()
        },
        ..Metrics::default()
    };
    for id in ["question-trip", "question-trip-native"] {
        assert_eq!(scenario(id).id, id);
        assert!(
            failing(id, &good).is_empty(),
            "{id}: {:?}",
            failing(id, &good)
        );
    }
}

#[test]
fn the_question_trip_names_what_is_missing_a_role_that_never_asked_an_answer_not_delivered_a_short_question_no_code(
) {
    let good = Metrics {
        tasks: vec![
            task(None, "accepted"),
            task(Some("advisor"), "accepted"),
            task(Some("reviewer"), "accepted"),
        ],
        member_questions_by: vec![
            asked("advisor", 60),
            asked("reviewer", 6250),
            asked("worker", 60),
        ],
        notes_text: "its code: PLOP-6142.".to_owned(),
        plumbing: Plumbing {
            accepted: 3,
            ..Plumbing::default()
        },
        ..Metrics::default()
    };
    let trip = "question-trip";
    assert_eq!(
        failing(
            trip,
            &Metrics {
                member_questions_by: vec![asked("advisor", 60), asked("worker", 60)],
                ..good.clone()
            }
        ),
        [
            "the reviewer asks the chief, and the answer reaches its window",
            "the long question reaches the chief whole (over 4000 characters)",
        ]
    );
    let mut undelivered = asked("reviewer", 6250);
    undelivered.delivered = 0;
    assert_eq!(
        failing(
            trip,
            &Metrics {
                member_questions_by: vec![asked("advisor", 60), undelivered, asked("worker", 60)],
                ..good.clone()
            }
        ),
        ["the reviewer asks the chief, and the answer reaches its window"]
    );
    assert_eq!(
        failing(
            trip,
            &Metrics {
                notes_text: "Done.".to_owned(),
                ..good.clone()
            }
        ),
        ["the chief's note holds the long question's code"]
    );
    assert_eq!(
        failing(
            trip,
            &Metrics {
                owner_questions: in_the_terminal(1, 0),
                ..good.clone()
            }
        ),
        ["the owner is not asked anything"]
    );
    assert_eq!(
        failing(
            trip,
            &Metrics {
                tasks: vec![task(None, "done")],
                ..good
            }
        ),
        ["a worker, an advisor and a reviewer each get a task"],
        "a worker's task, and no advisor's or reviewer's"
    );
}

#[test]
fn the_terminal_questions_eval_holds_when_the_chief_asked_in_its_terminal_and_the_answer_reached_the_work(
) {
    let good = Metrics {
        owner_questions: in_the_terminal(1, 1),
        tasks: vec![task(None, "queued")],
        files_changed: vec!["site/cariere.html".to_owned()],
        ..Metrics::default()
    };
    assert!(failing("terminal-questions", &good).is_empty());
}

#[test]
fn the_terminal_questions_eval_fails_a_chief_that_asked_nothing_tried_cf_ask_or_whose_answer_never_reached_the_work(
) {
    let good = Metrics {
        owner_questions: in_the_terminal(1, 1),
        tasks: vec![task(None, "queued")],
        files_changed: vec!["site/cariere.html".to_owned()],
        ..Metrics::default()
    };
    assert_eq!(
        failing(
            "terminal-questions",
            &Metrics {
                owner_questions: in_the_terminal(0, 0),
                ask_refused: 1,
                ..good.clone()
            }
        ),
        [
            "the owner is asked in the terminal",
            "the chief never tries cf ask"
        ]
    );
    assert_eq!(
        failing(
            "terminal-questions",
            &Metrics {
                files_changed: vec!["site/careers.html".to_owned()],
                ..good
            }
        ),
        ["the owner's answer reaches the work: site/cariere.html exists"]
    );
}

#[test]
fn the_advice_trip_wants_one_advice_task_accepted_a_note_and_nothing_else() {
    let good = Metrics {
        advice: 1,
        tasks: vec![task(Some("advisor"), "accepted")],
        notes_to_human: vec![Note::default()],
        ..Metrics::default()
    };
    assert!(failing("advice-trip", &good).is_empty());
    let bad = Metrics {
        advice: 2,
        tasks: vec![task(Some("advisor"), "done")],
        notes_to_human: Vec::new(),
        files_changed: vec!["site/index.html".to_owned()],
        owner_questions: in_the_terminal(1, 0),
        ..Metrics::default()
    };
    assert_eq!(failing("advice-trip", &bad).len(), 5);
}

#[test]
fn the_complex_launch_wants_tasks_in_parallel_advice_a_review_questions_a_note_and_few_edits() {
    let good = Metrics {
        tasks: vec![
            task(None, "accepted"),
            task(None, "accepted"),
            task(Some("reviewer"), "accepted"),
        ],
        parallel: 2,
        advice: 1,
        reviews: 1,
        owner_questions: in_the_terminal(1, 1),
        notes_to_human: vec![Note::default()],
        chief_edits: Some(9),
        ..Metrics::default()
    };
    assert!(failing("complex-launch", &good).is_empty());
    // A chief of another harness has no count of edits, and is not held to one.
    assert!(failing(
        "complex-launch",
        &Metrics {
            chief_edits: None,
            ..good.clone()
        }
    )
    .is_empty());
    assert_eq!(
        failing(
            "complex-launch",
            &Metrics {
                chief_edits: Some(10),
                ..good
            }
        ),
        ["the chief's own edits stay under ten (counted for a Claude chief only)"]
    );
}

#[test]
fn the_conversation_wants_changes_on_the_board_the_english_page_a_workers_and_the_work_done() {
    let english = Task {
        title: "Translate the evaluation page to English".to_owned(),
        ..Task::default()
    };
    let good = Metrics {
        tasks: vec![task(None, "accepted"), task(None, "accepted"), english],
        files_changed: vec!["site/en/evaluare.html".to_owned()],
        ..Metrics::default()
    };
    assert!(failing("conversation", &good).is_empty());
    assert_eq!(
        failing(
            "conversation",
            &Metrics {
                tasks: vec![task(None, "accepted"); 3],
                files_changed: Vec::new(),
                ..Metrics::default()
            }
        ),
        [
            "the English page is a worker’s (a translation runs on its own)",
            "the work is done: the English evaluation page exists",
        ]
    );
    // The title may name the file instead of the language.
    let by_file = Task {
        title: "Create en/evaluare.html".to_owned(),
        ..Task::default()
    };
    let titled = Metrics {
        tasks: vec![task(None, "x"), task(None, "x"), by_file],
        files_changed: good.files_changed,
        ..Metrics::default()
    };
    assert!(failing("conversation", &titled).is_empty());
}

#[test]
fn the_long_trip_wants_three_long_results_the_long_answer_delivered_five_codes_and_no_file_changed()
{
    let good = Metrics {
        longest_result: LongestResult {
            worker: 8000,
            advisor: 9000,
            reviewer: 12000,
        },
        // The prompt, itself long, and then what the owner typed in answer.
        owner_messages: vec![8500, 6100],
        notes_text: "CEDRU-7314 ARTAR-2659 STEJAR-4087 DELTA-5530 FARUL-2208".to_owned(),
        ..Metrics::default()
    };
    assert!(failing("long-trip", &good).is_empty());
    assert_eq!(
        failing(
            "long-trip",
            &Metrics {
                longest_result: LongestResult {
                    worker: 7999,
                    advisor: 7999,
                    reviewer: 7999
                },
                // The prompt is not the answer, however long.
                owner_messages: vec![9000],
                notes_text: String::new(),
                files_changed: vec!["x".to_owned()],
                ..Metrics::default()
            }
        )
        .len(),
        10
    );
}

#[test]
fn the_simple_fix_wants_one_page_fixed_no_advice_no_question_one_task_at_most_and_a_word_on_it() {
    let good = Metrics {
        files_changed: vec!["site/evaluare.html".to_owned()],
        tasks: vec![task(None, "accepted")],
        chief_last_words: "Am corectat: scorul e între 0 și 10 acum.".to_owned(),
        ..Metrics::default()
    };
    assert!(failing("simple-fix", &good).is_empty());
    for words in ["0 si 10", "între 0–10", "0-10", "0 și 10"] {
        let said = Metrics {
            chief_last_words: words.to_owned(),
            ..good.clone()
        };
        assert!(failing("simple-fix", &said).is_empty(), "{words}");
    }
    assert_eq!(
        failing(
            "simple-fix",
            &Metrics {
                chief_last_words: "Done.".to_owned(),
                ..good.clone()
            }
        ),
        ["the chief says what it did"]
    );
    assert_eq!(
        failing(
            "simple-fix",
            &Metrics {
                files_changed: vec![
                    "site/evaluare.html".to_owned(),
                    "site/index.html".to_owned()
                ],
                advice: 1,
                tasks: vec![task(None, "x"), task(None, "x")],
                ..good
            }
        ),
        [
            "the page is fixed, and nothing else touched",
            "no advice is asked for a one-word fix",
            "at most one task goes on the board",
        ]
    );
}

#[test]
fn the_six_decisions_want_three_questions_in_the_terminal_none_on_the_board_work_in_parallel_and_a_review(
) {
    let good = Metrics {
        owner_questions: in_the_terminal(2, 1),
        notes_to_human: vec![Note::default()],
        tasks: vec![task(None, "x"), task(None, "x")],
        parallel: 2,
        reviews: 1,
        ..Metrics::default()
    };
    assert!(failing("six-decisions", &good).is_empty());
    assert_eq!(
        failing(
            "six-decisions",
            &Metrics {
                questions_on_board: 1,
                ask_refused: 1,
                chief_edits: Some(10),
                ..good
            }
        ),
        [
            "nothing is asked on the board",
            "the chief never tries cf ask",
            "the chief's own edits stay under ten (counted for a Claude chief only)",
        ]
    );
}

#[test]
fn the_control_trip_wants_a_tell_answered_a_pause_and_resume_a_follow_up_and_both_tasks_accepted() {
    let good = Metrics {
        plumbing: Plumbing {
            tells: 1,
            tells_answered: 1,
            pauses: 1,
            resumes: 1,
            continuations: 1,
            accepted: 2,
            ..Plumbing::default()
        },
        files_changed: vec!["site/notes.md".to_owned()],
        ..Metrics::default()
    };
    assert!(failing("control-trip", &good).is_empty());
    // A second new file is not "only notes.md".
    assert_eq!(
        failing(
            "control-trip",
            &Metrics {
                files_changed: vec!["site/notes.md".to_owned(), "site/x.md".to_owned()],
                ..good
            }
        ),
        ["only site/notes.md is new"]
    );
}

#[test]
fn the_round_trip_wants_a_worker_task_the_question_and_its_answer_on_the_board_a_review_and_acceptance(
) {
    let good = Metrics {
        tasks: vec![task(None, "accepted"), task(Some("reviewer"), "accepted")],
        reviews: 1,
        plumbing: Plumbing {
            member_questions: 1,
            member_questions_answered: 1,
            accepted: 1,
            ..Plumbing::default()
        },
        files_changed: vec!["site/notes.md".to_owned()],
        ..Metrics::default()
    };
    assert!(failing("round-trip", &good).is_empty());
    // A reviewer's task is no worker's.
    assert_eq!(
        failing(
            "round-trip",
            &Metrics {
                tasks: vec![task(Some("reviewer"), "accepted")],
                ..good
            }
        ),
        ["one task goes to a worker"]
    );
}
