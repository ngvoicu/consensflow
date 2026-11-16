//! The board's state clock comes from the ledger's moves, including old ones.
#![allow(clippy::expect_used)]

use std::{cell::Cell, rc::Rc};

use cf_base::time::Clock;
use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, NewQuestion, NewTask, Options,
};
use serde_json::{json, Value};

struct Moving(Rc<Cell<i64>>);
impl Clock for Moving {
    fn now_ms(&mut self) -> i64 {
        self.0.get()
    }
}

fn card(ledger: &Ledger, project: i64, number: i64) -> Value {
    let board = ledger.board(project).expect("board");
    let card = board
        .open
        .iter()
        .chain(board.lanes.iter().flat_map(|lane| &lane.tasks))
        .find(|card| card.number == number)
        .expect("card");
    serde_json::to_value(card).expect("card JSON")
}

#[test]
fn board_state_time_is_the_last_move_not_the_last_edit_and_survives_restart() {
    let dir = tempfile::tempdir().expect("home");
    let file = dir.path().join("ledger.db");
    let now = Rc::new(Cell::new(1_791_892_800_000));
    let opts = || Options {
        clock: Box::new(Moving(Rc::clone(&now))),
        ..Options::default()
    };
    let mut ledger = open_ledger(&file, opts()).expect("ledger");
    let project = ledger
        .create_project(&NewProject {
            directory: "/work/state-clock".into(),
            name: "clock".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("apollo".into()),
                effort: None,
                model_name: None,
            },
            staff: vec![NewMember {
                model: None,
                effort: None,
                model_name: Some("zeus".into()),
                harness: "claude-code".into(),
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
        .expect("project");
    let worker = project
        .participants
        .iter()
        .find(|p| p.handle == "zeus")
        .expect("worker")
        .id;
    let number = ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "The work".into(),
                ..NewTask::default()
            },
        )
        .expect("open")
        .task
        .number;
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:00:00.000Z",
        "task.opened"
    );

    now.set(now.get() + 60_000);
    ledger
        .edit_task(project.id, number, "chief", "Edited brief")
        .expect("brief edit");
    ledger
        .retitle_task(project.id, number, "chief", "Retitled")
        .expect("title edit");
    let edited = card(&ledger, project.id, number);
    assert_eq!(edited["updatedAt"], "2026-10-13T12:01:00.000Z");
    assert_eq!(
        edited["stateSince"], "2026-10-13T12:00:00.000Z",
        "an edit is not a move"
    );

    now.set(now.get() + 60_000);
    ledger
        .assign_task(project.id, number, worker)
        .expect("assign");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:02:00.000Z",
        "task.assigned"
    );
    now.set(now.get() + 60_000);
    ledger
        .tell_queued(project.id, number, "chief", "A note for the window")
        .expect("tell");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:02:00.000Z",
        "a message keeps queued"
    );
    now.set(now.get() + 60_000);
    ledger
        .release_task(project.id, number, "another worker")
        .expect("release");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:04:00.000Z",
        "task.released"
    );
    ledger.close().expect("close");
    // Old rows may contain invalid JSON, and an event that keeps a state is not a move.
    let db = rusqlite::Connection::open(&file).expect("open ledger file");
    db.execute(
        "INSERT INTO event (project_id, at, kind, data) VALUES (?1, ?2, 'task.state', ?3)",
        rusqlite::params![
            project.id,
            "2026-10-13T12:05:00.000Z",
            json!({"task": number, "from": "open", "to": "open"}).to_string()
        ],
    )
    .expect("same state event");
    db.execute(
        "INSERT INTO event (project_id, at, kind, data) VALUES (?1, ?2, 'task.state', 'not json')",
        rusqlite::params![project.id, "2026-10-13T12:06:00.000Z"],
    )
    .expect("old damaged event");
    drop(db);
    let ledger = open_ledger(&file, opts()).expect("restart");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:04:00.000Z",
        "the ledger survives restart, ignoring same-state and unreadable events"
    );
}

#[test]
fn board_state_time_follows_named_creation_and_later_state_moves() {
    // Kept separate from the assignment/release case so each kind of move has an independent oracle.
    let dir = tempfile::tempdir().expect("home");
    let now = Rc::new(Cell::new(1_791_892_800_000));
    let mut ledger = open_ledger(
        &dir.path().join("ledger.db"),
        Options {
            clock: Box::new(Moving(Rc::clone(&now))),
            ..Options::default()
        },
    )
    .expect("ledger");
    let project = ledger
        .create_project(&NewProject {
            directory: "/work/named-clock".into(),
            name: "clock".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("apollo".into()),
                effort: None,
                model_name: None,
            },
            staff: vec![NewMember {
                model: None,
                effort: None,
                model_name: Some("zeus".into()),
                harness: "claude-code".into(),
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
        .expect("project");
    let created = ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                to: Some("zeus".into()),
                body: "Named".into(),
                ..NewTask::default()
            },
        )
        .expect("named");
    let number = created.task.number;
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:00:00.000Z",
        "task.created"
    );
    now.set(now.get() + 60_000);
    let brief = created.message.expect("brief").id;
    ledger.begin_delivery(brief).expect("begin");
    ledger
        .confirm_delivery(brief, Some(&json!({"item":"one"})))
        .expect("delivered");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:01:00.000Z",
        "task.state to working"
    );
    now.set(now.get() + 60_000);
    let question = ledger
        .ask(
            project.id,
            &NewQuestion {
                from: Some("zeus".into()),
                to: "chief".into(),
                body: Some("Which?".into()),
                task: Some(number),
                ..NewQuestion::default()
            },
        )
        .expect("question");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:02:00.000Z",
        "waiting"
    );
    now.set(now.get() + 60_000);
    let chief = project
        .participants
        .iter()
        .find(|p| p.role == "chief")
        .expect("chief")
        .id;
    let answer = ledger
        .answer(question.id, chief, Some(&json!("Yes")), None)
        .expect("answer");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:02:00.000Z",
        "answer in transit keeps waiting"
    );
    ledger.begin_delivery(answer.id).expect("begin answer");
    ledger
        .confirm_delivery(answer.id, Some(&json!({"item":"answer"})))
        .expect("answer received");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:03:00.000Z",
        "answer moves to working"
    );
    now.set(now.get() + 60_000);
    ledger
        .pause_task(project.id, number, Some("chief"), None)
        .expect("pause");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:04:00.000Z",
        "paused"
    );
    now.set(now.get() + 60_000);
    let resumed = ledger
        .resume_task(project.id, number, Some("chief"), "Go on")
        .expect("resume");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:05:00.000Z",
        "resume moves"
    );
    now.set(now.get() + 60_000);
    ledger
        .work_while_resumed(project.id, number)
        .expect("working in window");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:06:00.000Z",
        "resumed window working"
    );
    let resume_words = resumed.message.expect("resume words").id;
    ledger.begin_delivery(resume_words).expect("begin resume");
    ledger
        .confirm_delivery(resume_words, Some(&json!({"item":"resume"})))
        .expect("resume received");
    now.set(now.get() + 60_000);
    ledger
        .record_result(project.id, number, "Done")
        .expect("result");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:07:00.000Z",
        "done"
    );
    now.set(now.get() + 60_000);
    let reopened = ledger
        .reopen_task(project.id, number, "chief", "More")
        .expect("reopen");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:08:00.000Z",
        "reopen moves"
    );
    let more = reopened.message.expect("follow-up").id;
    ledger.begin_delivery(more).expect("begin follow-up");
    ledger
        .confirm_delivery(more, Some(&json!({"item":"more"})))
        .expect("follow-up received");
    now.set(now.get() + 60_000);
    ledger
        .record_result(project.id, number, "Done again")
        .expect("result again");
    now.set(now.get() + 60_000);
    ledger
        .accept_task(project.id, number, "chief")
        .expect("accept");
    assert_eq!(
        card(&ledger, project.id, number)["stateSince"],
        "2026-10-13T12:10:00.000Z",
        "accepted"
    );
}

struct Ticking(Rc<Cell<i64>>);
impl Clock for Ticking {
    fn now_ms(&mut self) -> i64 {
        let at = self.0.get();
        self.0.set(at + 1_000);
        at
    }
}

#[test]
fn board_state_time_uses_the_initial_event_even_when_the_row_was_written_earlier() {
    let dir = tempfile::tempdir().expect("home");
    let mut ledger = open_ledger(
        &dir.path().join("ledger.db"),
        Options {
            clock: Box::new(Ticking(Rc::new(Cell::new(1_791_892_800_000)))),
            ..Options::default()
        },
    )
    .expect("ledger");
    let request = NewProject {
        directory: "/work/initial-clock".into(),
        name: "clock".into(),
        chief: NewChief {
            harness: "claude-code".into(),
            model: Some("apollo".into()),
            effort: None,
            model_name: None,
        },
        staff: vec![NewMember {
            model: None,
            effort: None,
            model_name: Some("zeus".into()),
            harness: "claude-code".into(),
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
    };
    let project = ledger.create_project(&request).expect("project");
    let open = ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Open".into(),
                ..NewTask::default()
            },
        )
        .expect("open")
        .task
        .number;
    let named = ledger
        .create_task(
            project.id,
            &NewTask {
                from: "chief".into(),
                to: Some("zeus".into()),
                body: "Named".into(),
                ..NewTask::default()
            },
        )
        .expect("named")
        .task
        .number;
    let mut other_request = request.clone();
    other_request.directory = "/work/other-clock".into();
    let other = ledger
        .create_project(&other_request)
        .expect("other project");
    let other_task = ledger
        .create_task(
            other.id,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: "Other project's T-1".into(),
                ..NewTask::default()
            },
        )
        .expect("other task")
        .task
        .number;
    assert_eq!(
        other_task, open,
        "the two projects have the same task number"
    );
    let events = ledger.events(project.id, 0, 500).expect("events");
    for (number, kind) in [(open, "task.opened"), (named, "task.created")] {
        let event = events
            .iter()
            .find(|event| event.kind == kind && event.data["task"] == number)
            .expect("initial event");
        let card = card(&ledger, project.id, number);
        assert_ne!(
            card["createdAt"], event.at,
            "the row and event have distinct clock readings"
        );
        assert_eq!(card["stateSince"], event.at, "{kind} is the start");
    }
}
