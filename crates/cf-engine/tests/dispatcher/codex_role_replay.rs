//! T-1232's P3 (never-close, merged in T-1190): a Codex session whose brief never began, and
//! whose role text changes before its relaunch, reached the way the engine reaches it, with no
//! call of the ledger's `relaunch_stalled` of its own. The relaunch opens the window on its
//! conversation with the new role first (`windows.rs`: the launch's message is given back to
//! its queue, "its window is told its role first"), and the replay of the brief that stalled
//! follows as an ordinary paste. Its window's exit and a switch of its record are then read for
//! that replay: missing, it stays unconfirmed, and goes in once on the Resume; arrived, it is
//! received by its new copy, never by the old one the stall left in the record.

use cf_engine::testing::Context;
use cf_harness::records::Role;
use serde_json::{json, Value};

use crate::brief_never_began::PAST_ARRIVAL;
use crate::chiefs::with_codex;
use crate::fixtures::native_of;

/// The project's new rules for its staff: its members' role text changes with them.
const RULES: &str = "Write every comment in British English.";

/// A stalled follow-up on a Codex session, as the engine left it.
struct Stalled {
    project: i64,
    /// The session's handle.
    session: String,
    /// The brief's message.
    brief: i64,
    /// The record's item of the copy that stalled.
    old: String,
}

/// T-1 done by diana's Codex session; T-2, `--after T-1`, given to the same session, which
/// never begins it: the stall pauses T-2 (`never_began`).
fn stalled(context: &Context) -> Stalled {
    context.codex.keeps_first_instructions.set(true);
    context.host.exits_showing(Some(0), None, None);
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "model": "claude-opus-5" },
            "staff": [{ "agent": "diana", "harness": "codex", "roles": ["worker"], "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project")
        .id;
    context.pool_task(project, "worker", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = context
        .task(project, 1)
        .task
        .assignee
        .expect("T-1's session");
    context.codex.answer(&session, "Parser done");
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "done");
    context.create_task(
        project,
        cf_ledger::NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..cf_ledger::NewTask::default()
        },
    );
    context.codex.never_begins.set(Some(false));
    context
        .ledger
        .borrow_mut()
        .accept_task(project, 1, "chief")
        .unwrap();
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let thread = context.task(project, 2);
    assert_eq!(thread.task.state, "working", "its record shows the brief");
    assert_eq!(thread.task.assignee.as_deref(), Some(session.as_str()));
    let brief = thread
        .messages
        .iter()
        .find(|message| message.kind == "task")
        .expect("its brief")
        .id;
    context
        .host
        .set_snapshot(json!({ "outputQuietMs": 600_000, "screenQuietMs": 600_000 }));
    for _ in 0..2 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
        context.pass().unwrap();
    }
    assert_eq!(context.task(project, 2).task.state, "paused", "stalled");
    let old = context.message(brief).receipt["item"]
        .as_str()
        .expect("received once")
        .to_owned();
    Stalled {
        project,
        session,
        brief,
        old,
    }
}

/// The brief's copies in the session's record after the old copy.
fn copies_after(context: &Context, stalled: &Stalled) -> usize {
    let marker = format!("m-{} · T-2 · task", stalled.brief);
    let items = context.codex.agent(&stalled.session).items;
    let from = items
        .iter()
        .rposition(|item| *item.id == *stalled.old)
        .map_or(0, |at| at + 1);
    items[from..]
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains(&marker))
        .count()
}

/// The staff's rules change, so the session's role text does; the stall's rounds then relaunch
/// its window on its conversation. The launch says the new role first and carries no message:
/// the replay of the brief is given back, and pasted after the role's note as any message is.
/// It is pasted, and no look has read it yet.
fn relaunched_with_the_role_first(context: &Context, stalled: &Stalled) {
    context
        .ledger
        .borrow_mut()
        .set_staff_rules(stalled.project, Some(RULES))
        .unwrap();
    let launches = context.codex.prepared().len();
    let kills = context.host.killed().len();
    context.codex.never_begins.set(None);
    for _ in 0..6 {
        context.advance(5_000);
        context.pass().unwrap();
    }
    assert!(
        context.host.killed().len() > kills,
        "the relaunch retired it"
    );
    let relaunch = context
        .codex
        .prepared()
        .into_iter()
        .skip(launches)
        .last()
        .expect("relaunched");
    assert_eq!(relaunch["message"], Value::Null, "the role goes first");
    assert!(
        relaunch["instructions"]
            .as_str()
            .is_some_and(|role| role.contains(RULES)),
        "the new role: {relaunch}"
    );
    // The role's note goes in, the window comes to rest, and the replay follows it.
    for _ in 0..8 {
        context.codex.answer(&stalled.session, "Understood.");
        context.pass().unwrap();
        if copies_after(context, stalled) == 1 {
            break;
        }
        context.advance(3_000);
    }
    assert_eq!(
        copies_after(context, stalled),
        1,
        "the replay pasted after the old copy"
    );
    assert_eq!(
        context.message(stalled.brief).state,
        "delivering",
        "no look read it yet"
    );
}

/// The replay that went in was received by its own copy, the one after the old.
fn received_by_the_new_copy(context: &Context, stalled: &Stalled) {
    let message = context.message(stalled.brief);
    assert_eq!(message.state, "delivered", "{:?}", message.receipt);
    let new = message.receipt["item"].as_str().expect("its receipt");
    assert_ne!(
        new, stalled.old,
        "the new copy's receipt, not the old one's"
    );
}

/// The replay its record lost: taken out of the session's record, which then holds only the old
/// copy of the brief.
fn the_replay_lost(context: &Context, stalled: &Stalled) {
    let marker = format!("m-{} · T-2 · task", stalled.brief);
    context.codex.with(&stalled.session, |agent| {
        let at = agent
            .items
            .iter()
            .rposition(|item| item.role == Role::User && item.text.contains(&marker))
            .expect("the replay");
        agent.items.remove(at);
    });
}

/// A replay that went nowhere was not taken for the old copy; the Resume gives it once.
fn unconfirmed_then_given_once_on_the_resume(context: &Context, stalled: &Stalled) {
    let message = context.message(stalled.brief);
    assert_ne!(message.state, "delivered", "{:?}", message.receipt);
    assert_ne!(
        message.receipt["item"],
        stalled.old.as_str(),
        "{:?}",
        message.receipt
    );
    context.close_project(stalled.project).unwrap();
    context.settle();
    context.resume_project(stalled.project).unwrap();
    // The window comes back, is told what it is to be told first, comes to rest, and is given the
    // brief.
    for _ in 0..8 {
        context.pass().unwrap();
        if context.message(stalled.brief).state == "delivered" {
            break;
        }
        context.codex.answer(&stalled.session, "Understood.");
        context.advance(3_000);
    }
    received_by_the_new_copy(context, stalled);
    assert_eq!(
        copies_after(context, stalled),
        1,
        "given once after the old copy"
    );
}

#[test]
fn a_codex_window_that_exits_with_its_replay_after_a_new_role_receives_it_by_the_new_copy() {
    let context = with_codex();
    let stalled = stalled(&context);
    relaunched_with_the_role_first(&context, &stalled);
    context.exit(&stalled.session);
    context.settle();
    received_by_the_new_copy(&context, &stalled);
}

#[test]
fn a_codex_window_that_exits_with_its_replay_after_a_new_role_lost_leaves_it_for_the_resume() {
    let context = with_codex();
    let stalled = stalled(&context);
    relaunched_with_the_role_first(&context, &stalled);
    the_replay_lost(&context, &stalled);
    context.exit(&stalled.session);
    context.settle();
    unconfirmed_then_given_once_on_the_resume(&context, &stalled);
}

/// The paste waits in the harness's queue when the human switches the window to another
/// conversation, and runs in the one it left (`watch_left`), whose record holds the old copy.
fn switched_away(context: &Context, stalled: &Stalled) -> String {
    let left = native_of(context, context.id(stalled.project, &stalled.session))
        .expect("its conversation");
    context
        .codex
        .switch_to(&stalled.session, "native-elsewhere");
    context.pass().unwrap();
    context.settle();
    left
}

#[test]
fn a_switched_codex_window_receives_its_replay_after_a_new_role_by_the_new_copy() {
    let context = with_codex();
    let stalled = stalled(&context);
    relaunched_with_the_role_first(&context, &stalled);
    let marker = format!("m-{} · T-2 · task", stalled.brief);
    let queued = context.codex.with(&stalled.session, |agent| {
        let at = agent
            .items
            .iter()
            .rposition(|item| item.role == Role::User && item.text.contains(&marker))
            .expect("the replay");
        agent.items.remove(at)
    });
    let left = switched_away(&context, &stalled);
    assert_eq!(
        context.message(stalled.brief).state,
        "delivering",
        "not in the look before the switch"
    );
    context.codex.with(&stalled.session, |agent| {
        agent
            .records
            .get_mut(&left)
            .expect("the record it left")
            .push(queued)
    });
    for _ in 0..3 {
        context.pass().unwrap();
        context.settle();
    }
    received_by_the_new_copy(&context, &stalled);
}

#[test]
fn a_switched_codex_window_whose_replay_after_a_new_role_never_ran_leaves_it_for_the_resume() {
    let context = with_codex();
    let stalled = stalled(&context);
    relaunched_with_the_role_first(&context, &stalled);
    the_replay_lost(&context, &stalled);
    switched_away(&context, &stalled);
    for _ in 0..3 {
        context.pass().unwrap();
        context.settle();
    }
    unconfirmed_then_given_once_on_the_resume(&context, &stalled);
}
