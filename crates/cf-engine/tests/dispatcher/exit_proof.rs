//! The host's proof that a window's program is gone, kept across a restart (T-541): the
//! ledger keeps it on the conversation the window ran on (`program_gone`, migration 0024),
//! so a follow-up of another task goes on in that conversation after the app's restart as
//! it did before it. What was not proven stays so: a window whose program the host could
//! not end, a window open when the daemon stopped (the window's quit ends it after the
//! daemon has gone and tells no one), and a window that opened on the conversation after
//! the one whose late word proved it gone. Node's dispatcher had none of it, so none of
//! these is held to a Node trace.

use cf_engine::seams::EngineError;
use cf_engine::testing::{Answer, Context, Gate, Restarted};
use cf_harness::contract::Pane;
use cf_ledger::{NewTask, ProjectView};

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

/// The program gone of the conversation the participant `session` has now.
fn current_proven(context: &Context, tiers: &Tiers<'_>, session: &str) -> (i64, bool) {
    let current = context
        .ledger
        .borrow()
        .current_conversation(tiers.id(session))
        .unwrap()
        .expect("a conversation now");
    let gone = context.ledger.borrow().program_gone(current.id).unwrap();
    (current.id, gone)
}

/// Calliope's P3-1 on 0181cd9a (T-573): the human deletes a session whose idle window
/// the human kept open, and before that window's exit comes, a follow-up of another
/// task brings the session back in a window of its own, on a new conversation. The
/// first window's exit proves its own program gone, not that of the window open on the
/// new conversation, which may still write to it when the daemon stops.
#[test]
fn the_exit_of_a_deleted_sessions_window_proves_nothing_of_the_window_that_brought_it_back() {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let (tiers, session) = session_at_work(&context);
    // The human keeps the window open; T-1 has its result and is accepted.
    context.open_window(tiers.project.id, &session).unwrap();
    context.adapter.answer(&session, "Recorded");
    passes(&context, 2);
    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .unwrap();
    let first = context.host.last(&session).unwrap().pane;
    let (old, _) = current_proven(&context, &tiers, &session);
    // The human deletes the session; its window's kill is held.
    let kill = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((first.generation, kill.clone()));
    let ending = context.begin_end_session(tiers.project.id, &session);
    context.settle();
    // A follow-up of another task brings it back, in a new window on a new conversation.
    follow_up(&context, &tiers, 1, "Write the docs");
    passes(&context, 3);
    let second = context.host.last(&session).unwrap().pane;
    assert_ne!(second, first, "a window of its own");
    assert_eq!(
        resumed(&context),
        serde_json::Value::Null,
        "a new conversation"
    );
    let (new, _) = current_proven(&context, &tiers, &session);
    assert_ne!(new, old);
    // Then the first window's kill is taken, and its exit comes.
    kill.open();
    context.finish(ending).unwrap();
    context.settle();
    assert!(closed(&context, &first));
    assert_eq!(
        current_proven(&context, &tiers, &session),
        (new, false),
        "the window on it still runs"
    );
}

/// The human keeps the window of the session that did T-1 open, T-1 is done (and accepted
/// where `accepted`), and the
/// human deletes the session, the window's kill held: the session, its first window's
/// pane and conversation, and the kill and the session's end, to let go.
fn deleted_with_its_window_held(context: &Context, accepted: bool) -> Deleted<'_> {
    context.host.exits_showing(Some(0), None, None);
    let (tiers, session) = session_at_work(context);
    context.open_window(tiers.project.id, &session).unwrap();
    context.adapter.answer(&session, "Recorded");
    passes(context, 2);
    if accepted {
        context
            .ledger
            .borrow_mut()
            .accept_task(tiers.project.id, 1, "chief")
            .unwrap();
    }
    let first = context.host.last(&session).unwrap().pane;
    let (old, _) = current_proven(context, &tiers, &session);
    let kill = Gate::default();
    *context.host.hold_kill.borrow_mut() = Some((first.generation, kill.clone()));
    let ending = context.begin_end_session(tiers.project.id, &session);
    context.settle();
    (tiers, session, first, old, kill, ending)
}

/// What [`deleted_with_its_window_held`] leaves.
type Deleted<'a> = (
    Tiers<'a>,
    String,
    Pane,
    i64,
    Gate,
    Answer<Result<ProjectView, EngineError>>,
);

/// Calliope's P3-1 on 46664f52 (T-621): the session brought back for another task's
/// follow-up opens on a new conversation, and that window ends, its program proven gone,
/// while the deleted session's first window still waits for its kill. The first window
/// writes to the old conversation alone: the proof is kept on the new one.
#[test]
fn a_brought_back_windows_end_on_its_new_conversation_is_kept_while_the_deleted_sessions_window_waits_for_its_kill(
) {
    let context = Context::new();
    let (tiers, session, first, old, kill, ending) = deleted_with_its_window_held(&context, true);
    follow_up(&context, &tiers, 1, "Write the docs");
    passes(&context, 3);
    let second = context.host.last(&session).unwrap().pane;
    assert_ne!(second, first, "a window of its own");
    let (new, _) = current_proven(&context, &tiers, &session);
    assert_ne!(new, old, "a new conversation");
    // The window that brought it back ends by itself; the first one's kill waits still.
    context.exit(&session);
    assert_eq!(
        current_proven(&context, &tiers, &session),
        (new, true),
        "the window that wrote to it is gone"
    );
    kill.open();
    context.finish(ending).unwrap();
    context.settle();
    assert!(closed(&context, &first));
}

/// Calliope's P3-3 on 46664f52 (T-621), the shape of the skip on the exit's own path: the
/// session's task is reopened while the deleted session's first window waits for its
/// kill, and with no proof of that window's end, the window that brings the session back
/// goes on in the same conversation. That window ends proven gone first: the first
/// window, still running on the conversation, may write to it, so nothing is proven of
/// it until its own end comes.
#[test]
fn a_brought_back_windows_end_proves_nothing_of_the_conversation_the_deleted_sessions_window_still_runs_on(
) {
    let context = Context::new();
    let (tiers, session, first, old, kill, ending) = deleted_with_its_window_held(&context, false);
    context
        .ledger
        .borrow_mut()
        .reopen_task(tiers.project.id, 1, "chief", "Handle empty input too")
        .unwrap();
    passes(&context, 3);
    let second = context.host.last(&session).unwrap().pane;
    assert_ne!(second, first, "a window of its own");
    assert_eq!(
        current_proven(&context, &tiers, &session).0,
        old,
        "the same conversation"
    );
    // The window that brought it back ends by itself; the first one's kill waits still.
    context.exit(&session);
    assert_eq!(
        current_proven(&context, &tiers, &session),
        (old, false),
        "the first window runs on it"
    );
    kill.open();
    context.finish(ending).unwrap();
    context.settle();
    assert!(closed(&context, &first));
    assert_eq!(
        current_proven(&context, &tiers, &session),
        (old, true),
        "both windows on it are gone"
    );
}

/// The host's late word on a window the engine's own close settled comes while the
/// window that goes on with its conversation is still opening: that window's program may
/// write to the conversation as soon as its open is answered, so the word proves nothing
/// of it (`Opening::conversation`, T-646).
#[test]
fn a_late_word_while_a_window_opens_on_the_conversation_proves_nothing_of_it() {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let (tiers, session) = session_at_work(&context);
    let window = context.host.last(&session).unwrap().pane;
    let (conversation, _) = current_proven(&context, &tiers, &session);
    // The human's Close settles the window itself; the host's word on it is held.
    context.host.hold_exits.set(true);
    context.close_project(tiers.project.id).unwrap();
    assert!(closed(&context, &window));
    context.host.hold_exits.set(false);
    // The Resume opens the session's window again on its conversation; its open is held.
    let opening = context.host.open_holds.hold(|_| true);
    context.resume_project(tiers.project.id).unwrap();
    passes(&context, 2);
    assert_eq!(
        context.host.last(&session).unwrap().pane,
        window,
        "still opening"
    );
    let (host, ended) = (std::rc::Rc::clone(&context.host), window.clone());
    context.run(async move { host.exited(ended).await });
    assert!(
        !context.ledger.borrow().program_gone(conversation).unwrap(),
        "the window opening on it may write to it"
    );
    opening.open();
    context.settle();
    assert_ne!(context.host.last(&session).unwrap().pane, window, "opened");
    assert!(!context.ledger.borrow().program_gone(conversation).unwrap());
}

/// Calliope's P3-2 on 0181cd9a (T-573): the host's word on a deleted session's window,
/// which the engine's own close settled, comes after a follow-up brought the session
/// back and before its window opens: the record made again carries the pane that word
/// is for, and the follow-up goes on in the conversation.
#[test]
fn the_hosts_word_after_a_deleted_session_is_brought_back_and_before_its_window_opens_is_kept() {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let (tiers, session) = session_at_work(&context);
    let native = native_of(&context, tiers.id(&session)).expect("bound");
    context.open_window(tiers.project.id, &session).unwrap();
    context.adapter.answer(&session, "Recorded");
    passes(&context, 2);
    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .unwrap();
    let window = context.host.last(&session).unwrap().pane;
    // The human's Close settles the window itself, the host's word held; the human
    // deletes the session, and a follow-up brings it back while the project is closed.
    context.host.hold_exits.set(true);
    context.close_project(tiers.project.id).unwrap();
    assert!(closed(&context, &window));
    context.end_session(tiers.project.id, &session).unwrap();
    follow_up(&context, &tiers, 1, "Write the docs");
    passes(&context, 2);
    assert!(tiers.participant(&session).is_some(), "back on the board");
    assert_eq!(
        context.host.last(&session).unwrap().pane,
        window,
        "no window yet"
    );
    // The word comes now, and then the human's Resume.
    context.host.hold_exits.set(false);
    let host = std::rc::Rc::clone(&context.host);
    context.run(async move { host.exited(window).await });
    context.resume_project(tiers.project.id).unwrap();
    passes(&context, 3);
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(
        resumed(&context),
        serde_json::Value::String(native),
        "T-1's conversation"
    );
    assert_eq!(told_new(&tiers, &session), Vec::<String>::new());
}

/// Calliope's sibling of P3-1 on 0181cd9a (T-573): a window that opens on a new
/// conversation, whose session the human deletes while it opens, and which exits before
/// its open is answered, wrote to no conversation the ledger holds. Its end proves
/// nothing of the conversation before, whose last window's end said nothing.
#[test]
fn a_window_forgotten_as_it_opened_on_a_new_conversation_proves_nothing_of_the_one_before() {
    let context = Context::new();
    let (tiers, session) = session_at_work(&context);
    // T-1's window closes with its result, and the host says nothing of how it ended.
    context.adapter.answer(&session, "Recorded");
    passes(&context, 2);
    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .unwrap();
    let (old, before) = current_proven(&context, &tiers, &session);
    assert!(!before, "no word on the window before");
    // The harness holds nothing of the conversation: the human's Open starts a new one.
    // Its open is held, and its window exits before the open is answered.
    context.adapter.holds_nothing(&session);
    let opening = context
        .host
        .open_holds
        .hold_instead(|_| true, session.clone());
    context.open_window(tiers.project.id, &session).unwrap();
    context.settle();
    assert_eq!(
        resumed(&context),
        serde_json::Value::Null,
        "a new conversation"
    );
    let ending = context.begin_end_session(tiers.project.id, &session);
    context.settle();
    context.host.exits_showing(Some(0), None, None);
    opening.open();
    context.finish(ending).unwrap();
    context.settle();
    assert!(tiers.participant(&session).is_none(), "off the board");
    assert!(
        !context.ledger.borrow().program_gone(old).unwrap(),
        "the window that ended never wrote to it"
    );
}

/// A window the human switched to another conversation (/clear) writes to that one: its
/// end proven is kept there, and the next task of its session goes on in it after a
/// restart.
#[test]
fn the_end_of_a_window_the_human_switched_is_kept_on_the_conversation_it_switched_to() {
    let context = Context::new();
    context.host.exits_showing(Some(0), None, None);
    let (tiers, session) = session_at_work(&context);
    let (old, _) = current_proven(&context, &tiers, &session);
    context.adapter.switch_to(&session, "native-cleared");
    passes(&context, 1);
    let (cleared, _) = current_proven(&context, &tiers, &session);
    assert_ne!(cleared, old, "the conversation it shows");
    // The chief calls T-1 off; the window stops, and closes with nothing in hand.
    let window = context.host.last(&session).unwrap().pane;
    context
        .ledger
        .borrow_mut()
        .cancel_task(tiers.project.id, 1, "chief")
        .unwrap();
    context.adapter.answer(&session, "Stopped");
    passes(&context, 3);
    assert!(closed(&context, &window));
    assert_eq!(current_proven(&context, &tiers, &session), (cleared, true));

    let after = restart(&context, &tiers);
    context.create_task(
        tiers.project.id,
        NewTask {
            from: "human".to_owned(),
            to: Some(session.clone()),
            body: "Write the docs".to_owned(),
            ..NewTask::default()
        },
    );
    again(&after, 3);
    assert_eq!(tiers.task(2).task.state, "working");
    assert_eq!(
        resumed(&context),
        serde_json::Value::String("native-cleared".to_owned()),
        "the conversation it switched to"
    );
}
