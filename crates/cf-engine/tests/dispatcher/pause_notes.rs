//! The note that tells a requester its tasks are paused because their
//! windows went away: one for the tasks of a pass or of a Close, and
//! withdrawn, whole or in part, when they are resumed before its reader has
//! it. Node tells each task in a note of its own and never withdraws one, so
//! none of these is held to a Node trace.

use cf_engine::testing::{Context, Restarted};
use cf_ledger::MessageView;

use crate::fixtures::{after, exhausted, given, Tiers};

/// A lone task's note, as every stall always told it.
const ONE: &str = "T-1 is paused: @zeus's window closed. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation.";

/// What the chief was told of three tasks a restart paused.
const THREE: &str = "\
T-1 is paused: @zeus-amber-pine's window is gone. Resume it with: cf task resume T-1 \"…\"
T-2 is paused: @diana-brisk-birch's window is gone. Resume it with: cf task resume T-2 \"…\"
T-3 is paused: @athena-calm-brook's window is gone. Resume it with: cf task resume T-3 \"…\"
Each window comes back on its own conversation.";

/// The chief's pause notes, oldest first, as the ledger has them now.
fn pause_notes(context: &Context, project: i64) -> Vec<MessageView> {
    let mut notes: Vec<MessageView> = context
        .inbox(context.id(project, "chief"))
        .into_iter()
        .filter(|message| message.kind == "note" && message.body.contains(" is paused: "))
        .collect();
    notes.reverse();
    notes
}

/// The app quits and starts again while the chief is in the middle of a turn:
/// every window of the staff went with it, the first pass pauses their tasks,
/// and the chief is not given the note until its turn ends.
fn restart_with_the_chief_at_work(context: &Context) -> Restarted<'_> {
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = context.make();
    restarted.resume_after_restart().unwrap();
    context.adapter.busy("chief");
    restarted.pass().unwrap();
    restarted
}

/// Three standard workers with a task each, working: T-1, T-2 and T-3.
fn working(context: &Context) -> Tiers<'_> {
    let tiers = Tiers::new(context, &["zeus", "diana", "athena"]);
    for body in ["Write the parser", "Write the lexer", "Write the printer"] {
        tiers.open_body(body);
    }
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        [1, 2, 3].map(|number| tiers.task(number).task.state),
        ["working", "working", "working"]
    );
    tiers
}

#[test]
fn tells_a_chief_once_of_all_the_tasks_a_restart_pauses() {
    let context = Context::new();
    let tiers = working(&context);

    // The app quits and starts again: every window of the staff went with it.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = context.make();
    restarted.resume_after_restart().unwrap();
    restarted.pass().unwrap();
    assert_eq!(
        [1, 2, 3].map(|number| tiers.task(number).task.state),
        ["paused", "paused", "paused"]
    );
    let notes = pause_notes(&context, tiers.project.id);
    assert_eq!(notes.len(), 1, "one note for the three tasks");
    assert_eq!(notes[0].body, THREE);
    assert_eq!(notes[0].task_number, None, "it is about none of them alone");

    // One message in the chief's window, and so one turn.
    restarted.pass().unwrap();
    restarted.pass().unwrap();
    let pasted: Vec<String> = given(&context, "chief")
        .into_iter()
        .filter(|text| text.contains(" is paused: "))
        .collect();
    assert_eq!(pasted.len(), 1);
    assert!(pasted[0].ends_with(THREE), "{:?}", pasted[0]);
    assert_eq!(context.message(notes[0].id).state, "delivered");
}

#[test]
fn tells_a_lone_stall_in_the_note_it_always_had() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.exit("zeus");
    let notes = pause_notes(&context, project.id);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body, ONE);
    assert_eq!(notes[0].task_number, Some(1), "it is the task's own");
    assert_eq!(notes[0].sender, None, "from ConsensFlow");
}

#[test]
fn tells_a_lone_stall_in_a_pass_in_the_note_it_always_had() {
    let context = Context::new();
    let tiers = working(&context);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let restarted = context.make();
    restarted.resume_after_restart().unwrap();
    // The chief paused two of the tasks before the restart took their windows: only T-1 stalls.
    context
        .ledger
        .borrow_mut()
        .pause_task(tiers.project.id, 2, Some("chief"), None)
        .unwrap();
    context
        .ledger
        .borrow_mut()
        .pause_task(tiers.project.id, 3, Some("chief"), None)
        .unwrap();
    restarted.pass().unwrap();
    let notes = pause_notes(&context, tiers.project.id);
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0].body,
        "T-1 is paused: @zeus-amber-pine's window is gone. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
    );
    assert_eq!(notes[0].task_number, Some(1));
}

#[test]
fn tells_stalls_that_come_apart_in_a_note_each() {
    let context = Context::new();
    let project = context.with_staff(&["zeus", "diana"]);
    context.give(project.id, "zeus", "Parser");
    context.give(project.id, "diana", "Lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    context.exit("zeus");
    context.exit("diana");
    let notes = pause_notes(&context, project.id);
    assert_eq!(
        notes
            .iter()
            .map(|note| note.task_number)
            .collect::<Vec<_>>(),
        [Some(1), Some(2)],
        "a window that closes on its own is a pass of its own"
    );
}

#[test]
fn tells_a_stall_after_a_close_in_a_note_of_its_own_though_the_close_left_a_note_queued() {
    let context = Context::new();
    let project = context.with_staff(&["zeus", "diana"]);
    context.give(project.id, "zeus", "Parser");
    context.give(project.id, "diana", "Lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    assert_eq!(
        pause_notes(&context, project.id).len(),
        1,
        "one note, queued"
    );

    // The project comes back with its chief in the middle of a turn; T-1 goes
    // on in its window, which then closes by itself.
    context.resume_project(project.id).unwrap();
    context.adapter.busy("chief");
    context.resume_task(project.id, 1, "Carry on");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.exit("zeus");
    let notes = pause_notes(&context, project.id);
    assert_eq!(notes.len(), 2, "the close's note is not the one it joins");
    assert!(
        notes[0].body.contains("T-2") && !notes[0].body.contains("T-1"),
        "{:?}",
        notes[0].body
    );
    assert_eq!(
        (notes[1].state.as_str(), notes[1].task_number),
        ("queued", Some(1))
    );
}

#[test]
fn tells_a_chief_once_of_all_the_tasks_a_closed_project_pauses() {
    let context = Context::new();
    let project = context.with_staff(&["zeus", "diana"]);
    context.give(project.id, "zeus", "Parser");
    context.give(project.id, "diana", "Lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    assert_eq!(
        [1, 2].map(|number| context.task(project.id, number).task.state),
        ["paused", "paused"]
    );
    let notes = pause_notes(&context, project.id);
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0].body,
        "\
T-1 is paused: @zeus's window closed. Resume it with: cf task resume T-1 \"…\"
T-2 is paused: @diana's window closed. Resume it with: cf task resume T-2 \"…\"
Each window comes back on its own conversation."
    );
}

#[test]
fn withdraws_a_note_its_reader_has_not_been_given_when_its_task_is_resumed() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.exit("zeus");
    let note = pause_notes(&context, project.id).remove(0);
    assert_eq!(note.state, "queued", "the chief has not been given it");

    context.resume_task(project.id, 1, "Carry on");
    let withdrawn = context.message(note.id);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some("T-1 resumed"))
    );
    context.pass().unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains(" is paused: ")),
        "never pasted"
    );
    assert_eq!(
        context.task(project.id, 1).task.state,
        "working",
        "it went on"
    );
}

#[test]
fn leaves_a_note_alone_once_its_reader_has_it_or_is_being_given_it() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.exit("zeus");
    let note = pause_notes(&context, project.id).remove(0);

    // Being pasted into the window.
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context.resume_task(project.id, 1, "Carry on");
    assert_eq!(context.message(note.id).state, "delivering");
    context.pass().unwrap();
    context.pass().unwrap();
    let kept = context.message(note.id);
    assert_eq!((kept.state.as_str(), kept.reason), ("delivered", None));

    // Given, and the task paused again and resumed again: the first note is still as it was.
    context.exit("zeus");
    let again = pause_notes(&context, project.id);
    assert_eq!(again.len(), 2, "the second pause is told afresh");
    context.resume_task(project.id, 1, "And again");
    assert_eq!(context.message(note.id).body, ONE);
    assert_eq!(context.message(note.id).state, "delivered");
    assert_eq!(context.message(again[1].id).state, "cancelled");
}

#[test]
fn takes_a_resumed_task_out_of_a_note_that_names_several_and_withdraws_it_when_none_is_left() {
    let context = Context::new();
    let tiers = working(&context);
    let restarted = restart_with_the_chief_at_work(&context);
    let project = tiers.project.id;
    let note = pause_notes(&context, project).remove(0);
    assert_eq!(note.body, THREE);
    assert_eq!(note.state, "queued", "the chief is in the middle of a turn");

    context.resume_task(project, 2, "Carry on");
    let narrowed = context.message(note.id);
    assert_eq!(narrowed.state, "queued");
    assert_eq!(
        narrowed.body,
        "\
T-1 is paused: @zeus-amber-pine's window is gone. Resume it with: cf task resume T-1 \"…\"
T-3 is paused: @athena-calm-brook's window is gone. Resume it with: cf task resume T-3 \"…\"
Each window comes back on its own conversation."
    );
    assert_eq!(narrowed.task_number, None);

    // One task left: it is told as a task alone is.
    context.resume_task(project, 1, "Carry on");
    let one = context.message(note.id);
    assert_eq!(
        one.body,
        "T-3 is paused: @athena-calm-brook's window is gone. Resume it with: cf task resume T-3 \"…\"; its window comes back on its own conversation."
    );
    assert_eq!((one.state.as_str(), one.task_number), ("queued", Some(3)));

    context.resume_task(project, 3, "Carry on");
    let none = context.message(note.id);
    assert_eq!(
        (none.state.as_str(), none.reason.as_deref()),
        ("cancelled", Some("T-3 resumed"))
    );
    context.adapter.answer("chief", "Resumed all three.");
    restarted.pass().unwrap();
    restarted.pass().unwrap();
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains(" is paused: ")),
        "the chief is never told: it resumed every one"
    );
}

#[test]
fn gives_what_is_left_of_a_note_whose_other_tasks_were_resumed() {
    let context = Context::new();
    let tiers = working(&context);
    let restarted = restart_with_the_chief_at_work(&context);
    let project = tiers.project.id;
    context.resume_task(project, 1, "Carry on");
    context.resume_task(project, 3, "Carry on");
    context.adapter.answer("chief", "Resumed two.");
    restarted.pass().unwrap();
    restarted.pass().unwrap();
    let pasted: Vec<String> = given(&context, "chief")
        .into_iter()
        .filter(|text| text.contains(" is paused: "))
        .collect();
    assert_eq!(pasted.len(), 1);
    assert!(
        pasted[0].ends_with("T-2 is paused: @diana-brisk-birch's window is gone. Resume it with: cf task resume T-2 \"…\"; its window comes back on its own conversation."),
        "{:?}",
        pasted[0]
    );
}

#[test]
fn takes_a_cancelled_task_out_of_a_note_that_names_several() {
    let context = Context::new();
    let tiers = working(&context);
    let _restarted = restart_with_the_chief_at_work(&context);
    let project = tiers.project.id;
    let note = pause_notes(&context, project).remove(0);
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "chief")
        .unwrap();
    let narrowed = context.message(note.id);
    assert_eq!(narrowed.state, "queued");
    assert!(
        !narrowed.body.contains("T-1"),
        "a cancelled task is no longer paused: {:?}",
        narrowed.body
    );
    assert!(narrowed.body.contains("T-2") && narrowed.body.contains("T-3"));
}

#[test]
fn withdraws_the_note_that_says_a_task_is_held_when_the_daemon_resumes_it() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    // The chief is in the middle of a turn when zeus's quota runs out.
    context.adapter.busy("chief");
    let resets_at = after(&context, 20 * 60_000);
    context
        .adapter
        .quota("zeus", Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    let notes: Vec<MessageView> = context
        .inbox(tiers.id("chief"))
        .into_iter()
        .filter(|message| message.body.contains(" waits with "))
        .collect();
    assert_eq!(
        notes.len(),
        1,
        "the chief is told the task waits for its member"
    );
    let note = &notes[0];
    assert_eq!(
        note.state, "queued",
        "the chief's window has not been given it"
    );

    // The reset passes before the chief's turn ends: the daemon resumes the task.
    context.adapter.answer("zeus", "Stopped at the lexer.");
    context.advance(21 * 60_000);
    context.adapter.quota("zeus", None);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let withdrawn = context.message(note.id);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        ("cancelled", Some("T-1 resumed"))
    );
    context.adapter.answer("chief", "Done.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert!(
        given(&context, "chief")
            .iter()
            .all(|text| !text.contains(" waits with ")),
        "never pasted"
    );
}
