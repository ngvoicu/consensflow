//! A switch after the chief's turn does not end the chief's background agents.
//! A switch is no message: it closes the window, and the agents' work dies with
//! it. So a switch the human asked for after the turn waits until the turn has
//! ended with none pending, as it waited for them while a chief with agents never
//! read at rest. A message goes in between turns, the note that asks where things
//! stand among them. A switch now goes at once, as the human asked. Node's
//! dispatcher had no agents, so none of these is held to a Node trace.

use cf_engine::SwitchWhen;

use crate::chiefs::{chief_of, to, with_codex};
use crate::fixtures::given;

/// The harness the project's chief runs on now.
fn harness(context: &cf_engine::testing::Context, project: i64) -> String {
    chief_of(context, project).harness.unwrap_or_default()
}

#[test]
fn a_switch_after_the_turn_waits_while_the_chiefs_agents_run_and_goes_once_a_turn_ends_with_none() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    // The chief's turn ended with three research agents still running.
    context
        .adapter
        .answer_leaving_agents("chief", "Three agents are researching it");
    context.pass().unwrap();
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Turn, false)
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        harness(&context, project.id),
        "claude-code",
        "the window would close and its agents end with it"
    );
    assert!(context.host.killed().is_empty(), "the window is open");
    assert_eq!(
        context.dispatcher.pending_switch(chief),
        Some(to("codex", "astraeus"))
    );

    // An agent ends and wakes a turn of its own, which leaves two: not yet.
    context.adapter.busy("chief");
    context.pass().unwrap();
    context
        .adapter
        .answer_leaving_agents("chief", "Two are left");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(harness(&context, project.id), "claude-code");
    assert!(context.host.killed().is_empty());

    // The last ends, and the turn it wakes leaves none: the chief goes.
    context.adapter.busy("chief");
    context.pass().unwrap();
    context.adapter.answer("chief", "All three are in");
    context.pass().unwrap();
    assert_eq!(harness(&context, project.id), "codex");
    assert_eq!(context.dispatcher.pending_switch(chief), None);
    assert_eq!(context.host.killed().len(), 1, "its window closed once");
}

#[test]
fn the_note_that_asks_where_things_stand_goes_in_between_turns_and_the_switch_still_waits_for_the_agents(
) {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context
        .adapter
        .answer_leaving_agents("chief", "Two agents are researching it");
    context.pass().unwrap();
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Turn, true)
        .unwrap();

    // The note is a message: it goes into the window at rest, though its agents run.
    context.pass().unwrap();
    context.pass().unwrap();
    let said = given(&context, "chief");
    assert!(
        said.last()
            .is_some_and(|last| last.contains("Write down where things stand")),
        "{said:?}"
    );
    assert_eq!(
        harness(&context, project.id),
        "claude-code",
        "it has not answered yet"
    );

    // It answers, its agents run on: the switch waits for them.
    context
        .adapter
        .answer_leaving_agents("chief", "Where things stand: the research is out.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(harness(&context, project.id), "claude-code");
    assert!(context.host.killed().is_empty());

    context.adapter.busy("chief");
    context.pass().unwrap();
    context
        .adapter
        .answer("chief", "Both are in; it is all above.");
    context.pass().unwrap();
    assert_eq!(harness(&context, project.id), "codex");
}

#[test]
fn a_switch_now_goes_at_once_whatever_the_chief_left_running() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context
        .adapter
        .answer_leaving_agents("chief", "Three agents are researching it");
    context.pass().unwrap();

    // The human asked for it now: the chief's agents end with its window.
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    assert_eq!(harness(&context, project.id), "codex");
    assert_eq!(context.host.killed().len(), 1);
}
