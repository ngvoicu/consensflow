//! The host's proof that a window's program is gone, kept across a restart (T-541): the
//! ledger keeps it on the conversation the window ran on (`program_gone`, migration 0023),
//! so a follow-up of another task goes on in that conversation after the app's restart as
//! it did before it. What was not proven stays so: a window whose program the host could
//! not end, a window open when the daemon stopped (the window's quit ends it after the
//! daemon has gone and tells no one), and a window that opened on the conversation after
//! the one whose late word proved it gone. Node's dispatcher had none of it, so none of
//! these is held to a Node trace.

use cf_engine::testing::{Context, Restarted};
use cf_ledger::NewTask;

use crate::fixtures::{closed, native_of, Tiers};

/// A project of zeus and diana, T-1 open for their tier and at work in the window of a
/// session of it, which is named.
fn session_at_work(context: &Context) -> (Tiers<'_>, String) {
    let tiers = Tiers::new(context, &["zeus", "diana"]);
    tiers.open();
    passes(context, 2);
    assert_eq!(tiers.task(1).task.state, "working");
    let session = tiers.assignee(1);
    (tiers, session)
}

/// T-`number`, from the human, a follow-up of T-`of`: it goes to the session that did it.
fn follow_up(context: &Context, tiers: &Tiers<'_>, of: i64, body: &str) {
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "human".to_owned(),
            after: Some(of),
            body: body.to_owned(),
            ..NewTask::default()
        },
    );
}

/// Passes enough for what is begun to be done.
fn passes(context: &Context, count: usize) {
    for _ in 0..count {
        context.pass().unwrap();
    }
}

/// The app quits and starts again: an engine made again on the same ledger, the project
/// resumed, and passes enough for what waits to go. What follows is the new engine's.
fn restart<'a>(context: &'a Context, tiers: &Tiers<'_>) -> Restarted<'a> {
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(tiers.project.id).unwrap();
    again(&after, 4);
    after
}

/// Passes of the engine a restart made.
fn again(after: &Restarted<'_>, count: usize) {
    for _ in 0..count {
        after.pass().unwrap();
    }
}

/// The conversation the adapter's last launch goes on with, none for a new one.
fn resumed(context: &Context) -> serde_json::Value {
    context.adapter.prepared().last().unwrap()["resume"].clone()
}

/// What the human was told of T-2 going to `session` in a new conversation.
fn told_new(tiers: &Tiers<'_>, session: &str) -> Vec<String> {
    let words = format!("T-2 goes to @{session} in a new conversation");
    tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.starts_with(&words))
        .collect()
}

/// T-530's shape (m-3891): T-1's window closed with its program proven gone before the app
/// restarted, and its follow-up, another task, goes on in the conversation T-1 built.
#[test]
fn a_follow_up_goes_on_in_its_sessions_conversation_after_a_restart_where_the_window_before_closed_proven_gone(
) {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let (tiers, session) = session_at_work(&context);
    let window = context.host.last(&session).unwrap().pane;
    let native = native_of(&context, tiers.id(&session)).expect("bound");
    context.adapter.answer(&session, "Recorded");
    passes(&context, 2);
    assert!(closed(&context, &window), "closed with its result");

    let after = restart(&context, &tiers);
    follow_up(&context, &tiers, 1, "Write the docs");
    again(&after, 3);
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(
        resumed(&context),
        serde_json::Value::String(native),
        "T-1's conversation"
    );
    assert_eq!(told_new(&tiers, &session), Vec::<String>::new());
}

/// The converse: T-1's window closed and the host could not prove its program gone, which
/// may still write to the conversation; the restart makes no proof of it.
#[test]
fn a_follow_up_goes_to_a_new_conversation_after_a_restart_where_the_window_before_closed_with_its_program_alive(
) {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    context.host.exits_with_a_survivor(4242);
    let (tiers, session) = session_at_work(&context);
    let window = context.host.last(&session).unwrap().pane;
    context.adapter.answer(&session, "Recorded");
    passes(&context, 2);
    assert!(closed(&context, &window), "closed with its result");

    let after = restart(&context, &tiers);
    follow_up(&context, &tiers, 1, "Write the docs");
    again(&after, 3);
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(
        resumed(&context),
        serde_json::Value::Null,
        "a new conversation"
    );
    assert_eq!(
        told_new(&tiers, &session).len(),
        1,
        "told once: {:?}",
        told_new(&tiers, &session)
    );
}

/// A window open when the daemon stopped: the app's window quits after the daemon has gone
/// and tells no one how it ended (`cf_panes::headless::reap_all`), so nothing proves its
/// program gone. The proof of the window before it on the same conversation holds no more
/// once it opened, and the next task goes to a new conversation.
#[test]
fn a_window_open_when_the_daemon_stopped_is_no_proof_though_the_window_before_it_was_proven_gone() {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let (tiers, session) = session_at_work(&context);
    let native = native_of(&context, tiers.id(&session)).expect("bound");
    context.adapter.answer(&session, "Recorded");
    passes(&context, 2);
    // The human opens the session's window again, on its conversation, and keeps it open.
    context.open_window(tiers.project.id, &session).unwrap();
    passes(&context, 1);
    assert_eq!(resumed(&context), serde_json::Value::String(native));
    follow_up(&context, &tiers, 1, "Write the docs");
    passes(&context, 2);
    assert_eq!(
        tiers.task(2).task.state,
        "queued",
        "the human keeps the window"
    );

    restart(&context, &tiers);
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(
        resumed(&context),
        serde_json::Value::Null,
        "a new conversation"
    );
    assert_eq!(
        told_new(&tiers, &session).len(),
        1,
        "told once: {:?}",
        told_new(&tiers, &session)
    );
}

/// The host's word on a window the engine's own close settled (a Close's, which does not
/// wait for it) can come after a window of the same session has opened on the
/// conversation: it proves the program of the window that closed gone, not that of the
/// one open, which may still write to it when the daemon stops.
#[test]
fn a_late_word_that_a_closed_windows_program_is_gone_proves_nothing_of_the_window_open_since() {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let (tiers, session) = session_at_work(&context);
    let window = context.host.last(&session).unwrap().pane;
    // The human's Close settles the window itself; the host's word on it is held.
    context.host.hold_exits.set(true);
    context.close_project(tiers.project.id).unwrap();
    assert!(closed(&context, &window));
    context.host.hold_exits.set(false);
    // The Resume opens the session's window again on its conversation, for T-1; then
    // the word on the first comes.
    context.resume_project(tiers.project.id).unwrap();
    passes(&context, 2);
    let open = context.host.last(&session).unwrap().pane;
    assert_ne!(open, window);
    assert_eq!(tiers.task(1).task.state, "working");
    let host = std::rc::Rc::clone(&context.host);
    context.run(async move { host.exited(window).await });
    // The human keeps the window open, and the chief calls T-1 off; T-2, for the same
    // session, waits for that window.
    context.open_window(tiers.project.id, &session).unwrap();
    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "chief")
        .unwrap();
    context.adapter.answer(&session, "Stopped");
    passes(&context, 2);
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "human".to_owned(),
            to: Some(session.clone()),
            body: "Write the docs".to_owned(),
            ..NewTask::default()
        },
    );
    passes(&context, 2);
    assert!(!closed(&context, &open), "the human keeps it open");
    assert_eq!(
        tiers.task(2).task.state,
        "queued",
        "the human keeps the window"
    );

    restart(&context, &tiers);
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(
        resumed(&context),
        serde_json::Value::Null,
        "a new conversation"
    );
    assert_eq!(
        told_new(&tiers, &session).len(),
        1,
        "told once: {:?}",
        told_new(&tiers, &session)
    );
}

/// The host's word on a window the engine's own close settled (a Close's) can come after
/// the human has deleted its session: it is kept on the session's conversation all the
/// same, and a follow-up that brings the session back goes on in it, after a restart too.
#[test]
fn the_hosts_late_word_on_a_deleted_sessions_window_is_kept_for_the_follow_up_that_brings_it_back()
{
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let (tiers, session) = session_at_work(&context);
    let native = native_of(&context, tiers.id(&session)).expect("bound");
    // The human keeps the window open; T-1 has its result and is accepted.
    context.open_window(tiers.project.id, &session).unwrap();
    context.adapter.answer(&session, "Recorded");
    passes(&context, 2);
    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .unwrap();
    let window = context.host.last(&session).unwrap().pane;
    assert!(!closed(&context, &window), "the human keeps it open");
    // The human's Close settles the window itself, the host's word held; then the
    // human deletes the session, and the word comes.
    context.host.hold_exits.set(true);
    context.close_project(tiers.project.id).unwrap();
    assert!(closed(&context, &window));
    context.end_session(tiers.project.id, &session).unwrap();
    assert!(tiers.participant(&session).is_none(), "off the board");
    context.host.hold_exits.set(false);
    let host = std::rc::Rc::clone(&context.host);
    context.run(async move { host.exited(window).await });

    let after = restart(&context, &tiers);
    follow_up(&context, &tiers, 1, "Write the docs");
    again(&after, 3);
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(tiers.assignee(2), session, "brought back");
    assert_eq!(
        resumed(&context),
        serde_json::Value::String(native),
        "T-1's conversation"
    );
    assert_eq!(told_new(&tiers, &session), Vec::<String>::new());
}

/// A window the pane host refuses to open ran nothing on the conversation: the proof of
/// the window before it holds. The follow-up it was for fails, and the next one goes on in
/// the conversation, after a restart too.
#[test]
fn an_open_the_host_refused_keeps_the_proof_of_the_window_before() {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let (tiers, session) = session_at_work(&context);
    let native = native_of(&context, tiers.id(&session)).expect("bound");
    context.adapter.answer(&session, "Recorded");
    passes(&context, 2);
    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .unwrap();
    // The window of T-2, a follow-up, is refused, and T-2 fails.
    context.host.refuse.set(true);
    follow_up(&context, &tiers, 1, "Write the docs");
    passes(&context, 2);
    assert!(!context.host.refuse.get(), "the open was asked and refused");
    assert_eq!(tiers.task(2).task.state, "failed");

    let after = restart(&context, &tiers);
    follow_up(&context, &tiers, 1, "Write the docs, again");
    again(&after, 3);
    assert_eq!(tiers.task(3).task.state, "working");
    assert_eq!(tiers.assignee(3), session);
    assert_eq!(
        resumed(&context),
        serde_json::Value::String(native),
        "T-1's conversation"
    );
    let told: Vec<String> = tiers
        .notes("human")
        .into_iter()
        .filter(|note| note.contains("in a new conversation"))
        .collect();
    assert_eq!(told, Vec::<String>::new());
}
