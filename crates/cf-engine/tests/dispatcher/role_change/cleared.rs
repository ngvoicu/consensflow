//! T-829 (a P3 of Calliope's T-824): a window its human switched to a session no hook
//! of its harness greeted (Muse Code's `/clear`, which runs no SessionStart hook) began
//! that conversation with no role text. On a harness that takes its role at every
//! launch, the engine writes the role for it, and it goes in first; once delivered, the
//! conversation is recorded as told it. A session a hook greeted, and one on a harness
//! that keeps its first instructions, are given nothing.

use cf_engine::testing::Context;
use cf_ledger::MessageView;

use super::recorded;

/// The words that tell the note that gives a cleared session its role.
const BEGAN_WITHOUT: &str = "began without your ConsensFlow role instructions";

/// The notes ConsensFlow wrote to `participant` that give a session its role, oldest first.
fn greetings(context: &Context, participant: i64) -> Vec<MessageView> {
    let mut notes: Vec<MessageView> = context
        .inbox(participant)
        .into_iter()
        .filter(|message| {
            message.sender.is_none()
                && message.kind == "note"
                && message.body.contains(BEGAN_WITHOUT)
        })
        .collect();
    notes.reverse();
    notes
}

#[test]
fn a_session_no_hook_greeted_is_given_the_role_first_and_one_a_hook_greeted_nothing() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    // A /resume of an older one: its hook greeted it.
    context.adapter.switch_to("chief", "native-older");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(greetings(&context, chief).is_empty(), "a greeted session");
    // A /clear that no hook greeted: the role goes in first, and is recorded told.
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context.adapter.switch_to("chief", "native-cleared");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let notes = greetings(&context, chief);
    assert_eq!(notes.len(), 1, "{notes:?}");
    let note = context.message(notes[0].id);
    assert_eq!(note.state, "delivered");
    let role = note
        .body
        .split_once("\n\n")
        .expect("the role after the opening")
        .1;
    assert_eq!(recorded(&context, chief).as_deref(), Some(role));
    // Looked at again, the session is told no more.
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(greetings(&context, chief).len(), 1);
    // A second /clear in the same window is given the role too.
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared-2".into())
    });
    context.adapter.switch_to("chief", "native-cleared-2");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(greetings(&context, chief).len(), 2);
    // Back on the first, which was told the role as it is: nothing more.
    context.adapter.switch_to("chief", "native-cleared");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(greetings(&context, chief).len(), 2);
}

#[test]
fn a_harness_that_keeps_its_first_instructions_is_given_nothing_at_a_switch() {
    let context = Context::new();
    context.adapter.keeps_first_instructions.set(true);
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context.adapter.switch_to("chief", "native-cleared");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(greetings(&context, chief).is_empty());
}
