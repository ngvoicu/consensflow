//! One task per member session, a session the human deleted or whose window
//! did not open: a follow-up brings a deleted session back on its
//! conversation, a harness that lost the conversation fails the launch, and a
//! closed project opens no window.

use std::rc::Rc;

use cf_engine::testing::Context;
use cf_ledger::NewTask;

use crate::fixtures::{assert_match, closed, finished, last_launch, last_message, native_of};
use crate::traces::held_to;

const SUITES: &[&str] = &["one task per member session"];

#[test]
fn brings_a_deleted_session_back_when_a_follow_up_comes_with_after_its_window_opens_on_its_own_conversation_and_a_busy_session_still_refuses(
) {
    let context = Context::new();
    let tiers = finished(&context);
    let session = tiers.id("zeus-amber-pine");
    let native = native_of(&context, session);
    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .unwrap();
    context
        .end_session(tiers.project.id, "zeus-amber-pine")
        .unwrap();
    assert!(
        tiers.participant("zeus-amber-pine").is_none(),
        "off the board"
    );
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer, in the same style".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    assert_eq!(
        last_launch(&context),
        ("zeus-amber-pine".to_owned(), native),
        "the session that did T-1, on its own conversation"
    );
    assert_match(
        &last_message(&context),
        r"^\[ConsensFlow m-\d+ · T-2 · task from @chief\]\nNow the lexer, in the same style$",
    );
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(
        tiers.participant("zeus-amber-pine").map(|back| back.id),
        Some(session),
        "back on the board"
    );
    let busy = context
        .ledger
        .borrow_mut()
        .create_task(
            tiers.project.id,
            &NewTask {
                from: "chief".to_owned(),
                after: Some(1),
                body: "More".to_owned(),
                ..NewTask::default()
            },
        )
        .unwrap_err();
    assert_eq!(busy.code(), Some("session-busy"));
    context.adapter.answer("zeus-amber-pine", "Lexer done");
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "done");
    assert!(
        closed(
            &context,
            &context.host.last("zeus-amber-pine").unwrap().pane
        ),
        "its window closes with its task as any"
    );
    held_to(
        context.close(),
        SUITES,
        "brings a deleted session back when a follow-up comes with --after: its window opens on its own conversation, and a busy session still refuses",
    );
}

#[test]
fn brings_a_deleted_session_back_when_its_task_is_reopened_its_window_opens_on_its_own_conversation(
) {
    let context = Context::new();
    let tiers = finished(&context);
    let session = tiers.id("zeus-amber-pine");
    let native = native_of(&context, session);
    context
        .end_session(tiers.project.id, "zeus-amber-pine")
        .unwrap();
    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 1, "chief", "Handle empty input too")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(
        last_launch(&context),
        ("zeus-amber-pine".to_owned(), native),
        "the same window comes back on its own conversation"
    );
    assert_match(
        &last_message(&context),
        r"^\[ConsensFlow m-\d+ · T-1 · task from @chief\]\nHandle empty input too$",
    );
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    held_to(
        context.close(),
        SUITES,
        "brings a deleted session back when its task is reopened: its window opens on its own conversation",
    );
}

#[test]
fn fails_a_deleted_sessions_follow_up_as_any_launch_fails_when_its_harness_has_lost_the_conversation_and_the_task_goes_to_its_tier_instead(
) {
    let context = Context::new();
    let tiers = finished(&context);
    context
        .end_session(tiers.project.id, "zeus-amber-pine")
        .unwrap();
    *context.adapter.prepare.borrow_mut() = Some(Rc::new(|asked| {
        if asked["resume"].is_null() {
            Ok(())
        } else {
            Err("no conversation found with that id".to_owned())
        }
    }));
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "failed");
    assert_match(
        tiers.notes("chief").last().unwrap(),
        r"^T-2 failed: the launch failed: no conversation found with that id\. Reopen it with",
    );
    tiers.open_body("Now the lexer");
    context.pass().unwrap();
    assert_eq!(
        last_launch(&context),
        ("zeus-brisk-birch".to_owned(), None),
        "given to its tier, a fresh session takes it"
    );
    held_to(
        context.close(),
        SUITES,
        "fails a deleted session's follow-up as any launch fails when its harness has lost the conversation, and the task goes to its tier instead",
    );
}

#[test]
fn opens_no_sessions_window_in_a_closed_project() {
    let context = Context::new();
    let tiers = finished(&context);
    context.close_project(tiers.project.id).unwrap();
    let opened = context.host.opened().len();
    let refused = context
        .open_window(tiers.project.id, "zeus-amber-pine")
        .unwrap_err()
        .to_string();
    assert_match(&refused, "app is closed: resume it first");
    context.pass().unwrap();
    assert_eq!(context.host.opened().len(), opened);
    held_to(
        context.close(),
        SUITES,
        "opens no session's window in a closed project",
    );
}

#[test]
fn tells_the_human_why_a_sessions_window_they_opened_did_not_start() {
    let context = Context::new();
    let tiers = finished(&context);
    context.host.refuse.set(true);
    context
        .open_window(tiers.project.id, "zeus-amber-pine")
        .unwrap();
    assert_eq!(
        tiers.notes("human"),
        ["@zeus-amber-pine could not start: the window did not open: refused by the test."]
    );
    context.roster.gone.borrow_mut().insert("zeus".to_owned());
    context
        .open_window(tiers.project.id, "zeus-amber-pine")
        .unwrap();
    assert_eq!(
        tiers.notes("human").last().unwrap(),
        "@zeus-amber-pine could not start: zeus is no longer among your agents."
    );
    held_to(
        context.close(),
        SUITES,
        "tells the human why a session's window they opened did not start",
    );
}
