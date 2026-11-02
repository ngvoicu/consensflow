//! How a meeting begins and ends: the human's Start and Stop, each logged once; a
//! closed project's refused; and the project's Close and the daemon's start after a
//! quit, which end it with why. What it held stays queued, and goes in at the
//! Resume, a member's question first.

use serde_json::json;

use crate::world::world;
use crate::{a_result, handed, head, held, meetings, note_to_chief};

/// The project says when its meeting began while it is on, and nothing after. A
/// Start of one that is on keeps its time and logs nothing; a Stop logs how many
/// messages it held; a Stop with none on logs nothing.
#[test]
fn a_meeting_is_logged_once_when_it_starts_and_when_it_stops_with_what_it_held() {
    let mut w = world();
    assert_eq!(
        w.ledger.set_meeting(w.project.id, false).unwrap().meeting,
        None
    );
    assert_eq!(meetings(&w), Vec::<serde_json::Value>::new());
    let begun = w.ledger.set_meeting(w.project.id, true).unwrap().meeting;
    assert!(begun.is_some());
    w.advance(60_000);
    assert_eq!(
        w.ledger.set_meeting(w.project.id, true).unwrap().meeting,
        begun
    );
    let project = w.ledger.project(w.project.id).unwrap().unwrap();
    assert_eq!(project.meeting, begun);
    note_to_chief(&mut w, Some("zeus"), None, "One");
    note_to_chief(&mut w, Some("zeus"), None, "Two");
    let stopped = w.ledger.set_meeting(w.project.id, false).unwrap();
    assert_eq!(stopped.meeting, None);
    w.ledger.set_meeting(w.project.id, false).unwrap();
    assert_eq!(
        meetings(&w),
        vec![
            json!({ "on": true }),
            json!({ "on": false, "ended": "stopped", "held": 2 })
        ]
    );
}

/// A meeting is had with an open project's chief.
#[test]
fn a_closed_projects_meeting_is_refused() {
    let mut w = world();
    w.ledger
        .set_project_state(w.project.id, "suspended")
        .unwrap();
    let refused = w.ledger.set_meeting(w.project.id, true).unwrap_err();
    assert_eq!(refused.code(), Some("project-closed"));
    let cf_ledger::LedgerError::Refused(refusal) = refused else {
        panic!("a refusal")
    };
    assert_eq!(refusal.status, 409);
    assert_eq!(meetings(&w), Vec::<serde_json::Value>::new());
}

/// The project's Close ends its meeting, logged as ended by the Close; a second
/// Close logs nothing more. What it held stays queued, and at the Resume it goes as
/// any queued message does, the question first.
#[test]
fn a_close_ends_the_meeting_and_what_it_held_goes_at_the_resume_a_question_first() {
    let mut w = world();
    w.ledger.set_meeting(w.project.id, true).unwrap();
    let result = a_result(&mut w);
    let given = w.give(&[]);
    w.start(&given);
    let question = w.ask(given.task.number).id;
    let closed = w
        .ledger
        .set_project_state(w.project.id, "suspended")
        .unwrap();
    assert_eq!(closed.meeting, None);
    w.ledger
        .set_project_state(w.project.id, "suspended")
        .unwrap();
    assert_eq!(
        meetings(&w).last(),
        Some(&json!({ "on": false, "ended": "closed", "held": 2 }))
    );
    assert_eq!(meetings(&w).len(), 2);
    assert_eq!(held(&w), Vec::<i64>::new());
    for id in [result, question] {
        assert_eq!(w.ledger.message(id).unwrap().unwrap().state, "queued");
    }
    w.ledger.set_project_state(w.project.id, "open").unwrap();
    assert_eq!(head(&w), Some(question));
    assert_eq!(handed(&mut w), vec![question, result]);
}

/// The daemon's start after the app quit ends every meeting the quit left on,
/// logged as ended by the quit; at the Resume, what it held goes, the question
/// first.
#[test]
fn the_daemons_start_ends_the_meeting_the_quit_left_on() {
    let mut w = world();
    w.ledger.set_meeting(w.project.id, true).unwrap();
    let note = note_to_chief(&mut w, Some("zeus"), None, "The build is green");
    let given = w.give(&[]);
    w.start(&given);
    let question = w.ask(given.task.number).id;
    let suspended = w.ledger.suspend_for_restart().unwrap();
    assert_eq!(suspended.len(), 1);
    assert_eq!(suspended[0].meeting, None);
    assert_eq!(
        meetings(&w).last(),
        Some(&json!({ "on": false, "ended": "quit", "held": 2 }))
    );
    w.ledger.set_project_state(w.project.id, "open").unwrap();
    assert_eq!(handed(&mut w), vec![question, note.id]);
}
