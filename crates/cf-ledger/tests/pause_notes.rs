//! The note that tells a requester its tasks are paused: one note for the
//! tasks that stalled together, which a stall joins while it is queued, and
//! which a task leaves when it is resumed or called off, the note going when
//! none is left. A note its reader has been given is never touched.

// A test's own scaffolding expects, as the tests do.
#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, MessageView, NewChief, NewMember, NewNote, NewProject, NewQuestion,
    NewTask, Options,
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
            wake_up: None,
            specs: None,
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

/// What the note of the last complete answer of T-1's window says while T-1 is paused, and
/// once it has moved on (`words` is that answer; the turn was on the tell m-5).
fn paused_answer(words: &str) -> String {
    format!("T-1 is paused, and @zeus's turn on the words put to its window (m-5) is over. What it said last:\n\n{words}\n\nIf that is the result, take it: cf task finish T-1. To send it on: cf task resume T-1 \"…\".")
}

fn moved_answer(words: &str) -> String {
    format!("T-1, paused when @zeus's turn on the words put to its window (m-5) began, has moved on since (resumed, or back on the board): its result will be its next complete answer, not this. What that turn ended with:\n\n{words}")
}

#[test]
fn the_answer_of_a_paused_tasks_window_is_noted_once_for_each_tell() {
    let mut world = world();
    world.pause(1);
    let first = world
        .ledger
        .note_paused_answer(world.project, 1, "zeus", 5, "Parser done")
        .expect("the ledger")
        .expect("a note");
    assert_eq!(first.body, paused_answer("Parser done"));
    assert_eq!((first.kind.as_str(), first.task_number), ("note", Some(1)));
    assert_eq!(first.sender, None, "ConsensFlow's own");
    // Asked again for the same tell, whatever the words: none.
    for words in ["Parser done", "Parser done, and docs"] {
        let again = world
            .ledger
            .note_paused_answer(world.project, 1, "zeus", 5, words)
            .expect("the ledger");
        assert!(again.is_none(), "{words}");
    }
    // Withdrawn, it is no new reason to tell again.
    world
        .ledger
        .cancel_message(first.id, "withdrawn")
        .expect("cancelled");
    assert!(world
        .ledger
        .note_paused_answer(world.project, 1, "zeus", 5, "Parser done")
        .expect("the ledger")
        .is_none());
    // Another tell is another note.
    let next = world
        .ledger
        .note_paused_answer(world.project, 1, "zeus", 6, "Lexer done")
        .expect("the ledger")
        .expect("a note");
    assert!(next.body.contains("(m-6)"));
}

#[test]
fn a_resume_keeps_the_answer_of_a_paused_tasks_window_in_words_true_of_a_task_that_moved_on_for_a_reader_not_yet_given_it(
) {
    let mut world = world();
    let stall = world.stall(1);
    let queued = world
        .ledger
        .note_paused_answer(world.project, 1, "zeus", 5, "Parser done")
        .expect("the ledger")
        .expect("a note");
    let given = world
        .ledger
        .note_paused_answer(world.project, 1, "zeus", 4, "Earlier")
        .expect("the ledger")
        .expect("a note");
    world.ledger.begin_delivery(given.id).expect("begun");
    world
        .ledger
        .confirm_delivery(given.id, None)
        .expect("confirmed");

    world.resume(1);
    assert_eq!(
        world.message(stall.id).state,
        "cancelled",
        "the pause note goes"
    );
    let kept = world.message(queued.id);
    assert_eq!(
        (kept.state.as_str(), kept.body),
        ("queued", moved_answer("Parser done"))
    );
    let read = world.message(given.id);
    assert_eq!(
        (read.state.as_str(), read.body),
        (
            "delivered",
            paused_answer("Earlier").replace("(m-5)", "(m-4)")
        ),
        "what was read stays as it was"
    );
    // Paused again and resumed again, it is not withdrawn by the second resume either.
    world.pause(1);
    world.resume(1);
    assert_eq!(world.message(queued.id).state, "queued");
}

#[test]
fn finishing_a_paused_task_withdraws_what_its_requester_was_told_of_the_pause_and_the_answer_not_yet_given(
) {
    let mut world = world();
    let stall = world.stall(1);
    let queued = world
        .ledger
        .note_paused_answer(world.project, 1, "zeus", 5, "Parser done")
        .expect("the ledger")
        .expect("a note");
    let given = world
        .ledger
        .note_paused_answer(world.project, 1, "zeus", 4, "Earlier")
        .expect("the ledger")
        .expect("a note");
    world.ledger.begin_delivery(given.id).expect("begun");
    world
        .ledger
        .confirm_delivery(given.id, None)
        .expect("confirmed");

    let finished = world
        .ledger
        .record_finished(world.project, 1, "Parser done")
        .expect("finished");
    assert_eq!(finished.task.state, "done");
    for id in [stall.id, queued.id] {
        let message = world.message(id);
        assert_eq!(
            (message.state.as_str(), message.reason.as_deref()),
            ("cancelled", Some("T-1 finished")),
            "m-{id}"
        );
    }
    assert_eq!(
        world.message(given.id).state,
        "delivered",
        "what was read stays"
    );
}

impl World {
    /// The chief tells the window of T-1 `Stop`: the message that puts it (the task is paused
    /// for it), and the ids of T-1 and of its window, as `Ledger::tell_on_its_way` is asked.
    fn tell(&mut self) -> (MessageView, i64, i64) {
        let thread = self
            .ledger
            .task(self.project, 1)
            .expect("a read")
            .expect("T-1");
        let window = thread.task.assignee.clone().expect("its window");
        let told = self
            .ledger
            .ask(
                self.project,
                &NewQuestion {
                    from: Some("chief".into()),
                    to: window.clone(),
                    body: Some("Stop".into()),
                    task: Some(1),
                    urgent: true,
                    ..NewQuestion::default()
                },
            )
            .expect("the tell");
        let participant = self
            .ledger
            .project(self.project)
            .expect("a read")
            .expect("the project")
            .participants
            .into_iter()
            .find(|participant| participant.handle == window)
            .expect("its window's participant")
            .id;
        (told, thread.task.id, participant)
    }
}

#[test]
fn a_tell_is_on_its_way_while_it_is_queued_held_at_the_gate_or_being_pasted_and_not_after() {
    // Finish refuses a paused task while a tell to its window is on its way (T-442, Calliope's
    // P3 on round 2): the record holds the turn from before the tell until it is pasted.
    for gated in [false, true] {
        let mut world = world();
        world
            .ledger
            .set_gate(world.project, gated)
            .expect("the gate");
        let (told, task, window) = world.tell();
        let on_its_way = |world: &World| {
            world
                .ledger
                .tell_on_its_way(window, task)
                .expect("the ledger")
        };
        assert_eq!(told.state, if gated { "gated" } else { "queued" });
        assert!(on_its_way(&world), "{} as it is made", told.state);
        if gated {
            world
                .ledger
                .approve_message(told.id, "human")
                .expect("passed on");
            assert!(on_its_way(&world), "queued once the human passed it on");
        }
        world.ledger.begin_delivery(told.id).expect("on its way");
        assert!(on_its_way(&world), "being pasted");
        world
            .ledger
            .confirm_delivery(told.id, None)
            .expect("pasted");
        assert!(!on_its_way(&world), "pasted: the window has it");
    }
}

#[test]
fn a_tell_withdrawn_with_its_cancelled_task_is_on_its_way_no_more() {
    let mut world = world();
    world
        .ledger
        .set_gate(world.project, true)
        .expect("the gate");
    let (_, task, window) = world.tell();
    assert!(world
        .ledger
        .tell_on_its_way(window, task)
        .expect("the ledger"));
    world
        .ledger
        .cancel_task(world.project, 1, "chief")
        .expect("called off");
    assert!(!world
        .ledger
        .tell_on_its_way(window, task)
        .expect("the ledger"));
}
