//! A board that ran on ready-made agents, upgraded while a member's window was paused
//! (spec harness-models, the chief's condition of 2026-11-03): migration 0031 names the
//! member and its window by the model they run on (`@poseidon` is `@opus-high`, its window
//! `@opus-high-<two words>`). The window keeps working once it is resumed: everything that
//! finds it goes by its id (its token names it so, its conversation, its task's
//! assignee), never by the name it had, so it is resumed on its own conversation, it is
//! delivered what is written to it, and its `cf task done` finishes its task.

use cf_engine::testing::Context;
use cf_ledger::{NewNote, NewTask};

#[test]
fn a_window_paused_across_the_migration_is_resumed_finishes_its_task_and_is_delivered_to() {
    let context = Context::new();
    context.adapter.resumes_records.set(true);
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // A task for the standard workers: it runs in a window of its own, `@zeus-<words>`.
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Write the parser".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let before = context.task(project.id, 1).task;
    assert_eq!(before.state, "working");
    let old = before.assignee.clone().expect("a window has it");
    let window = context.id(project.id, &old);
    let member = context.id(project.id, "zeus");
    let session = old
        .strip_prefix("zeus-")
        .unwrap_or_else(|| panic!("{old}"))
        .to_owned();
    context.pause_task(project.id, 1);
    // The board as 3.0.7-alpha.1 left it: the member of the ready-made agent Poseidon
    // (Opus 5.5 at high on Claude Code), and its window, named after it.
    context.edit_ledger(|file| {
        let db = rusqlite::Connection::open(file).unwrap();
        cf_ledger::testing::undo_migration_0031(&db).unwrap();
        db.execute_batch(&format!(
            "UPDATE participant SET agent = 'poseidon', model = NULL, tier = 'complex'
               WHERE id IN ({member}, {window});
             UPDATE participant SET handle = 'poseidon' WHERE id = {member};
             UPDATE participant SET handle = 'poseidon-{session}' WHERE id = {window};
             PRAGMA user_version = 30;"
        ))
        .unwrap();
    });
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    let renamed = format!("opus-high-{session}");
    let migrated = context.project(project.id);
    let seen = |id: i64| {
        let participant = migrated
            .participants
            .iter()
            .find(|participant| participant.id == id)
            .unwrap();
        (
            participant.handle.clone(),
            participant.agent.clone(),
            participant.model.clone(),
            participant.effort.clone(),
            participant.model_name.clone(),
        )
    };
    let named = |handle: &str| {
        (
            handle.to_owned(),
            Some("poseidon".to_owned()),
            Some("claude-opus-5-5".to_owned()),
            Some("high".to_owned()),
            Some("Opus".to_owned()),
        )
    };
    assert_eq!(seen(member), named("opus-high"));
    assert_eq!(seen(window), named(&renamed));
    assert_eq!(
        context.task(project.id, 1).task.assignee.as_deref(),
        Some(renamed.as_str()),
        "the task is its window's still, by its id"
    );

    // The chief writes the window under its new handle while its task waits, and resumes
    // the task: the words and the note go in together, into its own conversation.
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project.id,
            &NewNote {
                from: Some("chief".to_owned()),
                to: renamed.clone(),
                body: "Output must be YAML".to_owned(),
                task: Some(1),
            },
        )
        .unwrap();
    after.resume_project(project.id).unwrap();
    context.resume_task(project.id, 1, "Carry on");
    for _ in 0..4 {
        after.pass().unwrap();
    }
    let launched = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(launched["participant"]["handle"], renamed.as_str());
    assert!(
        launched["resume"]
            .as_str()
            .is_some_and(|resume| !resume.is_empty()),
        "on its own conversation: {launched}"
    );
    assert_eq!(launched["agent"]["model"], "claude-opus-5-5");
    assert_eq!(launched["agent"]["effort"], "high");
    let resumed = context
        .task(project.id, 1)
        .messages
        .into_iter()
        .find(|message| message.body == "Resumed: Carry on")
        .unwrap();
    assert_eq!(
        (
            context.message(resumed.id).state,
            context.message(note.id).state
        ),
        ("delivered".to_owned(), "delivered".to_owned())
    );
    let pasted = context
        .adapter
        .agent(&renamed)
        .items
        .iter()
        .rev()
        .find(|item| item.role == cf_harness::records::Role::User)
        .map(|item| item.text.to_string())
        .unwrap_or_default();
    assert!(pasted.contains("Output must be YAML"), "{pasted}");

    // Its `cf task done`: the API finds the caller by its token's participant id, and the
    // task is the caller's where its assignee is the caller's handle now.
    let caller = context
        .project(project.id)
        .participants
        .into_iter()
        .find(|participant| participant.id == window)
        .unwrap();
    assert_eq!(
        context.task(project.id, 1).task.assignee.as_deref(),
        Some(caller.handle.as_str())
    );
    context
        .ledger
        .borrow_mut()
        .record_result(project.id, 1, "The parser writes YAML.")
        .unwrap();
    after.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");
    context.close();
}

/// A member the update could not run on what it ran on before (its agent was in no catalog
/// and no agents file: T-1202) keeps its name and needs an edit: it takes no new task, and
/// a window of it that comes back after a restart does not open on its harness's default.
/// Its task goes back to the board for another member.
#[test]
fn a_member_that_needs_an_edit_takes_no_task_and_opens_no_window_on_a_default() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Write the parser".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let before = context.task(project.id, 1).task;
    assert_eq!(before.state, "working");
    let member = context.id(project.id, "zeus");
    let launches = context.adapter.prepared().len();
    // The update found nothing to run zeus on: it and its window need an edit.
    context.edit_ledger(|file| {
        let db = rusqlite::Connection::open(file).unwrap();
        db.execute_batch(&format!(
            "UPDATE participant SET model = NULL, effort = NULL, needs_edit = 1
               WHERE id = {member} OR member_id = {member};"
        ))
        .unwrap();
    });
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project.id).unwrap();
    for _ in 0..4 {
        after.pass().unwrap();
    }
    let opened: Vec<_> = context.adapter.prepared()[launches..]
        .iter()
        .filter(|launch| launch["role"] != "chief")
        .cloned()
        .collect();
    assert_eq!(
        opened,
        Vec::<serde_json::Value>::new(),
        "no member window opened"
    );
    let task = context.task(project.id, 1).task;
    assert_eq!(
        (task.state.as_str(), task.assignee.as_deref()),
        ("open", None),
        "its task is back on the board"
    );
    // And a new one waits for a member that can take it.
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Write the lexer".to_owned(),
            ..NewTask::default()
        },
    );
    after.pass().unwrap();
    assert_eq!(context.task(project.id, 2).task.state, "open");
    // It was never given to the member that needs an edit: no member was free for it.
    let given: Vec<_> = context
        .ledger
        .borrow()
        .events(project.id, 0, 10_000)
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == "task.assigned" && event.data["task"] == 2)
        .collect();
    assert!(given.is_empty(), "{given:?}");
}
