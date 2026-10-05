//! Windows that close and projects that end: the chief's window closing by
//! itself, a member who leaves, a project closed, resumed and deleted, and
//! one that comes back after a restart (`describe('the dispatcher')`).

use cf_engine::testing::{Context, Gate};
use cf_harness::contract::Pane;
use cf_proto::trace::{Traced, WindowEvent};
use serde_json::json;

use crate::matching::found;
use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher"];

/// The pane of the last window of `handle`.
fn pane_of(context: &Context, handle: &str) -> Pane {
    context.host.last(handle).expect("its window").pane
}

fn state_of(context: &Context, project: i64) -> (String, bool) {
    let found = context.ledger.borrow().project(project).unwrap().unwrap();
    (found.state, found.resume_on_start)
}

#[test]
fn closes_the_project_when_its_chief_window_closes_by_itself_as_close_does() {
    let context = Context::new();
    let project = context.with_tiers(&["zeus", "diana"]);
    context.pool_task(project.id, "worker", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let task = || context.task(project.id, 1);
    assert_eq!(task().task.state, "working");
    let session = task().task.assignee.unwrap();
    let zeus = pane_of(&context, "zeus");
    // The human types /exit in the chief's terminal, or the chief crashes.
    context.exit("chief");
    assert_eq!(
        state_of(&context, project.id),
        ("suspended".to_owned(), false)
    );
    assert_eq!(context.host.killed(), [zeus]);
    assert_eq!(
        context.dispatcher.pane(context.id(project.id, &session)),
        None
    );
    assert_eq!(
        (task().task.state.clone(), task().task.assignee.clone()),
        ("paused".to_owned(), Some(session))
    );
    let note = task().messages.into_iter().find(|m| m.kind == "note");
    assert!(found(
        r"^T-1 is paused: @zeus-amber-pine's window closed\.",
        &note.unwrap().body
    ));
    context.pass().unwrap();
    assert_eq!(
        context.host.opened().len(),
        2,
        "nothing opens while it is closed"
    );
    held_to(
        context.close(),
        SUITES,
        "closes the project when its chief window closes by itself, as Close does: every window goes and the work in them pauses",
    );
}

#[test]
fn opens_a_project_with_the_staff_it_is_given_and_only_the_chief_window() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let handles: Vec<&str> = project.participants.iter().map(|p| &*p.handle).collect();
    assert_eq!(handles, ["human", "chief", "zeus"]);
    let opened: Vec<String> = context
        .host
        .opened()
        .into_iter()
        .map(|open| open.pane.id)
        .collect();
    assert_eq!(opened, [format!("p{}-chief", project.id)]);
    held_to(
        context.close(),
        SUITES,
        "opens a project with the staff it is given, and only the chief window",
    );
}

#[test]
fn closes_the_window_of_a_member_who_leaves_and_its_exit_fails_nothing() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let zeus = context.id(project.id, "zeus");
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();

    let removed = context.remove_member(project.id, "zeus").unwrap();
    assert_eq!(removed.cancelled, [1]);
    assert_eq!(context.host.killed(), [pane_of(&context, "zeus")]);

    context.exit("zeus");
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "cancelled");
    let kinds: Vec<&str> = task.messages.iter().map(|m| &*m.kind).collect();
    assert_eq!(
        kinds,
        ["task"],
        "no \"window closed\" note for a member who left"
    );
    assert_eq!(context.dispatcher.pane(zeus), None);
    let opened = context.host.opened();
    assert_eq!(
        opened
            .iter()
            .filter(|open| open.pane.id.ends_with("-zeus"))
            .count(),
        1
    );
    held_to(
        context.close(),
        SUITES,
        "closes the window of a member who leaves, and its exit fails nothing",
    );
}

#[test]
fn waits_for_a_window_still_opening_before_closing_it_for_a_member_who_leaves() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    let open = Gate::default();
    *context.host.hold.borrow_mut() = Some(open.clone());
    let passing = context.begin_pass();
    let removing = context.begin_remove_member(project.id, "zeus");
    context.settle();
    assert!(
        context.host.killed().is_empty(),
        "the launch is still in progress"
    );

    open.open();
    context.finish(passing).unwrap();
    let removed = context.finish(removing).unwrap();
    assert_eq!(removed.cancelled, [1]);
    assert_eq!(context.host.killed(), [pane_of(&context, "zeus")]);
    held_to(
        context.close(),
        SUITES,
        "waits for a window still opening before closing it for a member who leaves",
    );
}

#[test]
fn closes_a_project() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pool_task(project.id, "worker", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let (chief, zeus) = (pane_of(&context, "chief"), pane_of(&context, "zeus"));
    let native = |handle: &str| {
        let id = context.id(project.id, handle);
        let conversation = context.ledger.borrow().current_conversation(id);
        conversation.unwrap().unwrap().native_session.unwrap()
    };
    let chief_native = native("chief");
    let closed = context.close_project(project.id).unwrap();
    assert_eq!(closed.state, "suspended");
    assert_eq!(context.host.killed(), [chief, zeus]);
    context.exit("chief");
    context.exit("zeus");
    assert_eq!(
        context.dispatcher.pane(context.id(project.id, "chief")),
        None
    );
    let task = context.task(project.id, 1).task;
    assert_eq!(
        (task.state.as_str(), task.assignee.as_deref()),
        ("paused", Some("zeus-amber-pine")),
        "the work waits, with its session, for the chief to resume it"
    );
    context.pass().unwrap();
    assert_eq!(
        context.host.opened().len(),
        2,
        "nothing reopens while suspended"
    );

    context.resume_project(project.id).unwrap();
    let resumed = context.adapter.prepared().pop().unwrap();
    assert_eq!(resumed["resume"], json!(chief_native));
    let zeus_native = native("zeus-amber-pine");
    context.resume_task(project.id, 1, "Go on");
    context.pass().unwrap();
    let back = context.adapter.prepared().pop().unwrap();
    assert_eq!(
        (
            back["participant"]["handle"].clone(),
            back["resume"].clone()
        ),
        (json!("zeus-amber-pine"), json!(zeus_native)),
        "the same session of zeus, on its own conversation"
    );
    held_to(
        context.close(),
        SUITES,
        "closes a project: its windows go, work in them pauses, and Resume brings the chief back",
    );
}

#[test]
fn names_the_window_that_would_not_close_when_a_project_closes_and_closes_it_on_the_next_close() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let chief = pane_of(&context, "chief");
    context.host.refuse_kills.set(true);
    let refused = context.close_project(project.id);
    assert_eq!(
        refused.unwrap_err().to_string(),
        "@chief's window would not close: resume the project and close it again"
    );
    assert_eq!(state_of(&context, project.id).0, "suspended");
    assert_eq!(
        context.dispatcher.pane(context.id(project.id, "chief")),
        Some(chief)
    );
    let failed: Vec<(Option<String>, Option<String>)> = context
        .trace
        .lines
        .borrow()
        .iter()
        .filter_map(|line| match &line.what {
            Traced::Window {
                participant,
                event: WindowEvent::KillFailed { error },
                ..
            } => Some((participant.clone(), error.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        failed,
        [(
            Some("chief".to_owned()),
            Some("refused by the test".to_owned())
        )]
    );
    context.host.refuse_kills.set(false);
    context.close_project(project.id).unwrap();
    assert_eq!(
        context.dispatcher.pane(context.id(project.id, "chief")),
        None,
        "no exit was made up before"
    );
    assert_eq!(context.host.killed().len(), 2);
    held_to(
        context.close(),
        SUITES,
        "names the window that would not close when a project closes, and closes it on the next Close",
    );
}

#[test]
fn deletes_a_closed_project_for_good_refuses_an_open_one_and_leaves_one_line_in_the_trace() {
    let context = Context::new();
    context.trace.forgets.set(true);
    let project = context.with_staff(&["zeus"]);
    let refused = context.delete_project(project.id).unwrap_err();
    assert_eq!(refused.refusal().code, "project-open");
    context.close_project(project.id).unwrap();
    let gone = context.delete_project(project.id).unwrap();
    assert_eq!(
        (gone.id, gone.name.as_str(), gone.directory.as_str()),
        (project.id, "app", "/work/app")
    );
    assert!(context.ledger.borrow().projects().unwrap().is_empty());
    let deleted: Vec<String> = context
        .trace
        .lines
        .borrow()
        .iter()
        .filter_map(|line| match &line.what {
            Traced::ProjectDeleted(deleted) => Some(deleted.name.clone()),
            Traced::Window { .. } => None,
        })
        .collect();
    assert_eq!(deleted, ["app"]);
    assert!(
        context.trace.forgotten.borrow().contains(&project.id),
        "the project's own lines are dropped"
    );
    context.pass().unwrap();
    assert_eq!(context.host.opened().len(), 1, "nothing reopens");
    held_to(
        context.close(),
        SUITES,
        "deletes a closed project for good, refuses an open one, and leaves one line in the trace",
    );
}

#[test]
fn closes_every_window_it_still_has_of_a_deleted_project_however_the_project_was_closed_before_it_forgets_them(
) {
    let context = Context::new();
    let project = context.with_tiers(&["zeus", "diana"]);
    context.pool_task(project.id, "worker", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let windows = [pane_of(&context, "chief"), pane_of(&context, "zeus")];
    // Closed in the ledger alone, as a restart marks a project, with its windows still up.
    context
        .ledger
        .borrow_mut()
        .set_project_state(project.id, "suspended")
        .unwrap();
    context.delete_project(project.id).unwrap();
    assert_eq!(context.host.killed(), windows);
    held_to(
        context.close(),
        SUITES,
        "closes every window it still has of a deleted project, however the project was closed, before it forgets them",
    );
}

#[test]
fn brings_back_the_projects_that_were_open_before_a_restart_on_their_own_conversations() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    let native = || {
        let conversation = context.ledger.borrow().current_conversation(chief);
        conversation.unwrap().unwrap().native_session.unwrap()
    };
    let before = native();
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.resume_after_restart().unwrap();
    let resumed = context.adapter.prepared().pop().unwrap();
    assert_eq!(
        (
            resumed["participant"]["handle"].clone(),
            resumed["resume"].clone()
        ),
        (json!("chief"), json!(before))
    );
    assert_eq!(state_of(&context, project.id), ("open".to_owned(), false));
    assert_eq!(native(), before);
    held_to(
        context.close(),
        SUITES,
        "brings back the projects that were open before a restart, on their own conversations",
    );
}
