//! A window that went with a paste on its way into it: its program ended (a crash,
//! a close) after its harness took the paste and before a look confirmed it, or
//! while the paste was handed over. Where the harness's record shows it, the
//! conversation that goes on holds it: it is confirmed, and never pasted again, as a
//! start settles what was in flight. Where it does not, it goes again, as it always
//! did. Found on 2026-10-16 in the delivery code, after the field reports of the day
//! before (windows that closed in the middle of their tasks; a result a chief read
//! twice); `regressions/a_result_pasted_again_after_its_window_went` holds it on
//! the rig. The record is read with no window open, and what the human did meanwhile
//! stands (the reviews of 2026-10-16): a message cancelled is left so, and the exit
//! closes the project all the same; a chief switched to, or a window opened again,
//! is not the window that went, and its exit settles nothing more. Held to no Node
//! trace: Node's dispatcher pasted it again either way, and read nothing first.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::testing::{Answer, Context, Gate};
use cf_engine::SwitchWhen;
use cf_harness::records::Role;

use crate::chiefs::{chief_of, handoffs_of, to, with_codex};
use crate::fixtures::Tiers;

/// How many messages the window of `handle` that is open now was given that say
/// `words`.
fn given_saying(context: &Context, handle: &str, words: &str) -> usize {
    context
        .adapter
        .agent(handle)
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains(words))
        .count()
}

/// The chief's window ends, as its program does when it crashes or is closed.
fn the_chiefs_window_ends(context: &Context) {
    let host = Rc::clone(&context.host);
    context.run(async move { host.exit("chief").await });
}

/// The chief's window closes as its harness takes the paste, before the host answers.
fn the_chiefs_window_closes_as_its_harness_takes_the_paste(context: &Context) {
    let host = Rc::clone(&context.host);
    *context.adapter.deliver.borrow_mut() = Some(Rc::new(move |taking| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            let outcome = taking();
            host.exit("chief").await;
            Ok(outcome)
        })
    }));
}

/// The chief's window ends while zeus's question about T-1 is on its way into it, and
/// the look at its record, with no window open, waits until the test lets it go: the
/// project, the question, the gate and where the exit's answer will be.
fn the_chiefs_window_ends_as_its_record_is_read(context: &Context) -> (i64, i64, Gate, Answer<()>) {
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let question = context.ask(project.id, "zeus", "chief", 1, "Which format?");
    context.adapter.answer("zeus", "I asked");
    context.pass().unwrap();
    // Its harness took it and its record has it; no look has confirmed it yet.
    assert_eq!(context.message(question.id).state, "delivering");
    let session = context.adapter.agent("chief").native;
    let looking = context
        .adapter
        .record_holds
        .hold(move |asked| asked[0] == session.as_str());
    let host = Rc::clone(&context.host);
    let exiting = context
        .executor
        .start(async move { host.exit("chief").await });
    context.settle();
    (project.id, question.id, looking, exiting)
}

#[test]
fn a_chiefs_window_that_ends_closes_the_project_whatever_became_of_its_paste_while_its_record_was_read(
) {
    let context = Context::new();
    let (project, question, looking, exiting) =
        the_chiefs_window_ends_as_its_record_is_read(&context);
    let zeus = context.host.last("zeus").unwrap().pane;
    // The human cancels T-1 meanwhile: the question is withdrawn with it.
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    assert_eq!(context.message(question).state, "cancelled");
    looking.open();
    context.settle();
    exiting.take().unwrap();
    assert_eq!(
        context.message(question).state,
        "cancelled",
        "left as it is"
    );
    assert_eq!(
        context.project(project).state,
        "suspended",
        "the chief's exit closes the project"
    );
    assert!(context.host.killed().contains(&zeus), "and its windows");
}

/// A review of 2026-10-16: the human closed the project as the chief's window ended
/// with a paste on its way, whose record was being read, and resumed it at once. The
/// Resume read whether the chief's turn had been cut, the chief's window not yet
/// launched, as the old window's exit ended: the exit took the project for one its
/// chief had left by itself, and suspended it, and the Resume opened the chief all the
/// same, which ran in a project that was suspended.
#[test]
fn a_chiefs_window_that_ends_leaves_the_project_the_human_closed_and_resumed_while_its_record_was_read(
) {
    let context = Context::new();
    let (project, _, looking, exiting) = the_chiefs_window_ends_as_its_record_is_read(&context);
    context.close_project(project).unwrap();
    let session = context.adapter.agent("chief").native;
    let cutting = context
        .adapter
        .cut_holds
        .hold(move |asked| asked[0] == session.as_str());
    context.resume_project(project).unwrap();
    looking.open();
    context.settle();
    exiting.take().unwrap();
    cutting.open();
    context.settle();
    context.pass().unwrap();
    assert_eq!(
        context.project(project).state,
        "open",
        "the old window's exit closes nothing of the project the human resumed"
    );
    let chief = context.host.last("chief").unwrap().pane;
    assert!(
        !context.host.killed().contains(&chief),
        "the chief's window the Resume opened runs"
    );
    let opened: Vec<String> = context
        .host
        .opened()
        .into_iter()
        .map(|open| open.pane.id)
        .collect();
    assert_eq!(
        opened,
        ["p1-chief", "p1-zeus", "p1-zeus", "p1-chief"],
        "the chief's, zeus's, and each again, once: the Resume brings back T-1, which the \
         Close paused, and the chief's launch waits on the read of its record"
    );
}

/// A member's launch's first message, whose window goes before a look confirmed it,
/// fails with the launch whatever its record shows, and is never pasted again: no
/// conversation is given it twice (the reviews of 2026-10-16 asked). Only the chief's
/// first messages go back to wait for its next window, so only those are settled by
/// the record.
#[test]
fn fails_the_brief_of_a_members_launch_whose_window_goes_whatever_its_record_shows() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    let brief = context.ledger.borrow().in_flight().unwrap().remove(0);
    assert_eq!(brief.kind, "task");
    assert_eq!(
        given_saying(&context, "zeus", "Write the parser"),
        1,
        "its record shows it"
    );
    let (host, session) = (Rc::clone(&context.host), brief.recipient.clone());
    context.run(async move { host.exit(&session).await });
    assert_eq!(context.message(brief.id).state, "failed");
    assert_eq!(tiers.task(1).task.state, "failed");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let launches = context
        .adapter
        .prepared()
        .iter()
        .filter(|launch| {
            launch["message"]
                .as_str()
                .is_some_and(|message| message.contains("Write the parser"))
        })
        .count();
    assert_eq!(launches, 1, "never given again");
}

/// The human cancels T-1 while the old window's record is read, which withdraws the
/// question on its way, and a pass launches the chief's window again for what waits
/// for it, its harness asked: the old window's exit is none of the window on its way.
#[test]
fn a_chiefs_window_that_ends_leaves_the_project_to_the_window_a_pass_launches_while_its_record_is_read(
) {
    let context = Context::new();
    let (project, question, looking, exiting) =
        the_chiefs_window_ends_as_its_record_is_read(&context);
    context
        .ledger
        .borrow_mut()
        .cancel_task(project, 1, "human")
        .unwrap();
    assert_eq!(context.message(question).state, "cancelled");
    context.note(project, "zeus", "chief", "And another thing");
    let preparing = context.adapter.prepare_holds.hold(|_| true);
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.pane(context.id(project, "chief")),
        None,
        "its harness is asked"
    );
    looking.open();
    context.settle();
    exiting.take().unwrap();
    assert_eq!(
        context.project(project).state,
        "open",
        "the old window's exit closes nothing of the window on its way"
    );
    preparing.open();
    context.settle();
    let chief = context.host.last("chief").unwrap().pane;
    assert_eq!(
        context.dispatcher.pane(context.id(project, "chief")),
        Some(chief)
    );
}

/// A review of 2026-10-16: the Codex chief the human switched to took its handoff, which
/// its record shows on the conversation its window began, and its window ended before a
/// look confirmed it. The handoff went back to its queue, and the Resume gave it again,
/// into the same conversation.
#[test]
fn confirms_a_chiefs_handoff_its_harness_recorded_when_its_window_ends_before_a_look_and_never_gives_it_again(
) {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]).id;
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    context
        .switch_chief(project, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    let handoff = handoffs_of(&context, project).remove(0);
    assert_eq!(context.message(handoff.id).state, "delivering");
    let session = context.codex.agent("chief").native;

    // It crashes, as the host says.
    context
        .host
        .exits_showing(Some(1), None, Some(&["panicked"]));
    the_chiefs_window_ends(&context);
    assert_eq!(context.project(project).state, "suspended");
    let human = context.id(project, "human");
    let told: Vec<String> = context
        .inbox(human)
        .into_iter()
        .map(|message| message.body)
        .filter(|body| body.contains("resume it to try again"))
        .collect();
    assert_eq!(
        told,
        Vec::<String>::new(),
        "it came up: no launch that failed"
    );
    let confirmed = context.message(handoff.id);
    assert_eq!(
        (confirmed.state.as_str(), confirmed.attempts),
        ("delivered", 1),
        "its record shows it: the conversation holds it"
    );
    context.resume_project(project).unwrap();
    context.pass().unwrap();
    let launch = context.codex.prepared().pop().unwrap();
    assert_eq!(launch["resume"], session.as_str(), "the same conversation");
    assert!(
        !launch["message"]
            .as_str()
            .is_some_and(|message| message.contains("You are the chief now")),
        "not given the handoff again: {launch}"
    );
}

#[test]
fn a_chiefs_window_that_ends_leaves_the_chief_the_human_switched_to_while_its_record_was_read() {
    let context = with_codex();
    let (project, _, looking, exiting) = the_chiefs_window_ends_as_its_record_is_read(&context);
    let zeus = context.host.last("zeus").unwrap().pane;
    let switched =
        context.begin_switch_chief(project, to("codex", "astraeus"), SwitchWhen::Now, false);
    context.settle();
    switched.take().unwrap().unwrap();
    let chief = context.host.last("chief").unwrap().pane;
    assert_eq!(
        chief_of(&context, project).harness.as_deref(),
        Some("codex"),
        "the new chief's window is open"
    );
    looking.open();
    context.settle();
    exiting.take().unwrap();
    assert_eq!(
        context.project(project).state,
        "open",
        "the old window's exit closes nothing of the new chief's"
    );
    let killed = context.host.killed();
    assert!(!killed.contains(&chief) && !killed.contains(&zeus));
}

#[test]
fn a_chiefs_window_that_ends_leaves_the_chief_the_human_switched_to_while_its_record_was_read_whose_window_is_not_up(
) {
    let context = with_codex();
    let (project, _, looking, exiting) = the_chiefs_window_ends_as_its_record_is_read(&context);
    let (zeus, old) = (
        context.host.last("zeus").unwrap().pane,
        context.host.last("chief").unwrap().pane,
    );
    // The new chief's window does not come up: it is tried again later, and no window
    // is open for the chief meanwhile, but its conversation is another.
    *context.codex.prepare.borrow_mut() =
        Some(Rc::new(|_| Err("codex is not signed in".to_owned())));
    let switched =
        context.begin_switch_chief(project, to("codex", "astraeus"), SwitchWhen::Now, false);
    context.settle();
    switched.take().unwrap().unwrap();
    assert_eq!(
        (
            chief_of(&context, project).harness.as_deref(),
            context.host.last("chief").unwrap().pane
        ),
        (Some("codex"), old),
        "switched, with no window of the new chief's"
    );
    looking.open();
    context.settle();
    exiting.take().unwrap();
    assert_eq!(
        context.project(project).state,
        "open",
        "the old window's exit closes nothing of the new chief's"
    );
    assert!(!context.host.killed().contains(&zeus));
}

#[test]
fn a_sessions_window_that_ends_leaves_its_task_to_the_window_the_human_opened_while_its_record_was_read(
) {
    let context = Context::new();
    let ended = a_sessions_window_ends_as_its_record_is_read(&context);
    // The human opens the session's window again, on its conversation.
    context.open_window(ended.project, &ended.session).unwrap();
    let reopened = context.host.last(&ended.session).unwrap().pane;
    assert_ne!(reopened, ended.old, "a window of the session's again");
    ended.looking.open();
    context.settle();
    ended.exiting.take().unwrap();
    assert_ne!(
        context.task(ended.project, 1).task.state,
        "paused",
        "the old window's exit stalls nothing of the new window's"
    );
    assert!(!context.host.killed().contains(&reopened));
}

/// Astra's sequence (the review of 2026-10-16): the window the human opens again is
/// still being prepared, its harness asked, when the old one's record is read.
#[test]
fn a_sessions_window_that_ends_leaves_its_task_to_the_window_still_prepared_for_it_when_its_record_is_read(
) {
    let context = Context::new();
    let ended = a_sessions_window_ends_as_its_record_is_read(&context);
    let preparing = context.adapter.prepare_holds.hold(|_| true);
    context.open_window(ended.project, &ended.session).unwrap();
    assert_eq!(
        context.host.last(&ended.session).unwrap().pane,
        ended.old,
        "no window yet: its harness is asked"
    );
    ended.looking.open();
    context.settle();
    ended.exiting.take().unwrap();
    assert_ne!(
        context.task(ended.project, 1).task.state,
        "paused",
        "the old window's exit stalls nothing of the one on its way"
    );
    preparing.open();
    context.settle();
    let reopened = context.host.last(&ended.session).unwrap().pane;
    assert_ne!(reopened, ended.old, "a window of the session's again");
    assert_ne!(context.task(ended.project, 1).task.state, "paused");
}

/// A session's window that ended with an answer on its way into it, the look at its
/// record with no window open held until the test lets it go.
struct Ended {
    project: i64,
    session: String,
    old: cf_harness::contract::Pane,
    looking: Gate,
    exiting: Answer<()>,
}

/// T-1 in a session's window, which asked and whose answer is on its way into it
/// when the window ends: the look at its record waits until the test lets it go.
fn a_sessions_window_ends_as_its_record_is_read(context: &Context) -> Ended {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let project = tiers.project.id;
    let session = context.task(project, 1).task.assignee.unwrap();
    let question = context.ask(project, &session, "chief", 1, "Which format?");
    context.adapter.answer("zeus", "I asked");
    context.pass().unwrap();
    let answer = context.answer(project, question.id, "JSON");
    context.pass().unwrap();
    assert_eq!(context.message(answer.id).state, "delivering");
    let old = context.host.last(&session).unwrap().pane;
    // Only the exit's look waits: a window opened again reads the record as ever.
    let (native, first) = (context.adapter.agent("zeus").native, Cell::new(true));
    let looking = context
        .adapter
        .record_holds
        .hold(move |asked| asked[0] == native.as_str() && first.replace(false));
    let (host, handle) = (Rc::clone(&context.host), session.clone());
    let exiting = context
        .executor
        .start(async move { host.exit(&handle).await });
    context.settle();
    Ended {
        project,
        session,
        old,
        looking,
        exiting,
    }
}

#[test]
fn confirms_a_paste_its_harness_recorded_when_its_window_ends_before_a_look_and_never_pastes_it_again(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let note = context.note(project.id, "zeus", "chief", "Taken");
    context.pass().unwrap();
    // The harness took it and its record has it; no look has confirmed it yet.
    assert_eq!(context.message(note.id).state, "delivering");
    let taken = context
        .adapter
        .agent("chief")
        .items
        .into_iter()
        .find(|item| item.role == Role::User && item.text.contains("Taken"))
        .unwrap();

    the_chiefs_window_ends(&context);
    let settled = context.message(note.id);
    assert_eq!(settled.state, "delivered", "the conversation holds it");
    assert_eq!(settled.receipt, serde_json::json!({ "item": &*taken.id }));

    context.resume_project(project.id).unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        given_saying(&context, "chief", "Taken"),
        0,
        "the window that came back is not given it again"
    );
}

#[test]
fn gives_back_a_paste_its_harness_never_recorded_when_its_window_ends_and_it_goes_again() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "Lost");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");

    the_chiefs_window_ends(&context);
    let settled = context.message(note.id);
    assert_eq!(
        (settled.state.as_str(), settled.reason.as_deref()),
        ("queued", Some("@chief's window closed"))
    );

    context.resume_project(project.id).unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivered");
    assert_eq!(
        given_saying(&context, "chief", "Lost"),
        1,
        "it goes again, once"
    );
}

#[test]
fn confirms_a_paste_its_harness_recorded_when_its_window_closes_while_it_is_handed_over() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    the_chiefs_window_closes_as_its_harness_takes_the_paste(&context);
    let note = context.note(project.id, "zeus", "chief", "Taken");
    context.pass().unwrap();
    assert_eq!(
        context.message(note.id).state,
        "delivered",
        "its record has it: the conversation holds it"
    );

    *context.adapter.deliver.borrow_mut() = None;
    context.resume_project(project.id).unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(given_saying(&context, "chief", "Taken"), 0);
}

#[test]
fn gives_back_a_paste_its_harness_never_recorded_when_its_window_closes_while_it_is_handed_over() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    the_chiefs_window_closes_as_its_harness_takes_the_paste(&context);
    let note = context.note(project.id, "zeus", "chief", "Lost");
    context.pass().unwrap();
    let settled = context.message(note.id);
    assert_eq!(
        (settled.state.as_str(), settled.reason.as_deref()),
        ("queued", Some("its window closed while it was handed over"))
    );
}
