//! The dispatcher assigns open tasks: each pass gives every open task to the
//! best free member of its tier, a window the human reassigns is stopped
//! first, and the requester hears once when nobody is free.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cf_engine::testing::Context;
use cf_ledger::{NewNote, NewTask};
use serde_json::json;

use crate::fixtures::{assert_match, low, placed, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher assigns open tasks"];

#[test]
fn gives_an_open_task_to_the_free_member_of_its_tier_with_the_fewest_tasks_so_far_the_earliest_joined_first(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("queued", Some("zeus-amber-pine"))
    );
    assert_eq!(
        context.host.last("zeus").unwrap().pane.id,
        "p1-zeus-amber-pine"
    );
    tiers.open_body("Write the docs");
    context.pass().unwrap();
    assert_match(&tiers.assignee(2), "^diana-");
    context.pass().unwrap();
    context.adapter.answer("zeus", "parser done");
    context.adapter.answer("diana", "docs done");
    context.pass().unwrap();
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["done", "done"]
    );

    tiers.open_body("Lexer");
    context.pass().unwrap();
    assert_match(&tiers.assignee(3), "^zeus-");
    context.pass().unwrap();
    context.adapter.answer("zeus", "lexer done");
    context.pass().unwrap();
    tiers.open_body("Tests");
    context.pass().unwrap();
    assert_match(&tiers.assignee(4), "^diana-");
    tiers.open_for("worker", "light", "Rename a file");
    context.pass().unwrap();
    assert_match(&tiers.assignee(5), "^hera-");
    held_to(
        context.close(),
        SUITES,
        "gives an open task to the free member of its tier with the fewest tasks so far, the earliest joined first",
    );
}

#[test]
fn reads_the_projects_once_a_pass_and_opens_a_tasks_new_session_in_the_pass_that_gave_it_out() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    context.pass().unwrap();
    let reads = Rc::new(Cell::new(0));
    let counted = Rc::clone(&reads);
    context.ledger.borrow_mut().watch(Box::new(move |read, _| {
        if read == "projects" {
            counted.set(counted.get() + 1);
        }
        Ok(())
    }));
    context.pass().unwrap();
    assert_eq!(reads.get(), 1, "a pass with nothing to give out");
    tiers.open();
    reads.set(0);
    context.pass().unwrap();
    assert_eq!(reads.get(), 1, "a pass that gives a task out");
    assert_eq!(tiers.assignee(1), "zeus-amber-pine");
    assert_eq!(
        context.host.last("zeus").unwrap().pane.id,
        "p1-zeus-amber-pine",
        "its session's window opened"
    );
    held_to(
        context.close(),
        SUITES,
        "reads the projects once a pass, and opens a task's new session in the pass that gave it out",
    );
}

#[test]
fn shares_the_work_of_one_tier_across_harnesses_the_harness_with_the_fewest_tasks_first() {
    let context = Context::new();
    let member = |agent: &str, harness: &str| json!({ "agent": agent, "harness": harness, "role": "worker", "tier": "standard" });
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [
                member("zeus", "claude-code"),
                member("diana", "claude-code"),
                member("ares", "opencode"),
            ],
        }))
        .unwrap();
    let open = |body: &str| {
        context.create_task(
            project.id,
            NewTask {
                from: "chief".to_owned(),
                pool: Some("worker".to_owned()),
                tier: Some("standard".to_owned()),
                body: body.to_owned(),
                ..NewTask::default()
            },
        );
    };
    let assignee = |number: i64| {
        context
            .task(project.id, number)
            .task
            .assignee
            .unwrap_or_default()
    };
    open("Parser");
    context.pass().unwrap();
    assert_match(&assignee(1), "^zeus-");
    open("Docs");
    context.pass().unwrap();
    // Before: diana, the next Claude worker, since members ranked by their own count.
    assert_match(&assignee(2), "^ares-");
    open("Tests");
    context.pass().unwrap();
    assert_match(&assignee(3), "^diana-");
    held_to(
        context.close(),
        SUITES,
        "shares the work of one tier across harnesses: the harness with the fewest tasks first",
    );
}

#[test]
fn closes_the_old_window_of_a_task_the_human_reassigned_and_gives_the_task_to_another_member() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    let old = context.host.last("zeus").unwrap().pane;
    context
        .ledger
        .borrow_mut()
        .release_task(1, 1, "by @human")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.host.killed(), [old]);
    assert_match(&tiers.assignee(1), "^diana-");
    held_to(
        context.close(),
        SUITES,
        "closes the old window of a task the human reassigned and gives the task to another member",
    );
}

#[test]
fn steps_no_session_with_no_window_and_nothing_for_it_and_steps_it_again_once_something_is() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.participant(&tiers.assignee(1)).unwrap();
    context.adapter.answer("zeus", "Done");
    context.pass().unwrap();
    context
        .ledger
        .borrow_mut()
        .accept_task(1, 1, "chief")
        .unwrap();
    // After a restart no window is open: an aged project's sessions all idle.
    let after = context.make();
    let stepped = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&stepped);
    context.ledger.borrow_mut().watch(Box::new(move |read, id| {
        if read == "next_delivery" {
            seen.borrow_mut().extend(id);
        }
        Ok(())
    }));
    after.pass().unwrap();
    assert!(
        !stepped.borrow().contains(&session.id),
        "the idle session is not stepped"
    );
    assert!(
        stepped.borrow().contains(&tiers.id("chief")),
        "the chief is"
    );
    context
        .ledger
        .borrow_mut()
        .note(
            1,
            &NewNote {
                from: None,
                to: session.handle.clone(),
                body: "One more thing".to_owned(),
                task: None,
            },
        )
        .unwrap();
    after.pass().unwrap();
    assert!(
        stepped.borrow().contains(&session.id),
        "a message on its way makes it worth a step"
    );
    held_to(
        context.close(),
        SUITES,
        "steps no session with no window and nothing for it, and steps it again once something is",
    );
}

#[test]
fn stops_the_window_of_a_task_the_human_reassigns_before_the_task_leaves_it_even_one_the_human_opened(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let session = tiers.assignee(1);
    context.open_window(1, &session).unwrap();
    let old = context.host.last("zeus").unwrap().pane;
    context.reassign_task(1, 1).unwrap();
    // Stopped before anyone else could take the task: no two windows work on one task.
    assert_eq!(context.host.killed(), [old]);
    assert_eq!(placed(&tiers.task(1).task), ("open", None));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_match(&tiers.assignee(1), "^diana-");
    held_to(
        context.close(),
        SUITES,
        "stops the window of a task the human reassigns before the task leaves it, even one the human opened",
    );
}

#[test]
fn keeps_a_task_with_its_window_when_the_window_would_not_stop_and_reassigns_it_once_it_does() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let holder = tiers.assignee(1);
    context.host.refuse_kills.set(true);
    let refused = context.reassign_task(1, 1).unwrap_err().to_string();
    assert_match(
        &refused,
        &format!("@{holder}'s window could not be stopped, so T-1 stays with it: try again"),
    );
    assert_eq!(
        placed(&tiers.task(1).task),
        ("working", Some(holder.as_str()))
    );
    // The refusal left no close in progress: asked again, the window is killed again.
    context.host.refuse_kills.set(false);
    context.reassign_task(1, 1).unwrap();
    assert_eq!(context.host.killed().len(), 2);
    assert_eq!(placed(&tiers.task(1).task), ("open", None));
    held_to(
        context.close(),
        SUITES,
        "keeps a task with its window when the window would not stop, and reassigns it once it does",
    );
}

#[test]
fn stops_no_window_for_a_task_that_may_not_go_back_to_the_board() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "By name");
    context.pass().unwrap();
    context.pass().unwrap();
    let refused = context.reassign_task(project.id, 1).unwrap_err();
    assert_match(&refused.to_string(), "given by name");
    assert!(context.host.killed().is_empty());
    held_to(
        context.close(),
        SUITES,
        "stops no window for a task that may not go back to the board",
    );
}

#[test]
fn tells_the_requester_once_when_nobody_of_the_tier_is_free_and_assigns_when_one_frees_up() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    // Nothing caps a member's sessions: only quota keeps one from new work.
    tiers.open_body("One");
    tiers.open_body("Two");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.quota("zeus", Some(low(97)));
    context.adapter.quota("diana", Some(low(96)));
    context.pass().unwrap();
    context.adapter.answer("zeus", "done");
    context.adapter.answer("diana", "done");
    context.pass().unwrap();
    tiers.open_body("Third");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(3).task.state, "open");
    assert_eq!(
        tiers.notes("chief"),
        ["T-3 waits for a free standard worker: @zeus is low on quota; @diana is low on quota."]
    );
    context.pass().unwrap();
    assert_eq!(tiers.notes("chief").len(), 1, "told once");
    context.advance(2 * 3_600_000);
    context.pass().unwrap();
    assert_match(&tiers.assignee(3), "^zeus-");
    held_to(
        context.close(),
        SUITES,
        "tells the requester once when nobody of the tier is free, and assigns when one frees up",
    );
}

#[test]
fn tells_the_requester_of_a_waiting_task_though_a_deleted_projects_waiting_task_was_told_before_it()
{
    let context = Context::new();
    context.roster.model.replace(Some("m".to_owned()));
    context.roster.gone.borrow_mut().insert("zeus".to_owned());
    let waits = "T-1 waits for a free standard worker: @zeus has no agent any more (zeus is not among your agents: define it, or remove the member).";
    let first = Tiers::new(&context, &["zeus"]);
    let gone = first.open();
    context.pass().unwrap();
    assert_eq!(first.notes("chief"), [waits]);
    context.close_project(first.project.id).unwrap();
    context.delete_project(first.project.id).unwrap();

    let second = Tiers::new(&context, &["zeus"]);
    assert_ne!(
        second.open().id,
        gone.id,
        "the ledger never gives the deleted task's id again"
    );
    context.pass().unwrap();
    assert_eq!(second.notes("chief"), [waits]);
    held_to(
        context.close(),
        SUITES,
        "tells the requester of a waiting task, though a deleted project's waiting task was told before it",
    );
}
