//! A chain of ConsensFlow's notes is one message (RS-25; T-1410 counted 43% of the chief's
//! typed notes in chains, one turn a note, the largest 44 in 170 seconds): the notes that
//! wait for the chief together when its window can take the first go in one paste, a line
//! each. Each stays a row of its own, proved by the paste.

use cf_engine::testing::Context;
use cf_ledger::NewNote;
use cf_proto::ledger::MessageView;

use crate::fixtures::{given, Tiers};

/// ConsensFlow's own note to the chief.
fn told(context: &Context, project: i64, task: Option<i64>, body: &str) -> MessageView {
    context
        .ledger
        .borrow_mut()
        .note(
            project,
            &NewNote {
                from: None,
                to: "chief".to_owned(),
                task,
                body: body.to_owned(),
            },
        )
        .unwrap()
}

#[test]
fn the_notes_that_wait_for_a_chief_at_work_go_into_its_window_as_one_message() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    // The chief is in a turn as one event writes three notes for it.
    context.adapter.busy("chief");
    let before = given(&context, "chief").len();
    let notes = [
        told(&context, tiers.project.id, Some(1), "T-1 needs a look."),
        told(&context, tiers.project.id, Some(2), "T-2 needs a look."),
        told(
            &context,
            tiers.project.id,
            None,
            "The staff's rules changed.",
        ),
    ];
    context.pass().unwrap();
    context.adapter.answer("chief", "Done.");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let pasted: Vec<String> = given(&context, "chief").split_off(before);
    assert_eq!(pasted.len(), 1, "one message: {pasted:?}");
    let first = notes[0].id;
    assert!(
        pasted[0].contains(&format!("[ConsensFlow m-{first} · note from ConsensFlow]\n3 notes at once, a line each:\nm-{first} · T-1: T-1 needs a look.\n")),
        "{}",
        pasted[0]
    );
    for note in &notes {
        assert_eq!(context.message(note.id).state, "delivered", "m-{}", note.id);
    }
}
