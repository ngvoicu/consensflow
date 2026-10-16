//! A member's launch that is to say the role first, and what it owes until it
//! has: the ownership and the deadline of a launch with a first message, until the
//! note that says the role is told, and a launch settled as one that did not come
//! up where its window exits, is never ready, or cannot be told the role. The
//! message the role went ahead of fails once, and nothing is launched again for
//! it. A chief's is in `chief_launch`.

use std::rc::Rc;

use cf_engine::testing::Context;
use cf_harness::contract::{Held, Readiness};

use crate::fixtures::{after, exhausted};

use super::members::{finished_session, follow_up, said_to};
use super::{codex, recorded, replacements};

/// The windows opened from now on take no paste: the harness refuses each one.
pub(super) fn refusing_pastes(context: &Context) {
    let codex = Rc::downgrade(&context.codex);
    *context.codex.after_prepare.borrow_mut() = Some(Rc::new(move |handle| {
        if let Some(codex) = codex.upgrade() {
            codex.with(handle, |agent| agent.admit = false);
        }
    }));
}

/// The windows are never ready for a paste: the broker does not come up.
pub(super) fn never_ready(context: &Context) {
    context.codex.ready.replace(Some(Rc::new(|| {
        Ok::<_, String>(Readiness::Held(Held::Because(
            "the broker is not up".to_owned(),
        )))
    })));
}

/// How often the last window of `handle` was killed.
pub(super) fn kills_of(context: &Context, handle: &str) -> usize {
    let window = context.host.last(handle).expect("a window").pane;
    context
        .host
        .killed()
        .iter()
        .filter(|pane| pane.generation == window.generation)
        .count()
}

/// How many notes ConsensFlow wrote the human that say `words`.
pub(super) fn told_the_human(context: &Context, project: i64, words: &str) -> usize {
    context
        .inbox(context.id(project, "human"))
        .into_iter()
        .filter(|message| message.body.contains(words))
        .count()
}

#[test]
fn a_members_window_that_exits_before_the_role_was_told_fails_the_follow_up_it_went_ahead_of_once_and_no_launch_follows(
) {
    let context = codex();
    let (project, session, member) = finished_session(&context);
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    assert_eq!(context.codex.prepared().len(), 2, "its window is open");
    assert_eq!(
        said_to(&context, &session),
        Vec::<String>::new(),
        "and nothing was pasted into it"
    );

    // Its process exits before anything goes in: its saved thread cannot be resumed.
    context
        .host
        .exits_showing(Some(3), None, Some(&["Could not resume that thread"]));
    context.exit(&session);
    assert_eq!(
        context.task(project.id, 2).task.state,
        "failed",
        "the follow-up the role went ahead of fails with the launch, as a first message does"
    );
    assert_eq!(
        told_the_human(
            &context,
            project.id,
            "window closed (exit code 3); its screen ended with: \"Could not resume that thread\""
        ),
        1,
        "and says what the window showed, as the failure of a first message does"
    );
    let notes = replacements(&context, member);
    assert_eq!(notes.len(), 1);
    assert_eq!(
        context.message(notes[0].id).state,
        "cancelled",
        "and the role it was to say is withdrawn: no queue of a member's would give it to another window"
    );
    assert_eq!(
        recorded(&context, member).as_deref(),
        Some("instructions for worker"),
        "the session was told nothing"
    );

    // And that is the end of it: no launch follows, and no replacement is written for one.
    for _ in 0..4 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(context.codex.prepared().len(), 2, "no other window opens");
    assert_eq!(
        replacements(&context, member).len(),
        1,
        "no other is written"
    );
    assert_eq!(
        told_the_human(&context, project.id, "did not reach @"),
        1,
        "the human hears of the failed message once"
    );
    context.close();
}

#[test]
fn a_members_window_that_exits_as_it_opens_fails_the_follow_up_it_went_ahead_of_once_and_no_launch_follows(
) {
    let context = codex();
    let (project, session, member) = finished_session(&context);
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    follow_up(&context, &project, "Now the lexer");
    // The host's own exit comes before its answer to the open.
    *context.host.exit_after_open.borrow_mut() = Some(session.clone());
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 2).task.state, "failed");
    for _ in 0..3 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(context.codex.prepared().len(), 2);
    assert_eq!(replacements(&context, member).len(), 1);
    context.close();
}

#[test]
fn a_members_window_that_is_never_ready_for_the_role_is_closed_when_the_time_a_launch_has_is_up_and_fails_the_follow_up_once(
) {
    let context = codex();
    let (project, session, member) = finished_session(&context);
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    never_ready(&context);
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.codex.prepared().len(), 2);

    // It waits as a launch with a first message waits, and no longer than that.
    context.advance(100_000);
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 2).task.state,
        "queued",
        "the time a launch is given is not up"
    );
    assert_eq!(kills_of(&context, &session), 0);
    context.advance(21_000);
    context.pass().unwrap();
    assert_eq!(
        kills_of(&context, &session),
        1,
        "its window is closed, once, as the look gives the launch up"
    );
    assert_eq!(
        context.task(project.id, 2).task.state,
        "failed",
        "the follow-up the role went ahead of fails with the launch"
    );
    let notes = replacements(&context, member);
    assert_eq!(notes.len(), 1);
    assert_eq!(context.message(notes[0].id).state, "cancelled");
    assert_eq!(
        told_the_human(&context, project.id, "did not reach @"),
        1,
        "the human hears of it once"
    );

    // No launch follows at the next pass, and no replacement is written for it.
    for _ in 0..4 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(context.codex.prepared().len(), 2, "no other window opens");
    assert_eq!(replacements(&context, member).len(), 1);
    assert_eq!(kills_of(&context, &session), 1);
    context.close();
}

#[test]
fn a_members_window_is_not_closed_for_the_time_a_launch_has_once_the_delivery_of_the_role_began() {
    let context = codex();
    let (project, session, member) = finished_session(&context);
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    // The window refuses the first paste, and is not ready for the next.
    refusing_pastes(&context);
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    let notes = replacements(&context, member);
    assert_eq!(notes.len(), 1);
    assert_eq!(
        context.message(notes[0].id).attempts,
        1,
        "its delivery began, and it was refused"
    );
    never_ready(&context);
    context.codex.with(&session, |agent| agent.admit = true);
    for _ in 0..4 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(
        context.task(project.id, 2).task.state,
        "queued",
        "from then it is the delivery's own rules that go, and the window is waited for"
    );
    assert_eq!(kills_of(&context, &session), 0);

    // The window is ready: the role is told, and the follow-up after it.
    context.codex.ready.replace(None);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.message(notes[0].id).state, "delivered");
    assert_eq!(
        recorded(&context, member).as_deref(),
        Some("instructions for worker (rewritten)")
    );
    context.close();
}

#[test]
fn a_members_window_that_exits_with_the_role_on_its_way_fails_the_follow_up_once_and_no_launch_follows(
) {
    let context = codex();
    let (project, session, member) = finished_session(&context);
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    // Its harness never records what is pasted.
    context.codex.with(&session, |agent| agent.arrive = false);
    context.pass().unwrap();
    let notes = replacements(&context, member);
    assert_eq!(notes.len(), 1);
    assert_eq!(
        context.message(notes[0].id).state,
        "delivering",
        "the role is on its way: the paste began, its arrival was not seen"
    );

    context.exit(&session);
    assert_eq!(context.task(project.id, 2).task.state, "failed");
    assert_ne!(context.message(notes[0].id).state, "queued");
    for _ in 0..4 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(context.codex.prepared().len(), 2, "no other window opens");
    assert_eq!(
        replacements(&context, member).len(),
        1,
        "no other is written"
    );
    context.close();
}

/// A review of 2026-10-16 (a chief's, in `chief_launch`): the window recorded the note
/// that says its role and went before a look saw it there. The window's end confirms
/// the note from the record: the launch came up, and the follow-up does not fail with
/// it.
#[test]
fn a_members_window_that_exits_once_its_record_showed_the_role_came_up_and_its_follow_up_does_not_fail(
) {
    let context = codex();
    let (project, session, member) = finished_session(&context);
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    let notes = replacements(&context, member);
    assert_eq!(context.message(notes[0].id).state, "delivering");

    context
        .host
        .exits_showing(Some(3), None, Some(&["thread 'main' panicked"]));
    context.exit(&session);
    assert_eq!(context.message(notes[0].id).state, "delivered");
    assert_eq!(
        recorded(&context, member).as_deref(),
        Some("instructions for worker (rewritten)")
    );
    assert_eq!(
        context.task(project.id, 2).task.state,
        "queued",
        "the follow-up waits for the next window"
    );
    assert_eq!(
        told_the_human(&context, project.id, "panicked"),
        0,
        "no launch failed"
    );

    // The next window resumes the session told its role, the follow-up its first
    // message, and no role is said again.
    for _ in 0..4 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    let launches = context.codex.prepared();
    assert_eq!(launches.len(), 3);
    assert!(
        launches[2]["message"]
            .as_str()
            .is_some_and(|message| message.contains("Now the lexer")),
        "{}",
        launches[2]["message"]
    );
    assert_eq!(context.task(project.id, 2).task.state, "working");
    assert_eq!(replacements(&context, member).len(), 1);
    context.close();
}

#[test]
fn a_members_replacement_the_harness_refuses_as_often_as_a_message_is_given_attempts_closes_the_window_and_fails_the_follow_up_once(
) {
    let context = codex();
    let (project, session, member) = finished_session(&context);
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    refusing_pastes(&context);
    follow_up(&context, &project, "Now the lexer");
    for _ in 0..8 {
        context.pass().unwrap();
    }

    let notes = replacements(&context, member);
    assert_eq!(notes.len(), 1);
    assert_eq!(context.message(notes[0].id).state, "failed");
    assert_eq!(context.message(notes[0].id).attempts, 3);
    assert_eq!(
        said_to(&context, &session),
        Vec::<String>::new(),
        "nothing was taken, and the follow-up was not put to a window that was never told its role"
    );
    assert_eq!(
        context.task(project.id, 2).task.state,
        "failed",
        "the follow-up fails: it is not given under the role the conversation has"
    );
    assert_eq!(kills_of(&context, &session), 1, "the window is closed");
    assert_eq!(
        recorded(&context, member).as_deref(),
        Some("instructions for worker"),
        "the session was told nothing"
    );

    // And that is the end of it.
    for _ in 0..4 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(context.codex.prepared().len(), 2, "no other window opens");
    assert_eq!(replacements(&context, member).len(), 1);
    assert_eq!(
        told_the_human(&context, project.id, "did not reach @"),
        2,
        "the human hears of the note, and of the follow-up, once each"
    );
    context.close();
}

#[test]
fn a_members_window_that_exits_just_after_the_role_was_given_up_fails_the_follow_up_once_and_no_launch_follows(
) {
    let context = codex();
    let (project, session, member) = finished_session(&context);
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    refusing_pastes(&context);
    follow_up(&context, &project, "Now the lexer");
    for _ in 0..4 {
        context.pass().unwrap();
    }
    let notes = replacements(&context, member);
    assert_eq!(notes.len(), 1);
    assert_eq!(
        context.message(notes[0].id).state,
        "failed",
        "refused as often as a message is given attempts"
    );
    assert_eq!(
        context.task(project.id, 2).task.state,
        "queued",
        "the look that gives the launch up has not come yet"
    );

    // The window exits first.
    context.exit(&session);
    assert_eq!(context.task(project.id, 2).task.state, "failed");
    for _ in 0..4 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(context.codex.prepared().len(), 2, "no other window opens");
    assert_eq!(replacements(&context, member).len(), 1);
    context.close();
}

#[test]
fn a_members_window_that_waits_to_say_the_role_while_its_member_is_out_of_quota_is_not_given_up_for_the_time_it_waits(
) {
    let context = codex();
    let (project, session, member) = finished_session(&context);
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    never_ready(&context);
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    let resets_at = after(&context, 3 * 3_600_000);
    context
        .codex
        .quota(&session, Some(exhausted(None, Some(&resets_at))));
    context.pass().unwrap();
    // The harness refuses the member's turn for quota: its task is held with it.
    assert_eq!(context.task(project.id, 2).task.state, "paused");
    // Hours go by, more than a launch is given, passes all the while.
    for _ in 0..12 {
        context.advance(10 * 60_000);
        context.pass().unwrap();
    }
    assert_eq!(
        kills_of(&context, &session),
        0,
        "it is not closed for waiting"
    );
    assert_eq!(context.task(project.id, 2).task.state, "paused");
    let notes = replacements(&context, member);
    assert_eq!(notes.len(), 1);
    assert_eq!(context.message(notes[0].id).state, "queued");

    // The human says the member is back before the reset, and the window is
    // ready: the role is told, and the task goes on.
    context.codex.quota(&session, None);
    context.codex.ready.replace(None);
    context.back_from_quota(project.id, &session).unwrap();
    for _ in 0..6 {
        context.pass().unwrap();
    }
    assert_ne!(context.task(project.id, 2).task.state, "failed");
    assert_eq!(
        recorded(&context, member).as_deref(),
        Some("instructions for worker (rewritten)"),
        "told, once the member was back"
    );
    assert_eq!(
        context.codex.prepared().len(),
        2,
        "by the window that waited: the hours it waited out of quota were not the launch's"
    );
    assert_eq!(kills_of(&context, &session), 0);
    context.close();
}
