//! One task per member session: a window opens with its task and closes with
//! it; the session and its conversation stay until the human deletes it, and
//! a follow-up or a restart brings its window back on its conversation.

use cf_engine::testing::{window_of, Context};
use cf_ledger::NewTask;

use crate::fixtures::{
    assert_match, finished, last_launch, last_message, native_of, placed, Tiers,
};
use crate::traces::held_to;

const SUITES: &[&str] = &["one task per member session"];

#[test]
fn closes_a_sessions_window_with_its_task_but_keeps_its_conversation_and_opens_a_fresh_session_for_the_next_task(
) {
    let context = Context::new();
    let tiers = finished(&context);
    let first = context.host.last("zeus").unwrap().pane;
    let session = tiers.id("zeus-amber-pine");
    assert_eq!(tiers.task(1).task.state, "done");
    assert_eq!(context.host.killed(), [first]);
    assert!(
        context
            .ledger
            .borrow()
            .current_conversation(session)
            .unwrap()
            .is_some(),
        "the conversation stays until the work is accepted, for a follow-up"
    );
    assert_eq!(
        context.dispatcher.pane(session),
        None,
        "the window went with the task"
    );

    tiers.open_body("Write the lexer");
    context.pass().unwrap();
    assert_eq!(
        last_launch(&context),
        ("zeus-brisk-birch".to_owned(), None),
        "a fresh session of the same member"
    );
    assert_match(&last_message(&context), "Write the lexer");
    let windows = context
        .host
        .opened()
        .into_iter()
        .filter(|open| window_of(&open.pane.id, "zeus"))
        .count();
    assert_eq!(windows, 2);

    context
        .ledger
        .borrow_mut()
        .accept_task(tiers.project.id, 1, "chief")
        .unwrap();
    context.pass().unwrap();
    let kept = context
        .ledger
        .borrow()
        .current_conversation(session)
        .unwrap();
    assert!(
        kept.is_some(),
        "accepted: the session keeps its conversation for the human"
    );
    context
        .end_session(tiers.project.id, "zeus-amber-pine")
        .unwrap();
    assert_eq!(
        context
            .ledger
            .borrow()
            .current_conversation(session)
            .unwrap()
            .map(|conversation| conversation.id),
        kept.map(|conversation| conversation.id),
        "deleted, it keeps its conversation for a follow-up"
    );
    assert!(tiers.participant("zeus-amber-pine").is_none());
    held_to(
        context.close(),
        SUITES,
        "closes a session's window with its task but keeps its conversation, and opens a fresh session for the next task",
    );
}

#[test]
fn opens_the_next_tasks_session_while_the_old_window_is_still_closing() {
    let context = Context::new();
    context.host.hold_exits.set(true);
    let tiers = finished(&context);
    tiers.open_body("Write the lexer");
    context.pass().unwrap();
    assert_eq!(tiers.assignee(2), "zeus-brisk-birch");
    let windows = context
        .host
        .opened()
        .into_iter()
        .filter(|open| window_of(&open.pane.id, "zeus"))
        .count();
    assert_eq!(windows, 2, "a session of its own waits for no window");
    assert_eq!(last_launch(&context).1, None);
    context.exit("zeus-amber-pine");
    context.pass().unwrap();
    assert_eq!(
        tiers.task(2).task.state,
        "working",
        "the old exit lands on the old session only"
    );
    held_to(
        context.close(),
        SUITES,
        "opens the next task's session while the old window is still closing",
    );
}

#[test]
fn never_closes_a_coordinator_the_chief_keeps_its_window_after_its_own_task() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.create_task(
        project.id,
        NewTask {
            from: "human".to_owned(),
            to: Some("chief".to_owned()),
            body: "Plan the week".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("chief", "Planned.");
    context
        .ledger
        .borrow_mut()
        .record_result(project.id, 1, "Planned.")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");
    assert!(context.host.killed().is_empty());
    assert_ne!(
        context.dispatcher.pane(context.id(project.id, "chief")),
        None
    );
    held_to(
        context.close(),
        SUITES,
        "never closes a coordinator: the chief keeps its window after its own task",
    );
}

#[test]
fn after_a_restart_pauses_a_member_task_with_no_window_for_the_chief_to_resume_and_resumes_one_whose_answer_is_due(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    tiers.open();
    tiers.open_body("Write the lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        [tiers.assignee(1), tiers.assignee(2)],
        ["zeus-amber-pine", "diana-brisk-birch"]
    );
    let question = context
        .ledger
        .borrow_mut()
        .ask(
            tiers.project.id,
            &cf_ledger::NewQuestion {
                from: Some("diana-brisk-birch".to_owned()),
                to: "chief".to_owned(),
                task: Some(2),
                body: Some("Which dialect?".to_owned()),
                ..cf_ledger::NewQuestion::default()
            },
        )
        .unwrap();
    context.adapter.answer("diana", "I asked the chief.");
    context.pass().unwrap();
    assert_eq!(
        [tiers.task(1).task.state, tiers.task(2).task.state],
        ["working", "waiting"]
    );
    let native = native_of(&context, tiers.id("diana-brisk-birch"));
    let native_zeus = native_of(&context, tiers.id("zeus-amber-pine"));

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.resume_after_restart().unwrap();
    context
        .ledger
        .borrow_mut()
        .answer(
            question.id,
            question.recipient_id,
            Some(&serde_json::json!("ANSI")),
            None,
        )
        .unwrap();
    after.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("paused", Some("zeus-amber-pine")),
        "its window is gone; its session and conversation wait for the chief"
    );
    let thread = tiers.task(1);
    let note = thread.messages.iter().find(|m| m.kind == "note").unwrap();
    assert_match(
        &note.body,
        r"^T-1 is paused: @zeus-amber-pine's window is gone\. Resume it with: cf task resume T-1",
    );
    assert_eq!(
        last_launch(&context),
        ("diana-brisk-birch".to_owned(), native),
        "its own session, with the brief in it"
    );
    assert_match(
        &last_message(&context),
        r"^\[ConsensFlow m-\d+ · T-2 · answer from @chief\]\nANSI$",
    );
    after.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "working");
    context
        .ledger
        .borrow_mut()
        .resume_task(tiers.project.id, 1, Some("chief"), "Go on")
        .unwrap();
    after.pass().unwrap();
    assert_eq!(
        last_launch(&context),
        ("zeus-amber-pine".to_owned(), native_zeus),
        "the same conversation, with its memory"
    );
    assert_match(
        &last_message(&context),
        r"T-1 · task from @chief\]\nResumed: Go on$",
    );
    held_to(
        context.close(),
        SUITES,
        "after a restart, pauses a member task with no window for the chief to resume, and resumes one whose answer is due",
    );
}

#[test]
fn reopens_a_finished_task_on_its_own_session_resumed_with_the_follow_up_and_nothing_else() {
    let context = Context::new();
    let tiers = finished(&context);
    let native = native_of(&context, tiers.id("zeus-amber-pine"));
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
        "reopens a finished task on its own session, resumed with the follow-up and nothing else",
    );
}

#[test]
fn continues_a_finished_tasks_session_with_after_the_same_window_comes_back_on_its_conversation() {
    let context = Context::new();
    let tiers = finished(&context);
    let native = native_of(&context, tiers.id("zeus-amber-pine"));
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
    context.adapter.answer("zeus-amber-pine", "Lexer done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(2).task.state, "done");
    assert_eq!(
        context.host.killed().len(),
        2,
        "the window closed after each task"
    );
    held_to(
        context.close(),
        SUITES,
        "continues a finished task's session with --after: the same window comes back on its conversation",
    );
}
