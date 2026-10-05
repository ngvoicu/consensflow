//! A participant that leaves (a member off the staff, a session deleted,
//! every one of a deleted project) is forgotten at once, quota marks and
//! all: a member that comes back, or the next project's session, starts
//! clean; and work of it that waited meanwhile does nothing more.

use std::rc::Rc;

use cf_engine::testing::{Context, Gate};
use cf_harness::records::Role;
use cf_ledger::{NewMember, ParticipantView};
use serde_json::{json, Value};

use crate::fixtures::{low, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["a participant that leaves"];

/// A standard worker on Claude Code joins the staff (`ledger.addMember`).
fn add_worker(context: &Context, project: i64, agent: &str) -> ParticipantView {
    context
        .ledger
        .borrow_mut()
        .add_member(
            project,
            &NewMember {
                agent: agent.to_owned(),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .unwrap()
}

/// What zeus's window was writing when its turn was cut: words not yet complete.
fn push_half(context: &Context) {
    let mut half = context.adapter.item(Role::Assistant, "Half");
    half.complete = false;
    context.adapter.with("zeus", |agent| agent.items.push(half));
}

/// The tokens revoked so far, in order.
fn revoked(context: &Context) -> Vec<Value> {
    context
        .recorder
        .calls("credentials", &["revoke"])
        .into_iter()
        .map(|(_, given)| given[0].clone())
        .collect()
}

#[test]
fn is_forgotten_a_member_that_comes_back_to_the_staff_starts_clean_not_low_on_quota() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.quota("zeus", Some(low(97)));
    context.pass().unwrap();
    context.remove_member(tiers.project.id, "zeus").unwrap();
    add_worker(&context, tiers.project.id, "zeus");
    tiers.open_body("Write the lexer");
    context.pass().unwrap();
    assert!(
        tiers
            .task(2)
            .task
            .assignee
            .is_some_and(|session| session.starts_with("zeus-")),
        "zeus takes it: the low quota was before"
    );
    held_to(
        context.close(),
        SUITES,
        "is forgotten: a member that comes back to the staff starts clean, not low on quota",
    );
}

#[test]
fn is_forgotten_with_its_project_a_session_of_the_next_project_starts_clean() {
    let context = Context::new();
    let first = Tiers::new(&context, &["zeus"]);
    first.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let gone = first.id("zeus-amber-pine");
    // Its window copied two items, the human pinned it open, and its agent was interrupted.
    push_half(&context);
    context
        .open_window(first.project.id, "zeus-amber-pine")
        .unwrap();
    context.pause_task(first.project.id, 1);
    context.pass().unwrap();
    context.remove_member(first.project.id, "zeus").unwrap();
    context.pass().unwrap();
    context.close_project(first.project.id).unwrap();
    context.delete_project(first.project.id).unwrap();

    let second = Tiers::new(&context, &["zeus"]);
    second.open();
    context.pass().unwrap();
    assert_ne!(
        second.id(&second.assignee(1)),
        gone,
        "the ledger never gives its id again"
    );
    push_half(&context);
    context.pass().unwrap();
    let copied = context
        .ledger
        .borrow()
        .transcript(second.project.id, 1, None)
        .unwrap();
    let roles: Vec<&str> = copied.items.iter().map(|item| item.role.as_str()).collect();
    assert_eq!(
        roles,
        ["user", "assistant"],
        "its copy starts with its brief"
    );
    let pane = context.host.last("zeus").unwrap().pane;
    context.pause_task(second.project.id, 1);
    context.pass().unwrap();
    let interrupts = context
        .host
        .inputs()
        .iter()
        .filter(|body| body["generation"] == json!(pane.generation))
        .count();
    assert_eq!(interrupts, 1, "its agent is interrupted");
    context
        .ledger
        .borrow_mut()
        .cancel_task(second.project.id, 1, "chief")
        .unwrap();
    context.pass().unwrap();
    assert!(
        context
            .host
            .killed()
            .iter()
            .any(|killed| killed.generation == pane.generation),
        "its window closes with its work: nobody pinned it"
    );
    held_to(
        context.close(),
        SUITES,
        "is forgotten with its project: a session of the next project starts clean",
    );
}

#[test]
fn keeps_the_chief_of_a_project_created_while_a_deleted_one_still_closes_its_windows() {
    let context = Context::new();
    let old = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The old chief takes a paste its harness holds: its window closes once that is over.
    let held = Gate::default();
    let waiting = held.clone();
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        let waiting = waiting.clone();
        Box::pin(async move {
            waiting.wait().await;
            Ok(taking())
        })
    }));
    context.note(old.id, "zeus", "chief", "Held");
    context.pass().unwrap();
    let closing = context.begin_close_project(old.id);
    let deleting = context.begin_delete_project(old.id);
    let opening = context.begin_open_project(json!({
        "directory": "/work/api",
        "name": "api",
        "chief": { "harness": "claude-code", "agent": "apollo" },
    }));
    let fresh = context.finish(opening).unwrap().unwrap();
    let chief = fresh
        .participants
        .iter()
        .find(|participant| participant.role == "chief")
        .unwrap()
        .id;
    held.open();
    context.finish(closing).unwrap();
    context.finish(deleting).unwrap();
    context.settle();
    let window = context.host.last("chief").unwrap().pane;
    assert_eq!(
        context.dispatcher.pane(chief),
        Some(window.clone()),
        "its chief is known"
    );
    assert!(
        !context
            .host
            .killed()
            .iter()
            .any(|pane| pane.generation == window.generation),
        "and was never closed"
    );
    held_to(
        context.close(),
        SUITES,
        "keeps the chief of a project created while a deleted one still closes its windows",
    );
}

#[test]
fn opens_nothing_for_a_sessions_open_that_waited_while_its_project_was_deleted_nor_for_a_new_projects_member(
) {
    let context = Context::new();
    let first = Tiers::new(&context, &["zeus"]);
    first.open();
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    assert_eq!(
        first.task(1).task.state,
        "done",
        "the session's window went with its task"
    );
    let session = first.id("zeus-amber-pine");
    // The human opens the session's window, and again while the first one
    // comes up; that one exits at once, before its open is answered.
    let held = Gate::default();
    *context.adapter.hold_prepares.borrow_mut() =
        Some(("zeus-amber-pine".to_owned(), held.clone()));
    let launches = context.adapter.prepared().len();
    context
        .open_window(first.project.id, "zeus-amber-pine")
        .unwrap();
    context
        .open_window(first.project.id, "zeus-amber-pine")
        .unwrap();
    // Meanwhile the project is closed and deleted, and a new one is opened with a member added.
    let closing = context.begin_close_project(first.project.id);
    let deleting = context.begin_delete_project(first.project.id);
    let second = Tiers::new(&context, &["zeus"]);
    let diana = add_worker(&context, second.project.id, "diana");
    assert_ne!(
        diana.id, session,
        "the ledger never gives the session's id to a new member"
    );
    // Node's host wrapped every open of the session's window to exit it at
    // once; the one open it matches is the launch the held prepare lets go.
    *context.host.exit_after_open.borrow_mut() = Some("zeus-amber-pine".to_owned());
    held.open();
    context.finish(closing).unwrap();
    context.finish(deleting).unwrap();
    context.settle();
    let launched: Vec<String> = context
        .adapter
        .prepared()
        .into_iter()
        .skip(launches)
        .filter(|request| {
            let of = (
                request["participant"]["handle"].as_str().unwrap(),
                request["project"]["id"].as_i64().unwrap(),
            );
            [
                ("zeus-amber-pine", first.project.id),
                ("diana", second.project.id),
            ]
            .contains(&of)
        })
        .map(|request| {
            request["participant"]["handle"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        launched,
        ["zeus-amber-pine"],
        "the first Open launched for the session, and the second opened nothing, nor for diana"
    );
    assert_eq!(
        context.dispatcher.pane(session),
        None,
        "no window has its id"
    );
    held_to(
        context.close(),
        SUITES,
        "opens nothing for a session's Open that waited while its project was deleted, nor for a new project's member",
    );
}

#[test]
fn stops_a_closed_projects_windows_acting_at_once_before_their_exits_come() {
    // A token names a participant and a project by id. Once the project is
    // closed none of its windows may act, exit or no exit; once it is
    // deleted, those ids name nothing.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.host.hold_exits.set(true);
    context.close_project(project.id).unwrap();
    assert_eq!(
        revoked(&context),
        [json!("token-chief")],
        "closing revokes its windows"
    );
    context.delete_project(project.id).unwrap();
    assert_eq!(
        revoked(&context),
        [json!("token-chief")],
        "deleting finds none left"
    );
    held_to(
        context.close(),
        SUITES,
        "stops a closed project's windows acting at once, before their exits come",
    );
}
