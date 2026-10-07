//! The note that tells a requester a task is paused goes when the task is
//! resumed, on each way a resume goes (`tasks/pausing.rs`): into its window,
//! back on the board for its tier when its session ended, and as the task
//! nobody holds yet. The window case, and a note of several, are held in
//! `tests/pause_notes.rs`.

use crate::fixture::{gated_world, world};

#[test]
fn a_resume_after_its_session_ended_withdraws_the_pause_note_and_keeps_its_own() {
    let mut w = gated_world();
    let task = w.open("Parser").task.number;
    let moved = w.assign(task, "zeus");
    let session = moved.task.assignee.clone().expect("a session");
    let brief = moved.message.expect("its brief").id;
    w.ledger.approve_message(brief, "human").expect("passed on");
    w.deliver(brief);
    // What the human never passed on waits at the gate, and goes with the session.
    let gated = w.note("chief", &session, task, "Mind the tests");
    assert_eq!(w.message(gated.id).state, "gated");

    // Its window went away: the daemon paused it and told the chief, who has
    // not been given the note.
    let because = format!("@{session}'s window is gone");
    w.ledger
        .pause_task(w.project, task, None, Some(&because))
        .expect("paused");
    let paused = w
        .ledger
        .note_pause(w.project, "chief", task, &because)
        .expect("the chief is told");
    w.ledger
        .end_session(w.project, &session, "human")
        .expect("a session that holds only a paused task ends");

    // The daemon resumes it in its own name, and the task goes back on the
    // board. The note the resume writes about what it withdrew goes to the
    // requester, as the pause note did: it says what the resume did, and stays.
    w.daemon_resumes(task);
    assert_eq!(w.state(task), "open");
    let withdrawn = w.message(paused.id);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some("T-1 resumed"))
    );
    let told = w
        .ledger
        .inbox(w.id("chief"), 100)
        .expect("the chief's inbox")
        .into_iter()
        .find(|message| message.body.starts_with("Withdrawn with it"))
        .expect("the requester is told what the resume withdrew");
    assert_eq!(
        (told.state.as_str(), told.body.as_str()),
        (
            "queued",
            format!(
                "Withdrawn with it, still waiting for the human: m-{}.",
                gated.id
            )
            .as_str()
        )
    );
}

#[test]
fn a_paused_task_nobody_holds_yet_goes_back_on_the_board_without_its_pause_note() {
    let mut w = world();
    let task = w.open("Parser").task.number;
    w.pause(task);
    let paused = w
        .ledger
        .note_pause(
            w.project,
            "chief",
            task,
            "it was held before any window had it",
        )
        .expect("the chief is told");

    let moved = w.resume(task, "Go on");
    assert_eq!(moved.message, None, "no window has it to be told");
    assert_eq!(w.state(task), "open");
    let withdrawn = w.message(paused.id);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some("T-1 resumed"))
    );
}
