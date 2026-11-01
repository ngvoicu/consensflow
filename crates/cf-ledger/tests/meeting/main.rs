//! A meeting with the chief in the ledger (3.0.7 chief-meeting, CMT-11 to CMT-16):
//! while the human's meeting is on, what is queued for the chief is held, a member's
//! question first in the order it will go; what a launch owes the chief's window and
//! the note a Switch chief asks with are not; a member's queue goes on. The Stop, the
//! project's Close and the daemon's start after a quit end it, each logged with why,
//! and what it held goes as any queued message does. The human's approval comes
//! first: a gated message is the human's to pass on before a meeting holds it. No
//! wake-up is queued during a meeting, and the calls count again from its end.

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::expect_used, clippy::unwrap_used)]

// The wake-up calls' world: a project at a clock the test moves, and what a test
// does to it. Not every one of its helpers is a meeting's.
#[allow(dead_code)]
#[path = "../wake_ups/world.rs"]
mod world;

mod approval;
mod board;
mod ending;
mod holding;
mod wake_ups;

use cf_ledger::{MessageView, NewNote};
use world::World;

/// A note to the chief from `from` (a handle, or ConsensFlow's own with none),
/// about task `task` or none.
fn note_to_chief(w: &mut World, from: Option<&str>, task: Option<i64>, body: &str) -> MessageView {
    w.ledger
        .note(
            w.project.id,
            &NewNote {
                from: from.map(str::to_owned),
                to: "chief".to_owned(),
                task,
                body: body.to_owned(),
            },
        )
        .expect("a note")
}

/// The ids of what the meeting holds, in the order they will go.
fn held(w: &World) -> Vec<i64> {
    w.ledger
        .held_by_meeting(w.project.id)
        .expect("a read")
        .iter()
        .map(|message| message.id)
        .collect()
}

/// The id at the head of the chief's queue, if any goes now.
fn head(w: &World) -> Option<i64> {
    w.ledger
        .next_delivery(w.id("chief"))
        .expect("a read")
        .map(|next| next.id)
}

/// The ids the chief's queue hands over, in order, each delivered before the next.
fn handed(w: &mut World) -> Vec<i64> {
    let mut order = Vec::new();
    while let Some(next) = w.ledger.next_delivery(w.id("chief")).expect("a read") {
        w.deliver(&next);
        order.push(next.id);
    }
    order
}

/// The data of each `project.meeting` event, oldest first.
fn meetings(w: &World) -> Vec<serde_json::Value> {
    w.ledger
        .events(w.project.id, 0, 5000)
        .expect("events")
        .into_iter()
        .filter(|event| event.kind == "project.meeting")
        .map(|event| event.data)
        .collect()
}

/// zeus's task given, at work, and done: its result queued for the chief.
fn a_result(w: &mut World) -> i64 {
    let given = w.give(&[]);
    w.start(&given);
    let number = given.task.number;
    w.finish(number);
    w.ledger
        .task(w.project.id, number)
        .expect("a read")
        .expect("the task")
        .messages
        .iter()
        .rfind(|message| message.kind == "result")
        .expect("its result")
        .id
}
