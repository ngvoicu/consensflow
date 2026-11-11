//! Measuring the rounds of the chief's tasks: when each was asked for and with what
//! brief, when its results came in, each time it was sent back and with what words, and
//! when it was accepted. Read only for a scenario that holds the chief to what it did with
//! a result; any other report keeps its shape.

use cf_evals::measure::{measure, Asking, Options, Reopen};
use serde_json::json;

use crate::support::{measured, message, project_with, task, Folder};

/// A ledger in which the chief (2) asked a worker (3) for T-1, which came back, was sent
/// back once, was paused and resumed once, came back again and was accepted; asked a reviewer
/// for T-2, whose first result was withdrawn and whose second stood; and a member (3) asked
/// for T-3, which is no task of the chief's.
fn ledger() -> Folder {
    let folder = Folder::new();
    let (db, project) = project_with(&folder, &["zeus"]);
    let at = |time: &str| format!("2026-10-31T10:{time}.000Z");
    let event = |time: &str, data: serde_json::Value| {
        db.execute(
            "INSERT INTO event (project_id, at, kind, data) VALUES (?1, ?2, 'task.state', ?3)",
            rusqlite::params![project, at(time), data.to_string()],
        )
        .unwrap();
    };
    // T-1: asked for at 00:10 with a brief, finished, sent back at 01:00 with other words,
    // paused and resumed (no send-back), finished again, accepted at 03:00.
    db.execute(
        "INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, pool, tier, created_at, updated_at)
         VALUES (?1, 1, 'T-1', 'the first brief', 2, 3, 'accepted', 'worker', 'standard', ?2, ?2)",
        rusqlite::params![project, at("00:10")],
    )
    .unwrap();
    let one = db.last_insert_rowid();
    message(
        &db,
        project,
        (3, 2),
        "task",
        one,
        "delivered",
        &at("00:10"),
        Some(&at("00:11")),
    );
    message(
        &db,
        project,
        (2, 3),
        "result",
        one,
        "delivered",
        &at("00:50"),
        None,
    );
    message(
        &db,
        project,
        (3, 2),
        "task",
        one,
        "delivered",
        &at("01:00"),
        Some(&at("01:01")),
    );
    let sent_back = db.last_insert_rowid();
    db.execute(
        "UPDATE message SET body = 'the words of the send-back' WHERE id = ?1",
        [sent_back],
    )
    .unwrap();
    event(
        "01:00",
        json!({"task": 1, "from": "done", "to": "queued", "by": "chief", "message": sent_back}),
    );
    event(
        "01:30",
        json!({"task": 1, "from": "working", "to": "paused", "by": "chief"}),
    );
    event(
        "01:40",
        json!({"task": 1, "from": "paused", "to": "queued", "by": "chief", "message": sent_back}),
    );
    message(
        &db,
        project,
        (2, 3),
        "result",
        one,
        "delivered",
        &at("02:30"),
        None,
    );
    event(
        "03:00",
        json!({"task": 1, "from": "done", "to": "accepted", "by": "chief"}),
    );
    // T-2: a review, whose first result was withdrawn (the chief decided before it came).
    db.execute(
        "INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, pool, tier, created_at, updated_at)
         VALUES (?1, 2, 'T-2', 'review T-1', 2, 3, 'done', 'reviewer', 'standard', ?2, ?2)",
        rusqlite::params![project, at("00:55")],
    )
    .unwrap();
    let two = db.last_insert_rowid();
    message(
        &db,
        project,
        (2, 3),
        "result",
        two,
        "cancelled",
        &at("01:10"),
        None,
    );
    message(
        &db,
        project,
        (2, 3),
        "result",
        two,
        "delivered",
        &at("01:20"),
        None,
    );
    // T-3: asked for by a member.
    db.execute(
        "INSERT INTO task (project_id, number, title, body, requester_id, assignee_id, state, created_at, updated_at)
         VALUES (?1, 3, 'T-3', 'not the chief''s', 3, 3, 'working', ?2, ?2)",
        rusqlite::params![project, at("02:00")],
    )
    .unwrap();
    folder
}

#[test]
fn the_rounds_of_a_task_are_its_brief_its_results_that_stood_its_send_backs_and_its_acceptance() {
    let folder = ledger();
    let metrics = measure(
        &folder.ledger(),
        &Options {
            rounds: true,
            ..Options::default()
        },
    )
    .unwrap();
    let rounds = metrics.rounds.unwrap();
    assert_eq!(rounds.len(), 2, "the chief's tasks only, by number");
    let one = &rounds[0];
    assert_eq!(
        (one.number, one.pool.as_deref(), one.state.as_str()),
        (1, Some("worker"), "accepted")
    );
    assert_eq!(one.created_at, "2026-10-31T10:00:10.000Z");
    assert_eq!(one.brief, "the first brief");
    assert_eq!(
        one.results,
        ["2026-10-31T10:00:50.000Z", "2026-10-31T10:02:30.000Z"]
    );
    // One send-back, at its time and with its words; the resume out of a pause is none.
    assert_eq!(
        one.reopens,
        [Reopen {
            at: "2026-10-31T10:01:00.000Z".to_owned(),
            body: "the words of the send-back".to_owned()
        }]
    );
    assert_eq!(one.accepted_at.as_deref(), Some("2026-10-31T10:03:00.000Z"));
    assert!(one.is_work() && !one.is_review());
    let two = &rounds[1];
    assert_eq!(
        two.results,
        ["2026-10-31T10:01:20.000Z"],
        "a withdrawn result is none"
    );
    assert!(two.is_review() && !two.is_work());
    assert_eq!(two.first_result(), Some("2026-10-31T10:01:20.000Z"));
    assert!(two.reopens.is_empty());
    assert_eq!(two.accepted_at, None);
    // Sent back after a time: strictly after.
    assert_eq!(one.reopens_after("2026-10-31T10:00:59.000Z").len(), 1);
    assert_eq!(one.reopens_after("2026-10-31T10:01:00.000Z").len(), 0);
}

#[test]
fn a_task_sent_back_that_had_failed_is_a_send_back_too_and_the_last_acceptance_is_the_one_kept() {
    let folder = Folder::new();
    let (db, project) = project_with(&folder, &["zeus"]);
    let at = |time: &str| format!("2026-10-31T10:{time}.000Z");
    task(&db, project, 1, 3, "accepted");
    let ask = |time: &str, data: serde_json::Value| {
        db.execute(
            "INSERT INTO event (project_id, at, kind, data) VALUES (?1, ?2, 'task.state', ?3)",
            rusqlite::params![project, at(time), data.to_string()],
        )
        .unwrap();
    };
    message(
        &db,
        project,
        (3, 2),
        "task",
        1,
        "delivered",
        &at("00:30"),
        None,
    );
    let sent_back = db.last_insert_rowid();
    ask(
        "00:30",
        json!({"task": 1, "from": "failed", "to": "queued", "by": "chief", "message": sent_back}),
    );
    ask(
        "01:00",
        json!({"task": 1, "from": "done", "to": "accepted", "by": "chief"}),
    );
    ask(
        "02:00",
        json!({"task": 1, "from": "done", "to": "accepted", "by": "chief"}),
    );
    drop(db);
    let metrics = measure(
        &folder.ledger(),
        &Options {
            rounds: true,
            ..Options::default()
        },
    )
    .unwrap();
    let rounds = metrics.rounds.unwrap();
    assert_eq!(rounds[0].reopens.len(), 1);
    assert_eq!(
        rounds[0].accepted_at.as_deref(),
        Some("2026-10-31T10:02:00.000Z")
    );
}

#[test]
fn a_scenario_that_does_not_ask_gets_no_rounds_and_no_key_for_them_in_its_report() {
    let folder = ledger();
    let plain = measure(&folder.ledger(), &Options::default()).unwrap();
    assert_eq!(plain.rounds, None);
    assert!(plain.to_value().get("rounds").is_none());
    // The recorded runs keep the shape they were recorded in.
    assert!(measured("controls").to_value().get("rounds").is_none());
    // Asked, the rounds are the report's last key, after the files.
    let asked = measure(
        &folder.ledger(),
        &Options {
            rounds: true,
            ..Options::default()
        },
    )
    .unwrap();
    let report = asked.to_value();
    let keys: Vec<&String> = report.as_object().unwrap().keys().collect();
    assert_eq!(keys[keys.len() - 2..], ["rounds", "askedAt"]);
    assert_eq!(report["rounds"][0]["brief"], "the first brief");
    assert_eq!(
        report["rounds"][0]["reopens"][0]["body"],
        "the words of the send-back"
    );
    assert_eq!(
        report["rounds"][0]["acceptedAt"],
        "2026-10-31T10:03:00.000Z"
    );
    assert_eq!(report["rounds"][1]["acceptedAt"], serde_json::Value::Null);
    // Nothing else in the numbers moved because the rounds were read.
    assert_eq!(asked.tasks, plain.tasks);
    assert_eq!(asked.plumbing, plain.plumbing);
}

/// A ledger in which the chief (2) has a conversation whose turns end as `ends` say: for each, the
/// words, whether it is complete, the time the harness gave it and the time it was copied.
fn talking(ends: &[(&str, bool, Option<&str>, &str)]) -> Folder {
    let folder = Folder::new();
    let (db, project) = project_with(&folder, &["zeus"]);
    let _ = project;
    db.execute(
        "INSERT INTO conversation (participant_id, harness, native_session, started_at)
         VALUES (2, 'claude-code', NULL, '2026-10-31T10:00:00.000Z')",
        [],
    )
    .unwrap();
    for (at, (words, complete, given, copied)) in ends.iter().enumerate() {
        db.execute(
            "INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at)
             VALUES (1, ?1, ?2, 'assistant', ?3, ?4, ?5, ?6)",
            rusqlite::params![format!("item-{at}"), i64::try_from(at).unwrap(), words, i64::from(*complete), given, copied],
        )
        .unwrap();
    }
    folder
}

#[test]
fn the_questions_the_chief_put_to_the_owner_are_kept_with_the_time_of_each_and_counted_between_two_moments(
) {
    let folder = talking(&[
        (
            "Which scale is right?",
            true,
            Some("2026-10-31T10:01:00.000Z"),
            "2026-10-31T10:09:00.000Z",
        ),
        (
            "Working on it.",
            true,
            Some("2026-10-31T10:02:00.000Z"),
            "2026-10-31T10:09:00.000Z",
        ),
        (
            "May I accept it? Or shall I wait? Which?",
            true,
            Some("2026-10-31T10:03:00.000Z"),
            "2026-10-31T10:09:00.000Z",
        ),
        // Not a turn's end yet: the words so far, with a question in them.
        (
            "Shall I go on?",
            false,
            Some("2026-10-31T10:04:00.000Z"),
            "2026-10-31T10:09:00.000Z",
        ),
    ]);
    let metrics = measure(
        &folder.ledger(),
        &Options {
            rounds: true,
            ..Options::default()
        },
    )
    .unwrap();
    let asked = metrics.asked_at.clone().unwrap();
    assert_eq!(
        asked,
        [
            Asking {
                at: "2026-10-31T10:01:00.000Z".to_owned(),
                questions: 1
            },
            Asking {
                at: "2026-10-31T10:03:00.000Z".to_owned(),
                questions: 3
            },
        ]
    );
    // The same turns, counted as the run always counted them.
    assert_eq!(metrics.owner_questions.questions, 4);
    // Between two moments: a question at a moment itself is outside, and none for a bound is open.
    let at = |time: &str| Some(format!("2026-10-31T{time}.000Z"));
    let between = |from: Option<String>, until: Option<String>| {
        metrics.questions_between(from.as_deref(), until.as_deref())
    };
    assert_eq!(between(None, None), 4);
    assert_eq!(between(at("10:01:00"), None), 3);
    assert_eq!(between(at("10:00:59"), at("10:03:00")), 1);
    assert_eq!(between(at("10:01:00"), at("10:03:01")), 3);
    assert_eq!(between(at("10:03:00"), None), 0);
    assert_eq!(between(None, at("10:01:00")), 0);
    assert_eq!(between(at("10:05:00"), None), 0);
    // A scenario that does not ask for them gets none, and counts none.
    let plain = measure(&folder.ledger(), &Options::default()).unwrap();
    assert_eq!(plain.asked_at, None);
    assert_eq!(plain.questions_between(None, None), 0);
    assert!(plain.to_value().get("askedAt").is_none());
    // Asked, they are the report's last key, after the rounds.
    let report = metrics.to_value();
    let keys: Vec<&String> = report.as_object().unwrap().keys().collect();
    assert_eq!(keys[keys.len() - 2..], ["rounds", "askedAt"]);
    assert_eq!(
        report["askedAt"][1],
        json!({ "at": "2026-10-31T10:03:00.000Z", "questions": 3 })
    );
}

#[test]
fn a_turn_the_harness_gave_no_time_is_placed_where_the_daemon_copied_it() {
    let folder = talking(&[
        ("Which one?", true, None, "2026-10-31T10:05:00.000Z"),
        (
            "And which other?",
            true,
            Some("2026-10-31T10:06:00.000Z"),
            "2026-10-31T10:30:00.000Z",
        ),
    ]);
    let metrics = measure(
        &folder.ledger(),
        &Options {
            rounds: true,
            ..Options::default()
        },
    )
    .unwrap();
    let times: Vec<String> = metrics
        .asked_at
        .unwrap()
        .into_iter()
        .map(|asking| asking.at)
        .collect();
    assert_eq!(
        times,
        ["2026-10-31T10:05:00.000Z", "2026-10-31T10:06:00.000Z"]
    );
}

/// Work is what a worker or a frontend designer builds (spec new-roles, FD-04): a fix a
/// review sends either of them is rework. The architect's design, which runs nothing, the
/// researcher's findings and the integrator's landing, which changes no behaviour, are not,
/// nor is any role of the product's table this crate does not know.
#[test]
fn work_is_a_workers_or_a_frontend_designers_and_no_other_roles() {
    use cf_evals::measure::is_work_pool;
    for pool in [None, Some("worker"), Some("frontend")] {
        assert!(is_work_pool(pool), "{pool:?}");
    }
    for pool in [
        "architect",
        "researcher",
        "integrator",
        "advisor",
        "reviewer",
        "tester",
        "designer",
        "scout",
    ] {
        assert!(!is_work_pool(Some(pool)), "{pool}");
    }
}
