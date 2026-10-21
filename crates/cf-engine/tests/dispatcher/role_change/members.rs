//! A member's session whose role changed under it: resumed for a follow-up, the
//! role goes in first and the follow-up after it; a launch that does not come up
//! fails the brief the role went ahead of, as a launch always did; and a session
//! resumed on a task it still holds is not read to answer that task by the word
//! it gave the role. The word given the role is no result, and the answer that is
//! the result is, whichever word came after it; neither ends what the turn left
//! running (turn-end's smallest rule, 2026-10-21).

use std::rc::Rc;

use cf_engine::testing::Context;
use cf_harness::records::Role;
use cf_ledger::{MessageView, NewTask, ProjectView};
use serde_json::{json, Value};

use super::{codex, recorded, replacements, REPLACES};

/// The words the window gives the role when it has it.
const UNDERSTOOD: &str = "Understood.";

/// A project whose chief is on Claude and whose one worker is a Codex member,
/// with the session that did T-1, a task it finished: its window answered, and
/// the task is done. Its handle, and its id.
pub(super) fn finished_session(context: &Context) -> (ProjectView, String, i64) {
    // The host proves each window's program gone, as the app's does: a follow-up,
    // another task, goes on with the session's conversation (`ownership`).
    context.host.exits_showing(Some(0), None, None);
    let project = working_session(context);
    let session = session_of(context, &project);
    context.codex.answer(&session, "Parser done");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");
    let member = context.id(project.id, &session);
    (project, session, member)
}

/// The same project with its session at work on T-1: its window has the
/// brief, and has not answered.
fn working_session(context: &Context) -> ProjectView {
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [{ "agent": "diana", "harness": "codex", "roles": ["worker"], "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project");
    context.pool_task(project.id, "worker", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    project
}

/// The handle of the session of the project's member.
fn session_of(context: &Context, project: &ProjectView) -> String {
    context
        .project(project.id)
        .participants
        .into_iter()
        .find(|participant| participant.member_id.is_some())
        .expect("a session")
        .handle
}

/// The chief gives the session that did T-1 a follow-up.
pub(super) fn follow_up(context: &Context, project: &ProjectView, body: &str) {
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: body.to_owned(),
            ..NewTask::default()
        },
    );
}

/// What was said to the window of `session`, in order: what its record holds.
pub(super) fn said_to(context: &Context, session: &str) -> Vec<String> {
    context
        .codex
        .agent(session)
        .items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| item.text.to_string())
        .collect()
}

#[test]
fn a_members_replacement_that_failed_to_land_leaves_the_record_as_it_was_and_its_next_launch_says_the_role_again(
) {
    let context = codex();
    let (project, _session, member) = finished_session(&context);

    // A release rewrote the texts; the chief gives the session a follow-up, and
    // the session's next launch fails before its window opens. The role it was to
    // say first goes ahead of the brief, and the brief is what fails with the
    // launch, as it always did.
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    context
        .codex
        .prepare
        .replace(Some(Rc::new(|_| Err("no Codex".to_owned()))));
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    let failed = replacements(&context, member);
    assert_eq!(failed.len(), 1);
    assert_eq!(
        context.message(failed[0].id).state,
        "cancelled",
        "written for a window that never came up, and no queue of a member's would give it to another"
    );
    assert_eq!(
        recorded(&context, member).as_deref(),
        Some("instructions for worker"),
        "the session was told nothing: the record is what it has"
    );
    assert_eq!(
        context.task(project.id, 2).task.state,
        "failed",
        "the brief it went ahead of fails with the launch, as it always did"
    );
    // And that is the end of it: no launch is tried again at every pass, with a
    // new replacement written ahead of nothing each time.
    for _ in 0..3 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(
        replacements(&context, member).len(),
        1,
        "no other is written"
    );

    // The window can open now. The chief gives the session another follow-up,
    // and its launch says the role again, first.
    context.codex.prepare.replace(None);
    follow_up(&context, &project, "Now the lexer, again");
    context.pass().unwrap();
    context.pass().unwrap();
    let notes = replacements(&context, member);
    assert_eq!(
        notes.len(),
        2,
        "the role is said again, as a note of its own"
    );
    let launches = context.codex.prepared();
    assert_eq!(
        launches.last().expect("a launch")["message"],
        Value::Null,
        "by paste, once the window is up"
    );
    context.pass().unwrap();
    assert_eq!(context.message(notes[1].id).state, "delivered");
    assert_eq!(
        recorded(&context, member).as_deref(),
        Some("instructions for worker (rewritten)"),
        "told, and recorded as told, once it landed"
    );
    context.close();
}

#[test]
fn a_members_window_that_opens_and_cannot_start_fails_the_brief_the_role_went_ahead_of_and_writes_no_other_replacement(
) {
    let context = codex();
    let (project, session, member) = finished_session(&context);
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    context.codex.started.replace(Some(Rc::new(
        || Err("the server never answered".to_owned()),
    )));
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    context.pass().unwrap();

    let launches = context.codex.prepared();
    assert_eq!(launches.len(), 2, "the window was opened");
    assert_eq!(launches[1]["message"], Value::Null);
    assert_eq!(
        context.task(project.id, 2).task.state,
        "failed",
        "its window could not start, and the brief the role went ahead of fails with it"
    );
    let failed = replacements(&context, member);
    assert_eq!(failed.len(), 1);
    assert_eq!(context.message(failed[0].id).state, "cancelled");
    assert_eq!(
        recorded(&context, member).as_deref(),
        Some("instructions for worker"),
        "the session was told nothing"
    );

    // No launch is tried again at every pass, a role written ahead of nothing each time.
    for _ in 0..3 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(replacements(&context, member).len(), 1);
    assert_eq!(context.codex.prepared().len(), 2, "no other window opens");
    assert_eq!(
        said_to(&context, &session),
        Vec::<String>::new(),
        "nothing was said to the window that could not start"
    );
    context.close();
}

#[test]
fn a_members_session_resumed_for_a_follow_up_is_given_a_rewritten_role_first_and_the_follow_up_after_it(
) {
    let context = codex();
    let (project, session, _member) = finished_session(&context);
    assert_eq!(
        context.codex.prepared()[0]["instructions"],
        "instructions for worker"
    );

    // A release rewrote the texts; the chief gives the session a follow-up.
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    context.pass().unwrap();

    let launches = context.codex.prepared();
    assert_eq!(launches.len(), 2);
    assert!(launches[1]["resume"].is_string(), "{}", launches[1]);
    assert_eq!(
        launches[1]["instructions"],
        "instructions for worker (rewritten)"
    );
    assert_eq!(
        launches[1]["message"],
        Value::Null,
        "neither the role nor the brief is on the command line"
    );
    let waiting = context.task(project.id, 2);
    assert_eq!(
        waiting.task.state, "queued",
        "the follow-up waits for its turn, and has not been lost"
    );
    // The role goes in first, by paste; the follow-up once the window has answered it.
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 2).task.state, "queued");
    context.codex.answer(&session, UNDERSTOOD);
    context.pass().unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 2).task.state, "working");
    let texts = said_to(&context, &session);
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(texts[0].contains(REPLACES), "{}", texts[0]);
    assert!(texts[1].contains("Now the lexer"), "{}", texts[1]);
    context.close();
}

#[test]
fn a_follow_up_paused_after_the_launch_that_put_the_role_ahead_of_it_waits_when_its_turn_comes() {
    let context = codex();
    let (project, session, member) = finished_session(&context);
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    // The window is up, the role due first and the follow-up after it, and the
    // daemon pauses the follow-up before its turn comes (a pause of the chief's
    // would withdraw its brief): the brief waits, and the task is not one its
    // member holds.
    context
        .ledger
        .borrow_mut()
        .pause_task(project.id, 2, None, Some("a test pauses it"))
        .unwrap();
    context.pass().unwrap();
    context.codex.answer(&session, UNDERSTOOD);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let texts = said_to(&context, &session);
    assert_eq!(texts.len(), 1, "{texts:?}");
    assert!(texts[0].contains(REPLACES), "{}", texts[0]);
    assert_eq!(
        context.task(project.id, 2).task.state,
        "paused",
        "a task that is paused is not given the words it was to be given"
    );
    let waiting = context
        .inbox(member)
        .into_iter()
        .filter(|message| message.kind == "task" && message.state == "queued")
        .count();
    assert_eq!(waiting, 1, "its brief waits for the words that resume it");
    context.close();
}

/// The session's window is cut by a quit: what it held is what the resumed
/// window holds, with the role rewritten meanwhile. The engine started again.
fn quit_with_the_role_rewritten(context: &Context, session: &str) {
    let held = context.codex.agent(session).items;
    let codex = Rc::downgrade(&context.codex);
    *context.codex.after_prepare.borrow_mut() = Some(Rc::new(move |handle| {
        if let Some(codex) = codex.upgrade() {
            let held = held.clone();
            codex.with(handle, |agent| {
                let after = std::mem::replace(&mut agent.items, held);
                agent.items.extend(after);
            });
        }
    }));
    context.roles.rewritten.replace(" (rewritten)".to_owned());
}

/// The result T-1 gave the chief.
fn result_of(context: &Context, project: &ProjectView) -> MessageView {
    let chief = context.id(project.id, "chief");
    context
        .inbox(chief)
        .into_iter()
        .find(|message| message.kind == "result")
        .expect("the result")
}

#[test]
fn a_session_resumed_on_a_task_it_holds_is_not_read_to_answer_it_by_the_word_it_gave_the_role() {
    let context = codex();
    let project = working_session(&context);
    let session = session_of(&context, &project);
    quit_with_the_role_rewritten(&context, &session);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project.id).unwrap();

    // The window is up with what it held: the brief, with no answer. The role goes
    // in first, by paste, and the window has it.
    after.pass().unwrap();
    after.pass().unwrap();
    context.codex.answer(&session, UNDERSTOOD);
    after.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "working",
        "its answer to the role is no answer to the task"
    );
    // The task goes on where it stopped, and its answer is its result, and only that.
    after.pass().unwrap();
    after.pass().unwrap();
    context.codex.answer(&session, "Parser done");
    for _ in 0..4 {
        after.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 1).task.state, "done");
    let result = result_of(&context, &project);
    assert_eq!(result.body, "Parser done", "{}", result.body);
    let texts = said_to(&context, &session);
    assert!(
        texts.iter().any(|text| text.contains(REPLACES)),
        "{texts:?}"
    );
}

#[test]
fn a_session_that_had_answered_its_task_before_the_quit_gives_that_answer_as_its_result_and_not_the_word_it_gave_the_role(
) {
    let context = codex();
    let project = working_session(&context);
    let session = session_of(&context, &project);
    // Its turn ended, the daemon never saw it, and the app quit.
    context.codex.answer(&session, "Parser done");
    quit_with_the_role_rewritten(&context, &session);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project.id).unwrap();
    after.pass().unwrap();
    after.pass().unwrap();
    context.codex.answer(&session, UNDERSTOOD);
    for _ in 0..3 {
        after.pass().unwrap();
    }

    assert_eq!(context.task(project.id, 1).task.state, "done");
    let result = result_of(&context, &project);
    assert_eq!(
        result.body, "Parser done",
        "the answer it gave the task, which the role's word comes after"
    );
}

/// What whoever gave T-1 was told that its turn left running, as notes.
fn told_of_the_work(context: &Context, project: &ProjectView) -> Vec<String> {
    let chief = context.id(project.id, "chief");
    context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.kind == "note" && message.body.contains("background work"))
        .map(|message| message.body)
        .collect()
}

#[test]
fn a_sessions_word_to_its_role_is_no_result_and_its_answer_to_the_task_is_the_result_and_neither_ends_what_its_turn_left(
) {
    let context = codex();
    let project = working_session(&context);
    let session = session_of(&context, &project);
    quit_with_the_role_rewritten(&context, &session);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project.id).unwrap();
    // The host says what the window's turn left running.

    // The role goes in first, and the window gives it its word: a turn that settled
    // with a complete answer, which is the role's and none of the task's.
    after.pass().unwrap();
    after.pass().unwrap();
    context.codex.answer(&session, UNDERSTOOD);
    for _ in 0..3 {
        after.pass().unwrap();
    }
    assert_eq!(
        context.task(project.id, 1).task.state,
        "working",
        "its word to the role is no result"
    );
    assert_eq!(told_of_the_work(&context, &project), Vec::<String>::new());

    // Its answer to the task is the result at once, and nothing its turn left is ended
    // (turn-end's smallest rule, 2026-10-21).
    context.codex.answer(&session, "Parser done");
    for _ in 0..4 {
        after.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 1).task.state, "done");
    assert_eq!(result_of(&context, &project).body, "Parser done");
}

#[test]
fn the_answer_a_session_gave_its_task_before_its_role_was_said_is_the_result_and_ends_nothing_its_turn_left(
) {
    let context = codex();
    let project = working_session(&context);
    let session = session_of(&context, &project);
    // Its turn ended with its answer and a job running, the daemon never saw it, and
    // the app quit.
    context.codex.answer(&session, "Parser done");
    quit_with_the_role_rewritten(&context, &session);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project.id).unwrap();
    after.pass().unwrap();
    after.pass().unwrap();
    context.codex.answer(&session, UNDERSTOOD);
    for _ in 0..3 {
        after.pass().unwrap();
    }

    assert_eq!(context.task(project.id, 1).task.state, "done");
    assert_eq!(
        result_of(&context, &project).body,
        "Parser done",
        "the answer it gave the task, which the role's word comes after"
    );
}
