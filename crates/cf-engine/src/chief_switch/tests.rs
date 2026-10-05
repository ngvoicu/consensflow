//! What the chief switch keeps that no test of `core-dispatcher.test.mjs`
//! holds, so there is no Node trace to hold it to.

use crate::testing::{Context, Made};
use crate::{SwitchTo, SwitchWhen};

#[test]
fn a_chief_whose_turn_reads_settled_is_not_switched_before_the_answer_to_its_note_shows() {
    // The fake agent starts a turn as it takes a message, so no test of Node's
    // has a note delivered to a chief whose turn reads settled: its harness
    // has not begun the turn the note starts, or ended it without a word.
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let to = SwitchTo {
        harness: "codex".to_owned(),
        agent: "astraeus".to_owned(),
    };
    context
        .switch_chief(project.id, to, SwitchWhen::Now, true)
        .unwrap();
    // The note goes in, and the look after it sees it arrive.
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.settled = true);
    context.pass().unwrap();
    let harness = || {
        let chief = context.id(project.id, "chief");
        let found = context.project(project.id);
        found
            .participants
            .into_iter()
            .find(|participant| participant.id == chief)
            .and_then(|participant| participant.harness)
    };
    assert_eq!(harness().as_deref(), Some("claude-code"), "not answered");

    context
        .adapter
        .answer("chief", "Where things stand: half done.");
    context.pass().unwrap();
    assert_eq!(harness().as_deref(), Some("codex"));
    context.close();
}
