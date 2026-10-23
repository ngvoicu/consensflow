//! A member's session whose role changed under it: resumed for a follow-up, the
//! role goes in first and the follow-up after it; a launch that does not come up
//! fails the brief the role went ahead of, as a launch always did; and a session
//! resumed on a task it still holds is not read to answer that task by the word
//! it gave the role. Where that meets the ending of what a turn left running (a
//! worker's answer is its task's result at once, and the host is asked to end what
//! its turn left with it), the word given the role is no result and the host is
//! asked to end nothing for it, and the answer that is the result has the host
//! asked, whichever word came after it. What the host then ends is its own choice,
//! from the marks it was given; the stand-in host here names what it was told runs.

use std::rc::Rc;

use cf_engine::testing::{Context, Restarted};
use cf_harness::records::Role;
use cf_ledger::{MessageView, NewNote, NewTask, ProjectView};
use serde_json::{json, Value};

use super::launch::refusing_pastes;
use super::{codex, recorded, replacements, REPLACES};

/// The words the window gives the role when it has it.
const UNDERSTOOD: &str = "Understood.";

/// A command the window's turn left running, which the stand-in host names as
/// ended when it is asked to end the window's work.
const JOB: &str = "python3 lock.py record --run 7";

/// A project whose chief is on Claude and whose one worker is a Codex member,
/// with the session that did T-1, a task it finished: its window answered, and
/// the task is done. Its handle, and its id.
pub(super) fn finished_session(context: &Context) -> (ProjectView, String, i64) {
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
fn a_sessions_word_to_its_role_is_no_result_and_asks_the_host_to_end_nothing_and_its_answer_to_the_task_is_the_result_and_asks_it_to_end_what_its_turn_left(
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
    context.background.run(&session, &[JOB]);

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
    assert_eq!(
        context.background.endings_asked(&session),
        0,
        "and the host is asked to end nothing for it"
    );
    assert_eq!(told_of_the_work(&context, &project), Vec::<String>::new());

    // Its answer to the task is the result at once, and the host is asked to end what
    // its turn left, which whoever gave the task is told it named.
    context.codex.answer(&session, "Parser done");
    for _ in 0..4 {
        after.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 1).task.state, "done");
    assert_eq!(result_of(&context, &project).body, "Parser done");
    assert_eq!(context.background.ended(&session), [JOB]);
    assert_eq!(
        told_of_the_work(&context, &project),
        [format!(
            "T-1's turn left background work running. Ended: {JOB}."
        )]
    );
}

#[test]
fn the_answer_a_session_gave_its_task_before_its_role_was_said_is_the_result_and_asks_the_host_to_end_what_its_turn_left(
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
    context.background.run(&session, &[JOB]);
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
    assert_eq!(
        context.background.ended(&session),
        [JOB],
        "and the host is asked to end what the turn left, as for any result"
    );
    assert_eq!(told_of_the_work(&context, &project).len(), 1);
}

/// A session at work on T-1 when the app quit, its role rewritten meanwhile, whose
/// window the Resume cannot open (its harness's launch failed before any window), and a
/// note the chief told it about T-1 before the Resume: the note is what the next launch
/// is for, and the role goes ahead of it. Every window from then on refuses every
/// paste, so the note that says the role fails. Exits are held, as the bridge host sends
/// a window's exit after it answers its kill (the test host's own raises the exit inside
/// its kill). The project, the engine started again, the session's handle, its id and
/// the note.
fn told_before_a_reopen_that_failed(
    context: &Context,
) -> (ProjectView, Restarted<'_>, String, i64, MessageView) {
    let project = working_session(context);
    let session = session_of(context, &project);
    let member = context.id(project.id, &session);
    quit_with_the_role_rewritten(context, &session);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    context.host.hold_exits.set(true);
    let reopening = session.clone();
    context
        .codex
        .prepare
        .replace(Some(Rc::new(move |launch: &Value| {
            if launch["participant"]["handle"] == reopening.as_str() {
                Err("no Codex".to_owned())
            } else {
                Ok(())
            }
        })));
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project.id,
            &NewNote {
                from: Some("chief".to_owned()),
                to: session.clone(),
                task: Some(1),
                body: "Use the grammar in the docs".to_owned(),
            },
        )
        .unwrap();
    after.resume_project(project.id).unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "working",
        "with no window"
    );
    context.codex.prepare.replace(None);
    refusing_pastes(context);
    (project, after, session, member, note)
}

/// The launch is given up as one that cannot say the role. Its exit is settled at the
/// close, before the note the role went ahead of goes, in the bridge host's order:
/// T-1 is paused and never failed, the note kept for its resume, and the chief and the
/// human told why, once each (T-47 P2-1: T-1 failed, the note "did not reach").
#[test]
fn a_session_whose_launch_cannot_say_its_role_ahead_of_a_note_told_it_pauses_its_task_and_keeps_the_note_for_its_resume(
) {
    let context = codex();
    let (project, after, session, member, note) = told_before_a_reopen_that_failed(&context);
    for _ in 0..6 {
        after.pass().unwrap();
    }

    let launch = context.codex.prepared().last().cloned().expect("a launch");
    assert_eq!(launch["participant"]["handle"], session.as_str());
    assert_eq!(launch["message"], Value::Null, "the role goes in first");
    // The reopen's note that says the role was withdrawn with it; the launch's own failed.
    let notes = replacements(&context, member);
    let states: Vec<(&str, i64)> = notes
        .iter()
        .map(|note| (note.state.as_str(), note.attempts))
        .collect();
    assert_eq!(states, [("cancelled", 0), ("failed", 3)]);
    let window = context.host.last(&session).unwrap().pane;
    assert!(
        context.host.killed().contains(&window),
        "its window is closed"
    );
    let why = "the note that says its role did not reach it";
    let kept = context.message(note.id);
    assert_eq!(
        (kept.state.as_str(), kept.reason.as_deref()),
        (
            "queued",
            Some(format!("kept for T-1's resume: {why}").as_str())
        )
    );
    assert_eq!(context.task(project.id, 1).task.state, "paused");
    let kept_words = format!("m-{}", note.id);
    let to_chief: Vec<String> = context
        .inbox(context.id(project.id, "chief"))
        .into_iter()
        .rev()
        .map(|message| message.body)
        .filter(|said| said.contains(&kept_words) || said.contains(why))
        .collect();
    assert_eq!(
        to_chief,
        [format!(
            "T-1 is paused: @{session}'s window closed: {why}; the words told its window wait for the resume: {kept_words}. Resume it with: cf task resume T-1 \"…\"; its window comes back on its own conversation."
        )]
    );
    let to_human: Vec<String> = context
        .inbox(context.id(project.id, "human"))
        .into_iter()
        .map(|message| message.body)
        .filter(|said| said.contains(&kept_words) || said.contains(why))
        .collect();
    assert_eq!(
        to_human,
        [format!(
            "{kept_words}, a note from @chief on T-1, waits for T-1's resume to reach @{session}: {why}."
        )]
    );

    // The host's exit, when it comes, settles nothing more.
    let host = Rc::clone(&context.host);
    context.run(async move { host.exited(window).await });
    after.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "paused");
    assert_eq!(context.message(note.id).state, "queued");
}

/// The same launch, whose window's kill the host refuses: no exit pauses T-1, and the
/// note the role went ahead of goes, failing, T-1 with it, whoever is told hearing why
/// (T-63, as for a window unfit for work).
#[test]
fn a_session_whose_launch_cannot_say_its_role_and_whose_kill_is_refused_has_the_note_told_it_fail_and_says_why(
) {
    let context = codex();
    let (project, after, session, _member, note) = told_before_a_reopen_that_failed(&context);
    context.host.refuse_kills.set(true);
    for _ in 0..6 {
        after.pass().unwrap();
    }

    let why = "the note that says its role did not reach it";
    assert_eq!(context.message(note.id).state, "failed");
    assert_eq!(context.task(project.id, 1).task.state, "failed");
    let kept_words = format!("m-{}", note.id);
    let to_chief: Vec<String> = context
        .inbox(context.id(project.id, "chief"))
        .into_iter()
        .map(|message| message.body)
        .filter(|said| said.contains(&kept_words))
        .collect();
    assert_eq!(
        to_chief,
        [format!(
            "T-1 failed: what @chief told its window, {kept_words}, did not reach it ({why}). Reopen it with: cf task reopen T-1 \"…\""
        )]
    );
    let to_human: Vec<String> = context
        .inbox(context.id(project.id, "human"))
        .into_iter()
        .map(|message| message.body)
        .filter(|said| said.contains(&kept_words))
        .collect();
    assert_eq!(
        to_human,
        [format!(
            "{kept_words}, a note from @chief on T-1, did not reach @{session}: {why}."
        )]
    );
}
