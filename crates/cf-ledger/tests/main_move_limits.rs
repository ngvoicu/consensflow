//! Main's behaviour at a move where the human approves what goes to members, kept when the
//! words rework was turned off: the known limits of `architecture/staff-slots.md` 9.6 and the
//! stable limit list, each as the thing a person sees. A later fix turns these red on
//! purpose. (A follow-up held at the gate being cancelled and named to the chief is main's
//! own `receipt/transfers.rs`.)

#![allow(clippy::expect_used, clippy::unwrap_used, dead_code, unused_imports)]

#[path = "receipt/fixture.rs"]
mod fixture;

use cf_ledger::Read;
use fixture::{gated_world, world, World};

/// A standard task given to a window of zeus whose brief arrived: the window and the number.
fn at_work(w: &mut World) -> (String, i64) {
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let window = moved.task.assignee.clone().expect("a window");
    let brief = moved.message.expect("its brief").id;
    if w.message(brief).state == "gated" {
        w.ledger.approve_message(brief, "human").expect("passed on");
    }
    w.deliver(brief);
    (window, task)
}

fn next_brief(w: &mut World, task: i64) -> String {
    w.assign(task, "diana")
        .message
        .expect("the next window's brief")
        .body
}

/// A message still waiting for the human's approval leaves the approval list when its task
/// moves, and the next window's brief does not hold it.
#[test]
fn a_message_waiting_for_approval_is_withdrawn_and_not_given_to_the_next_window() {
    let mut w = gated_world();
    let (window, task) = at_work(&mut w);
    let note = w.note("chief", &window, task, "Mind the tests");
    assert_eq!(w.message(note.id).state, "gated");
    w.ledger
        .release_task(w.project, task, "ran out of quota")
        .expect("released");
    assert_eq!(w.message(note.id).state, "cancelled");
    let brief = next_brief(&mut w, task);
    assert!(!brief.contains("Mind the tests"), "{brief}");
}

/// A message being pasted when its task moves is given to the next window too, also where the
/// first window read it in its inbox.
#[test]
fn a_message_being_pasted_at_a_move_is_given_to_the_next_window_even_if_it_was_read() {
    let mut w = world();
    let (window, task) = at_work(&mut w);
    let note = w.note("chief", &window, task, "Mind the tests");
    w.begin(note.id);
    let reader = w.id(&window);
    w.ledger
        .receive_read(
            reader,
            &[note.id],
            Read::Inbox,
            &cf_proto::cf_word::CfWord::bare(),
        )
        .expect("read in the inbox");
    w.ledger
        .release_task(w.project, task, "by @human")
        .expect("released");
    let brief = next_brief(&mut w, task);
    assert!(
        brief.contains("Kept from before") && brief.contains("Mind the tests"),
        "{brief}"
    );
}

/// An urgent word of the chief's Pause that had not arrived is withdrawn at a move, and no note
/// names it.
#[test]
fn an_urgent_pause_word_not_arrived_at_a_move_is_withdrawn_and_nobody_is_told() {
    let mut w = world();
    let (window, task) = at_work(&mut w);
    w.pause(task);
    let urgent = w.tell(&window, task, "Stop and read this");
    w.ledger
        .release_task(w.project, task, "by @human")
        .expect("released");
    assert_eq!(w.message(urgent.id).state, "cancelled");
    let chief = w.id("chief");
    let named = w
        .ledger
        .inbox(chief, 100)
        .unwrap()
        .into_iter()
        .any(|m| m.body.contains(&format!("m-{}", urgent.id)));
    assert!(!named, "nobody is told its number");
    let brief = next_brief(&mut w, task);
    assert!(!brief.contains("Stop and read this"), "{brief}");
}

/// The chief's Pause takes back the chief's own messages the window had not received, a brief
/// and a Resume's words among them, and its answer names only the tells.
#[test]
fn the_chiefs_pause_takes_back_what_it_wrote_and_names_only_the_tells() {
    let mut w = world();
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let window = moved.task.assignee.clone().expect("a window");
    let brief = moved.message.expect("its brief").id;
    let tell = w.note("chief", &window, task, "Mind the tests");
    let (_, named) = w
        .ledger
        .pause_task_naming(w.project, task, Some("chief"), None)
        .expect("paused");
    assert_eq!(
        w.message(brief).state,
        "cancelled",
        "the brief is taken back"
    );
    assert_eq!(w.message(tell.id).state, "cancelled");
    assert_eq!(
        named.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![tell.id],
        "only the tells are named"
    );
    // A Resume's words, queued and not received, are taken back too.
    let resumed = w
        .resume(task, "Go on from there")
        .message
        .expect("its words");
    let (_, named) = w
        .ledger
        .pause_task_naming(w.project, task, Some("chief"), None)
        .expect("paused again");
    assert_eq!(w.message(resumed.id).state, "cancelled");
    assert!(named.iter().all(|m| m.id != resumed.id));
}

/// The human's Pause takes nothing back.
#[test]
fn the_humans_pause_takes_nothing_back() {
    let mut w = world();
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let window = moved.task.assignee.clone().expect("a window");
    let brief = moved.message.expect("its brief").id;
    let tell = w.note("chief", &window, task, "Mind the tests");
    w.ledger
        .pause_task(w.project, task, Some("human"), None)
        .expect("paused");
    assert_eq!(w.message(brief).state, "queued");
    assert_eq!(w.message(tell.id).state, "queued");
}

/// Where the row's words reach too far: the chief's Pause takes back neither a message whose
/// delivery had begun nor an answer the chief wrote, only the chief's queued or gated tasks
/// and notes.
#[test]
fn the_chiefs_pause_leaves_a_message_being_pasted_and_an_answer() {
    let mut w = world();
    let (window, task) = at_work(&mut w);
    let question = w.ask(&window, task, "Which format?");
    let answer = w.answer(question.id, "JSON");
    let note = w.note("chief", &window, task, "Mind the tests");
    w.begin(note.id);
    w.ledger
        .pause_task(w.project, task, Some("chief"), None)
        .expect("paused");
    assert_eq!(w.message(note.id).state, "delivering");
    assert_eq!(w.message(answer.id).state, "queued");
}
