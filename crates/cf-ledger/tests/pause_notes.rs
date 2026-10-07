//! The note that tells a requester its tasks are paused: one note for the
//! tasks that stalled together, which a stall joins while it is queued, and
//! which a task leaves when it is resumed or called off, the note going when
//! none is left. A note its reader has been given is never touched.

// A test's own scaffolding expects, as the tests do.
#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, MessageView, NewChief, NewMember, NewNote, NewProject, NewTask, Options,
};

/// A project whose chief has given T-1, T-2 and T-3 to three workers, each
/// paused as the daemon pauses a task whose window went away.
struct World {
    ledger: Ledger,
    project: i64,
    _dir: tempfile::TempDir,
}

fn world() -> World {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger =
        open_ledger(&dir.path().join("consensflow.db"), Options::default()).expect("a ledger");
    let staff = ["zeus", "diana", "athena"]
        .map(|agent| NewMember {
            agent: agent.into(),
            harness: "claude-code".into(),
            designer: false,
            roles: vec!["worker".into()],
            tier: "standard".into(),
        })
        .to_vec();
    let project = ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                agent: None,
            },
            staff,
            gate: false,
        })
        .expect("a project")
        .id;
    for (to, body) in [
        ("zeus", "Parser"),
        ("diana", "Lexer"),
        ("athena", "Printer"),
    ] {
        ledger
            .create_task(
                project,
                &NewTask {
                    from: "chief".into(),
                    to: Some(to.into()),
                    body: body.into(),
                    ..NewTask::default()
                },
            )
            .expect("a task");
    }
    World {
        ledger,
        project,
        _dir: dir,
    }
}

impl World {
    /// The daemon pauses T-`number` because its window is gone: why.
    fn pause(&mut self, number: i64) -> String {
        let because = format!("@worker{number}'s window is gone");
        self.ledger
            .pause_task(self.project, number, None, Some(&because))
            .expect("the task paused");
        because
    }

    /// The daemon pauses T-`number`, and tells the chief of it alone.
    fn stall(&mut self, number: i64) -> MessageView {
        let because = self.pause(number);
        self.ledger
            .note_pause(self.project, "chief", number, &because)
            .expect("the note")
    }

    /// The daemon pauses T-`number`, and the note `note` takes it in.
    fn join(&mut self, note: i64, number: i64) -> Option<MessageView> {
        let because = self.pause(number);
        self.ledger
            .join_pause_note(note, number, &because)
            .expect("asked")
    }

    fn message(&self, id: i64) -> MessageView {
        self.ledger
            .message(id)
            .expect("the ledger read")
            .expect("the message")
    }

    /// A note from `from` (ConsensFlow when none) to `to`, about T-`task` or none.
    fn note(&mut self, from: Option<&str>, to: &str, task: Option<i64>, body: &str) -> MessageView {
        self.ledger
            .note(
                self.project,
                &NewNote {
                    from: from.map(str::to_owned),
                    to: to.into(),
                    body: body.into(),
                    task,
                },
            )
            .expect("a note")
    }

    fn resume(&mut self, number: i64) {
        self.ledger
            .resume_task(self.project, number, Some("chief"), "Go on")
            .expect("the task resumed");
    }
}

const ONE: &str = "T-1 is paused: @worker1's window is gone. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation.";

#[test]
fn a_task_alone_is_told_as_the_stall_always_told_it() {
    let mut world = world();
    let note = world.stall(1);
    assert_eq!(
        (note.body.as_str(), note.task_number, note.sender.as_deref()),
        (ONE, Some(1), None)
    );
    assert_eq!(
        (
            note.kind.as_str(),
            note.recipient.as_str(),
            note.state.as_str()
        ),
        ("note", "chief", "queued")
    );
}

#[test]
fn a_stall_of_the_same_pass_joins_the_note_and_it_names_them_all_in_order() {
    let mut world = world();
    let note = world.stall(1);
    world.join(note.id, 3).expect("the note takes it");
    let note = world.join(note.id, 2).expect("the note takes it");
    assert_eq!(
        note.body,
        "\
T-1 is paused: @worker1's window is gone. Resume it with: cf task resume T-1 \"…\"
T-2 is paused: @worker2's window is gone. Resume it with: cf task resume T-2 \"…\"
T-3 is paused: @worker3's window is gone. Resume it with: cf task resume T-3 \"…\"
Each window comes back on its own conversation."
    );
    assert_eq!(note.task_number, None, "it is about none of them alone");
    // A task the note names already is named once.
    let again = world
        .ledger
        .join_pause_note(note.id, 2, "something else")
        .expect("joined")
        .expect("the note takes it");
    assert_eq!(again.body, note.body);
}

#[test]
fn a_note_that_was_given_or_withdrawn_or_is_no_pause_note_takes_no_task() {
    let mut world = world();
    let pasting = world.stall(1);
    world.ledger.begin_delivery(pasting.id).expect("begun");
    let taken = |world: &mut World, id: i64| {
        world
            .ledger
            .join_pause_note(id, 2, "@worker2's window is gone")
            .expect("asked")
    };
    assert_eq!(taken(&mut world, pasting.id), None, "being pasted");
    world
        .ledger
        .confirm_delivery(pasting.id, None)
        .expect("confirmed");
    assert_eq!(taken(&mut world, pasting.id), None, "given");
    assert_eq!(world.message(pasting.id).body, ONE, "and as it was");

    let withdrawn = world.note(None, "chief", Some(3), "T-3 is paused: only so");
    world
        .ledger
        .cancel_message(withdrawn.id, "no longer so")
        .expect("cancelled");
    assert_eq!(taken(&mut world, withdrawn.id), None, "withdrawn");

    let other = world.note(
        None,
        "chief",
        Some(3),
        "T-3 waits with @athena: out of quota.",
    );
    assert_eq!(taken(&mut world, other.id), None, "no pause note");
    let chiefs = world.note(Some("chief"), "human", Some(1), ONE);
    assert_eq!(
        taken(&mut world, chiefs.id),
        None,
        "its words, but not ConsensFlow's"
    );
}

#[test]
fn a_resume_withdraws_what_its_requester_was_told_of_the_pause_and_has_not_been_given() {
    let mut world = world();
    let stall = world.stall(1);
    let hold = world.note(
        None,
        "chief",
        Some(1),
        "T-1 waits with @zeus: out of quota until noon.",
    );
    let refusal = world.note(
        None,
        "chief",
        Some(1),
        "T-1 stays paused: it could not go on.",
    );
    // What is not ConsensFlow's own word to the requester about the task stays.
    let chiefs = world.note(
        Some("chief"),
        "chief",
        Some(1),
        "A reminder from the chief.",
    );
    let humans = world.note(None, "human", Some(1), "m-9, a task, did not reach @zeus.");
    let other_task = world.note(
        None,
        "chief",
        Some(2),
        "T-2 waits with @diana: out of quota.",
    );
    // What its reader has been given stays too.
    let given = world.note(None, "chief", Some(1), "T-1 is paused: given long ago.");
    world.ledger.begin_delivery(given.id).expect("begun");
    world
        .ledger
        .confirm_delivery(given.id, None)
        .expect("confirmed");

    world.resume(1);
    let state = |world: &World, id: i64| {
        let message = world.message(id);
        (message.state, message.reason)
    };
    let withdrawn = ("cancelled".to_owned(), Some("T-1 resumed".to_owned()));
    assert_eq!(state(&world, stall.id), withdrawn);
    assert_eq!(state(&world, hold.id), withdrawn);
    assert_eq!(state(&world, refusal.id), withdrawn);
    for (kept, why) in [
        (chiefs.id, "a note of the chief's"),
        (humans.id, "a note for another reader"),
        (other_task.id, "a note about another task"),
    ] {
        assert_eq!(world.message(kept).state, "queued", "{why}");
    }
    assert_eq!(world.message(given.id).state, "delivered", "a note given");
}

#[test]
fn a_resume_takes_its_task_out_of_a_note_of_several_and_a_last_one_leaves_it_a_note_of_the_task() {
    let mut world = world();
    let note = world.stall(1);
    world.join(note.id, 2).expect("joined");
    world.join(note.id, 3).expect("joined");
    world.resume(2);
    let narrowed = world.message(note.id);
    assert_eq!(narrowed.state, "queued");
    assert!(
        narrowed.body.starts_with("T-1 is paused:")
            && narrowed.body.contains("\nT-3 is paused:")
            && !narrowed.body.contains("T-2"),
        "{:?}",
        narrowed.body
    );
    assert_eq!(narrowed.task_number, None);

    world.resume(1);
    let one = world.message(note.id);
    assert_eq!(
        one.body,
        "T-3 is paused: @worker3's window is gone. Resume it with: cf task resume T-3 \"…\"; its window comes back on its own conversation."
    );
    assert_eq!(
        (one.state.as_str(), one.task_number),
        ("queued", Some(3)),
        "told as a task alone is"
    );

    world.resume(3);
    let gone = world.message(note.id);
    assert_eq!(
        (gone.state.as_str(), gone.reason.as_deref()),
        ("cancelled", Some("T-3 resumed"))
    );
}

#[test]
fn a_note_of_several_that_was_given_names_its_tasks_as_it_did() {
    let mut world = world();
    let note = world.stall(1);
    world.join(note.id, 2).expect("joined");
    world.ledger.begin_delivery(note.id).expect("begun");
    world.resume(1);
    assert_eq!(world.message(note.id).state, "delivering");
    world
        .ledger
        .confirm_delivery(note.id, None)
        .expect("confirmed");
    world.resume(2);
    let given = world.message(note.id);
    assert_eq!(given.state, "delivered");
    assert!(given.body.contains("T-1 is paused") && given.body.contains("T-2 is paused"));
}

#[test]
fn a_task_called_off_leaves_the_note_of_several_and_a_refused_resume_withdraws_nothing() {
    let mut world = world();
    let note = world.stall(1);
    world.join(note.id, 2).expect("joined");
    world
        .ledger
        .cancel_task(world.project, 1, "chief")
        .expect("cancelled");
    let narrowed = world.message(note.id);
    assert_eq!(
        (narrowed.state.as_str(), narrowed.task_number),
        ("queued", Some(2))
    );
    assert!(!narrowed.body.contains("T-1"), "{:?}", narrowed.body);

    // T-2 was given by name to a member who then left: its resume is refused,
    // and the note stays as it was.
    world
        .ledger
        .remove_member(world.project, "diana")
        .expect("the member left");
    let refused = world
        .ledger
        .resume_task(world.project, 2, Some("chief"), "Go on")
        .expect_err("the window that had it has ended");
    assert_eq!(refused.code(), Some("session-ended"));
    let kept = world.message(note.id);
    assert_eq!(
        (kept.state.as_str(), kept.task_number, kept.reason),
        ("queued", Some(2), None)
    );
}
