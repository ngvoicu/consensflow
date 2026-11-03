//! A task's title can change after its scope does (T-1118, from Gabriel's other board:
//! the title is the brief's first line, and a reopen or a resume adds words but keeps
//! it). The title is set in any state, the brief and everything else as they are, and
//! the old title stays in the log.

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, NewTask, Options, ProjectView,
};
use serde_json::{json, Value};

fn project(ledger: &mut Ledger) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("apollo".into()),
            },
            staff: vec![NewMember {
                agent: "zeus".into(),
                harness: "claude-code".into(),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect("a project")
}

fn open(ledger: &mut Ledger, body: &str) -> i64 {
    ledger
        .create_task(
            1,
            &NewTask {
                from: "chief".into(),
                pool: Some("worker".into()),
                tier: Some("standard".into()),
                body: body.into(),
                ..NewTask::default()
            },
        )
        .expect("a task")
        .task
        .number
}

/// Zeus's new session takes T-`number` in, its brief received.
fn taken(ledger: &mut Ledger, view: &ProjectView, number: i64) {
    let member = view
        .participants
        .iter()
        .find(|participant| participant.handle == "zeus")
        .expect("zeus")
        .id;
    let brief = ledger
        .assign_task(1, number, member)
        .expect("assigned")
        .message
        .expect("its brief")
        .id;
    ledger.begin_delivery(brief).expect("its delivery begins");
    ledger
        .confirm_delivery(brief, Some(&json!({ "item": format!("i-{number}") })))
        .expect("its delivery is confirmed");
}

fn titled(ledger: &Ledger, number: i64) -> (String, String) {
    let task = ledger
        .task(1, number)
        .expect("a thread")
        .expect("a task")
        .task;
    (task.title, task.body)
}

fn retitled_events(ledger: &Ledger) -> Vec<Value> {
    ledger
        .events(1, 0, 500)
        .expect("the log")
        .into_iter()
        .filter(|event| event.kind == "task.retitled")
        .map(|event| event.data)
        .collect()
}

#[test]
fn a_task_is_retitled_and_its_old_title_stays_in_the_log() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("ledger");
    project(&mut ledger);
    let task = open(&mut ledger, "Fix the parser\nand its tests");

    let retitled = ledger
        .retitle_task(1, task, "chief", "Fix the parser and the lexer")
        .expect("retitled");
    assert_eq!(retitled.title, "Fix the parser and the lexer");
    assert_eq!(
        titled(&ledger, task),
        (
            "Fix the parser and the lexer".to_owned(),
            "Fix the parser\nand its tests".to_owned()
        ),
        "the brief is as it was"
    );
    assert_eq!(
        retitled_events(&ledger),
        [json!({
            "task": task,
            "by": "chief",
            "from": "Fix the parser",
            "to": "Fix the parser and the lexer",
        })]
    );

    // Again: the log has both, and the same title again says nothing more.
    ledger
        .retitle_task(1, task, "chief", "Parser and lexer")
        .expect("retitled");
    ledger
        .retitle_task(1, task, "chief", "Parser and lexer")
        .expect("the same");
    let log = retitled_events(&ledger);
    assert_eq!(log.len(), 2, "{log:?}");
    assert_eq!(log[1]["from"], "Fix the parser and the lexer");
    assert_eq!(log[1]["to"], "Parser and lexer");
}

#[test]
fn a_task_is_retitled_in_every_state_it_has() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("ledger");
    let view = project(&mut ledger);
    let working = open(&mut ledger, "Working one");
    taken(&mut ledger, &view, working);
    let done = open(&mut ledger, "Done one");
    taken(&mut ledger, &view, done);
    ledger.record_result(1, done, "Done.").expect("a result");
    let accepted = open(&mut ledger, "Accepted one");
    taken(&mut ledger, &view, accepted);
    ledger
        .record_result(1, accepted, "Done.")
        .expect("a result");
    ledger.accept_task(1, accepted, "chief").expect("accepted");
    let cancelled = open(&mut ledger, "Cancelled one");
    ledger
        .cancel_task(1, cancelled, "chief")
        .expect("cancelled");
    let waiting = open(&mut ledger, "Open one");

    for (number, state) in [
        (working, "working"),
        (done, "done"),
        (accepted, "accepted"),
        (cancelled, "cancelled"),
        (waiting, "open"),
    ] {
        let now = ledger
            .retitle_task(1, number, "chief", &format!("New title of the {state} one"))
            .unwrap_or_else(|refused| panic!("{state}: {refused}"));
        assert_eq!(now.title, format!("New title of the {state} one"));
        assert_eq!(now.state, state, "the state is as it was");
    }
}

#[test]
fn a_title_is_its_first_line_cut_to_fit_and_must_say_something() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("ledger");
    project(&mut ledger);
    let task = open(&mut ledger, "Parser");

    let long = "x".repeat(300);
    let now = ledger
        .retitle_task(1, task, "chief", &format!("\n  {long}\nmore"))
        .expect("retitled");
    assert_eq!(
        now.title,
        format!("{}…", "x".repeat(119)),
        "as a brief's first line is"
    );

    for blank in ["", "  ", "\n \n"] {
        let refused = ledger
            .retitle_task(1, task, "chief", blank)
            .expect_err("no title");
        assert_eq!(refused.code(), Some("invalid-text"), "{blank:?}");
    }
    assert_eq!(
        titled(&ledger, task).0,
        format!("{}…", "x".repeat(119)),
        "unchanged"
    );

    let refused = ledger
        .retitle_task(1, 9, "chief", "Nothing")
        .expect_err("no task");
    assert_eq!(refused.code(), Some("unknown-task"));
}
