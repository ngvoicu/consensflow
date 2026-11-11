//! Notes of ConsensFlow's own that ask a chief no decision (RS-25): to a chief they are
//! shown, not typed. Each is still a row of the ledger, the chief's inbox lists it, and
//! no delivery ever takes it; what it says is its task's line (`aside`) until the task
//! moves on. To anyone but a chief it is queued as any note.

use cf_ledger::{MessageView, NewNote, SHOWN};
use serde_json::json;

use crate::fixture::{world, World};

/// The end of the hold the tests name.
const UNTIL: &str = "2026-10-10T15:00:00.000Z";

/// T-1 for the standard workers, in zeus's session, its brief delivered.
fn with_zeus(w: &mut World) -> (i64, String) {
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    w.deliver(moved.message.expect("its brief").id);
    (task, session)
}

/// What task `number`'s line says of it instead of a note, while it holds.
fn aside(w: &World, number: i64) -> Option<String> {
    w.ledger
        .task(w.project, number)
        .expect("a read")
        .expect("the task")
        .task
        .aside
}

/// ConsensFlow's note about T-`number` to `to`, shown where it asks no decision.
fn shown(w: &mut World, to: &str, number: i64, body: &str, said: &str) -> MessageView {
    w.ledger
        .show(
            w.project,
            &NewNote {
                from: None,
                to: to.into(),
                body: body.into(),
                task: Some(number),
            },
            said,
        )
        .expect("a note")
}

#[test]
fn a_hold_note_to_the_chief_is_shown_on_its_task_and_never_typed() {
    let mut w = world();
    let (task, session) = with_zeus(&mut w);
    w.hold(task);
    let note = w
        .ledger
        .note_hold(w.project, "chief", task, &session, UNTIL)
        .expect("the chief is told");
    // A row of the ledger, as it was, but never on its way to the chief's window.
    assert_eq!(note.state, "read");
    assert_eq!(
        note.receipt,
        json!({ SHOWN: format!("waits with @{session}: out of quota until {UNTIL}") })
    );
    assert_eq!(
        w.next("chief"),
        None,
        "nothing is typed into the chief's window"
    );
    // The chief's inbox still lists it.
    assert!(w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the inbox")
        .iter()
        .any(|message| message.id == note.id));
    // Its task's line says it while it holds.
    assert_eq!(
        aside(&w, task).as_deref(),
        Some(format!("waits with @{session}: out of quota until {UNTIL}").as_str())
    );
    // The task goes on: the line says that, which is news and asks nothing either.
    w.daemon_resumes(task);
    assert_eq!(aside(&w, task), None, "a move ends what the line said");
    w.went_on(task);
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("goes on: its account has quota again.")
    );
    assert_eq!(w.next("chief"), None);
    assert_eq!(w.message(note.id).state, "read", "the note stays as it was");
}

#[test]
fn a_note_shown_to_anyone_but_a_chief_is_queued_as_any_note() {
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    let for_the_human = shown(&mut w, "human", task, "T-1 goes on.", "goes on");
    assert_eq!(for_the_human.state, "queued");
    assert_eq!(for_the_human.receipt, serde_json::Value::Null);
    assert_eq!(aside(&w, task), None);
}

#[test]
fn the_newest_aside_holds_until_the_task_moves_or_it_is_said_over() {
    let mut w = world();
    let (task, _) = with_zeus(&mut w);
    shown(
        &mut w,
        "chief",
        task,
        "T-1 is still working: 1 command.",
        "in the background: 1 command",
    );
    shown(
        &mut w,
        "chief",
        task,
        "T-1 is still working: 2 commands.",
        "in the background: 2 commands",
    );
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("in the background: 2 commands")
    );
    w.ledger.end_aside(w.project, task).expect("over");
    assert_eq!(
        aside(&w, task),
        None,
        "the engine said it is over, with no move"
    );
    shown(
        &mut w,
        "chief",
        task,
        "T-1 is still working: 1 command.",
        "in the background: 1 command",
    );
    assert_eq!(
        aside(&w, task).as_deref(),
        Some("in the background: 1 command")
    );
    w.pause(task);
    assert_eq!(aside(&w, task), None, "a pause is a move");
}
