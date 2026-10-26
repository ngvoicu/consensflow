//! A window whose open the pane host did not answer by the open's deadline (kluris,
//! 2026-10-26, T-668): the host goes on with an open it was asked for, and the window's
//! program started three seconds after the daemon gave up on it, ran the brief its
//! command line carried and held the Codex thread the task sent again opened on. The
//! engine ends such a window, as the participant's, before another of its windows opens,
//! and takes no proof from it: its program may have run on the conversation. The rig's
//! regression (`a_window_whose_open_missed_its_deadline_worked_on_unseen`) holds the real
//! host to its part. Node's dispatcher had none of it, so none of these is held to a Node
//! trace.

use cf_engine::testing::Context;
use cf_ledger::NewTask;

use crate::fixtures::{closed, native_of, Tiers};

/// A project of zeus and diana, T-1 at work in the window of a session of it, whose
/// window then closes with its result, its program proven gone. The session.
fn session_done(context: &Context) -> (Tiers<'_>, String) {
    context.host.exits_showing(Some(0), None, None);
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    tiers.open();
    passes(context, 2);
    assert_eq!(tiers.task(1).task.state, "working");
    let session = tiers.assignee(1);
    let window = context.host.last(&session).unwrap().pane;
    context.adapter.answer(&session, "Recorded");
    passes(context, 2);
    assert!(closed(context, &window), "closed with its result");
    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .unwrap();
    (tiers, session)
}

/// T-`number`'s follow-up from the human: it goes to the session that did T-1.
fn follow_up(context: &Context, tiers: &Tiers<'_>, body: &str) {
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "human".to_owned(),
            after: Some(1),
            body: body.to_owned(),
            ..NewTask::default()
        },
    );
}

fn passes(context: &Context, count: usize) {
    for _ in 0..count {
        context.pass().unwrap();
    }
}

/// Whether the program of the last window on the session's conversation is proven gone.
fn proven(context: &Context, tiers: &Tiers<'_>, session: &str) -> bool {
    let current = context
        .ledger
        .borrow()
        .current_conversation(tiers.id(session))
        .unwrap()
        .expect("a conversation");
    context.ledger.borrow().program_gone(current.id).unwrap()
}

/// The window of T-2, a follow-up that goes on in T-1's conversation, is not answered by
/// its open's deadline: T-2 fails as one the host refused does, the window is killed, and
/// the proof of the window before it is not kept again, for this one may have run on the
/// conversation (the kluris window ran there for fourteen minutes).
#[test]
fn an_open_the_host_did_not_answer_in_time_is_killed_and_keeps_no_proof_of_the_window_before() {
    let context = Context::new();
    let (tiers, session) = session_done(&context);
    let native = native_of(&context, tiers.id(&session)).expect("bound");
    assert!(
        proven(&context, &tiers, &session),
        "T-1's window proven gone"
    );

    context.host.unanswered.set(true);
    follow_up(&context, &tiers, "Write the docs");
    passes(&context, 2);
    assert!(!context.host.unanswered.get(), "the open was asked");
    assert_eq!(tiers.task(2).task.state, "failed");
    let given_up = context.host.last(&session).unwrap().pane;
    assert_eq!(
        context.adapter.prepared().last().unwrap()["resume"],
        serde_json::Value::String(native),
        "it opened on T-1's conversation"
    );
    assert!(
        context.host.killed().contains(&given_up),
        "the window given up on is killed: {:?}",
        context.host.killed()
    );
    assert!(
        !proven(&context, &tiers, &session),
        "an open the host did not answer proves nothing"
    );
}

/// The host refuses the kill of the window given up on: it is asked again at each pass,
/// though nothing more is for the session, and the next window of the session waits, its
/// task queued; once the host ends the window, the next opens, once.
#[test]
fn the_next_window_of_a_session_waits_until_the_host_has_ended_the_one_given_up_on() {
    let context = Context::new();
    let (tiers, session) = session_done(&context);
    context.host.unanswered.set(true);
    context.host.refuse_kills.set(true);
    follow_up(&context, &tiers, "Write the docs");
    passes(&context, 2);
    assert_eq!(tiers.task(2).task.state, "failed");
    let given_up = context.host.last(&session).unwrap().pane;
    let opened = context.host.opened().len();
    let asked = || {
        context
            .host
            .killed()
            .iter()
            .filter(|pane| **pane == given_up)
            .count()
    };
    let before = asked();
    passes(&context, 2);
    assert_eq!(asked(), before + 2, "asked again at each pass");

    follow_up(&context, &tiers, "Write the docs, again");
    passes(&context, 3);
    assert_eq!(
        context.host.opened().len(),
        opened,
        "no window opens while the one given up on may run"
    );
    assert_eq!(tiers.task(3).task.state, "queued");

    context.host.refuse_kills.set(false);
    passes(&context, 3);
    assert_eq!(
        context.host.opened().len(),
        opened + 1,
        "the next window opens once"
    );
    assert_eq!(tiers.task(3).task.state, "working");
    assert_ne!(context.host.last(&session).unwrap().pane, given_up);
}
