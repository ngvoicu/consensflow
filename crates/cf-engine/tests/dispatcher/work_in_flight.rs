//! Work in flight when its participant is forgotten (a member taken off the
//! staff, a project deleted) and another is created meanwhile: the work does
//! nothing more by the ids it took, which name rows gone with their project or
//! a member that may be back, and nothing of the new one is touched.
//!
//! Each test holds what the old work waits on (`hold` in the Node suite),
//! lets the project be replaced, and then lets the work go. These are the
//! looks and the steps; the launches and the pastes are in
//! [`crate::work_in_flight_launches`] and [`crate::work_in_flight_pastes`].

use cf_engine::testing::Context;
use cf_harness::records::Role;
use cf_ledger::{NewMember, ParticipantView};
use serde_json::Value;

use crate::chiefs::{chief_of, replace_project};
use crate::fixtures::Tiers;
use crate::traces::held_to;

const SUITES: &[&str] = &["work in flight when its participant is forgotten"];

/// A call into the window of this launch (`looksAt`), told its arguments as
/// the Node traces write them.
pub fn looks_at(launch: String) -> impl Fn(&Value) -> bool {
    move |args| args[0]["launch"]["launchId"] == launch.as_str()
}

/// The lines the trace was told, as the daemon writes them (`entries`).
pub fn entries(context: &Context) -> Vec<Value> {
    let lines = context.trace.lines.borrow();
    lines
        .iter()
        .map(|line| serde_json::to_value(line).unwrap())
        .collect()
}

/// A member of the standard tier joins the staff (`ledger.addMember`).
fn add_worker(context: &Context, project: i64, agent: &str) -> ParticipantView {
    context
        .ledger
        .borrow_mut()
        .add_member(
            project,
            &NewMember {
                model: cf_engine::testing::MODELS
                    .iter()
                    .find(|(name, _)| *name == agent)
                    .map(|(_, model)| (*model).to_owned()),
                effort: None,
                model_name: Some(agent.to_owned()),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .unwrap()
}

#[test]
fn copies_and_delivers_nothing_once_the_chiefs_project_is_deleted_while_its_window_is_looked_at_nor_for_a_project_created_meanwhile(
) {
    let context = Context::new();
    let old = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The human told the old chief something since the last look, which waits on its harness.
    let told = "The codeword is tern";
    let said = context.adapter.item(Role::User, told);
    context
        .adapter
        .with("chief", |agent| agent.items.push(said));
    let launch = context.adapter.agent("chief").launch;
    let release = context.adapter.observe_holds.hold(looks_at(launch));
    let looking = context.begin_pass();
    let replaced = replace_project(&context, &old);
    let welcome = context.note_from_consensflow(replaced.fresh.id, "chief", "Welcome");
    release.open();
    context.settle();
    looking.take().unwrap().unwrap();
    replaced.gone();
    context.settle();
    let chief = chief_of(&context, replaced.fresh.id).id;
    assert_eq!(
        context
            .ledger
            .borrow()
            .copied_item_with(chief, told)
            .unwrap(),
        None,
        "the new chief's conversation has nothing of the old window"
    );
    assert_eq!(
        context.message(welcome.id).state,
        "queued",
        "its message waits for its own window"
    );
    held_to(
        context.close(),
        SUITES,
        "copies and delivers nothing once the chief's project is deleted while its window is looked at, nor for a project created meanwhile",
    );
}

#[test]
fn says_nothing_once_the_chiefs_project_is_deleted_while_its_windows_look_fails_of_it_or_a_project_created_meanwhile(
) {
    let context = Context::new();
    let old = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let launch = context.adapter.agent("chief").launch;
    let release = context
        .adapter
        .observe_holds
        .hold_instead(looks_at(launch), "the record could not be read".to_owned());
    let looking = context.begin_pass();
    let replaced = replace_project(&context, &old);
    release.open();
    context.settle();
    looking.take().unwrap().unwrap();
    replaced.gone();
    let unknown: Vec<_> = entries(&context)
        .into_iter()
        .filter(|entry| entry["kind"] == "window.activity" && entry["state"] == "unknown")
        .collect();
    assert!(
        unknown.is_empty(),
        "no line says the old chief's window, or the new chief's, could not be read"
    );
    held_to(
        context.close(),
        SUITES,
        "says nothing once the chief's project is deleted while its window's look fails, of it or a project created meanwhile",
    );
}

#[test]
fn delivers_nothing_to_a_sessions_window_once_its_project_is_deleted_while_its_paused_tasks_agent_is_interrupted_nor_for_a_project_created_meanwhile(
) {
    let context = Context::new();
    let first = Tiers::new(&context, &["zeus"]);
    first.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(first.task(1).task.state, "working");
    let session = first.id("zeus-amber-pine");
    // The human keeps the session's window open, so it stays after its
    // work. The chief pauses the task; the Escape that stops its agent
    // waits on the pane host.
    context
        .open_window(first.project.id, "zeus-amber-pine")
        .unwrap();
    context.pause_task(first.project.id, 1);
    context.adapter.with("zeus", |agent| agent.settled = true);
    let generation = context.host.last("zeus").unwrap().pane.generation;
    let release = context
        .host
        .request_holds
        .hold(move |args| args[0] == "pane.input" && args[1]["generation"] == generation);
    let stepping = context.begin_pass();
    context.settle();
    // Meanwhile the project is closed and deleted, and a member of a new one is given a task.
    let closing = context.begin_close_project(first.project.id);
    let deleting = context.begin_delete_project(first.project.id);
    let second = Tiers::new(&context, &["zeus"]);
    let diana = add_worker(&context, second.project.id, "diana");
    assert_ne!(
        diana.id, session,
        "the ledger never gives the session's id to a new member"
    );
    context.give(second.project.id, "diana", "Docs");
    release.open();
    context.settle();
    stepping.take().unwrap().unwrap();
    closing.take().unwrap().unwrap();
    deleting.take().unwrap().unwrap();
    context.settle();
    let states: Vec<_> = context
        .inbox(diana.id)
        .into_iter()
        .map(|message| message.state)
        .collect();
    assert_eq!(states, ["queued"], "its brief waits for its own window");
    held_to(
        context.close(),
        SUITES,
        "delivers nothing to a session's window once its project is deleted while its paused task's agent is interrupted, nor for a project created meanwhile",
    );
}
