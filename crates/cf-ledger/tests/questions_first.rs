//! The order a window's queue goes in (3.0.7 chief-meeting, CMT-07): a member's
//! question waits for no older message, since its member is blocked on it; the
//! rest go oldest first. An urgent question is the chief's `cf tell` to a task's
//! window, words of the chief's and no member's question: it keeps its place.

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewNote, NewProject, NewQuestion, NewTask, Options,
};

fn ledger() -> (tempfile::TempDir, Ledger, i64, i64) {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
    let created = ledger
        .create_project(&NewProject {
            directory: "/work/app".to_owned(),
            name: "app".to_owned(),
            chief: NewChief {
                harness: "claude-code".to_owned(),
                agent: Some("apollo".to_owned()),
            },
            staff: vec![NewMember {
                agent: "zeus".to_owned(),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "standard".to_owned(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect("a project");
    let chief = created
        .participants
        .iter()
        .find(|participant| participant.role == "chief")
        .expect("a chief")
        .id;
    (dir, ledger, created.id, chief)
}

fn note(ledger: &mut Ledger, project: i64, body: &str) -> i64 {
    ledger
        .note(
            project,
            &NewNote {
                from: Some("zeus".to_owned()),
                to: "chief".to_owned(),
                body: body.to_owned(),
                task: None,
            },
        )
        .expect("a note")
        .id
}

fn question(ledger: &mut Ledger, project: i64, body: &str, urgent: bool) -> i64 {
    ledger
        .ask(
            project,
            &NewQuestion {
                from: Some("zeus".to_owned()),
                to: "chief".to_owned(),
                body: Some(body.to_owned()),
                urgent,
                ..NewQuestion::default()
            },
        )
        .expect("a question")
        .id
}

/// The ids a window's queue hands over, in order, each confirmed before the next.
fn handed(ledger: &mut Ledger, recipient: i64) -> Vec<i64> {
    let mut order = Vec::new();
    while let Some(next) = ledger.next_delivery(recipient).expect("a read") {
        ledger.begin_delivery(next.id).expect("begun");
        ledger.confirm_delivery(next.id, None).expect("confirmed");
        order.push(next.id);
    }
    order
}

#[test]
fn a_members_question_goes_ahead_of_older_messages_and_the_rest_go_oldest_first() {
    let (_dir, mut ledger, project, chief) = ledger();
    let first = note(&mut ledger, project, "A note");
    let second = note(&mut ledger, project, "Another note");
    let asked = question(&mut ledger, project, "Which way?", false);
    let third = note(&mut ledger, project, "A third note");
    let later = question(&mut ledger, project, "And this?", false);
    assert_eq!(
        handed(&mut ledger, chief),
        [asked, later, first, second, third]
    );
}

/// An urgent question (the chief's `cf tell` as the ledger takes it, to whichever
/// window it names) is no member's question: it keeps its place by age. A tell
/// withdraws what its own window keeps of the task, so the place is held in the
/// queue the ledger lets it share: here, the chief's.
#[test]
fn an_urgent_question_keeps_its_place_by_age() {
    let (_dir, mut ledger, project, chief) = ledger();
    ledger
        .create_task(
            project,
            &NewTask {
                from: "chief".to_owned(),
                pool: Some("worker".to_owned()),
                tier: Some("standard".to_owned()),
                body: "Parser".to_owned(),
                ..NewTask::default()
            },
        )
        .expect("a task");
    let first = note(&mut ledger, project, "A note");
    let told = ledger
        .ask(
            project,
            &NewQuestion {
                from: Some("zeus".to_owned()),
                to: "chief".to_owned(),
                body: Some("Stop: the grammar changed".to_owned()),
                task: Some(1),
                urgent: true,
                ..NewQuestion::default()
            },
        )
        .expect("a tell")
        .id;
    let asked = question(&mut ledger, project, "Which way?", false);
    assert_eq!(handed(&mut ledger, chief), [asked, first, told]);
}
