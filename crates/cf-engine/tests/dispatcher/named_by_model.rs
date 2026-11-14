//! A board that ran on ready-made agents, upgraded while a member's window was paused
//! (spec harness-models, the chief's condition of 2026-11-03): migration 0031 names the
//! member and its window by the model they run on, and 0033 by the model's id (`@poseidon` is
//! `@opus-high`, then `@claude-opus-5-5-high`, its window `@claude-opus-5-5-high-<two
//! words>`). The window keeps working once it is resumed: everything that
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
        cf_ledger::testing::undo_migration_0040(&db).unwrap();
        cf_ledger::testing::undo_migration_0038(&db).unwrap();
        cf_ledger::testing::undo_migration_0036(&db).unwrap();
        cf_ledger::testing::undo_migration_0035(&db).unwrap();
        cf_ledger::testing::undo_migration_0034(&db).unwrap();
        cf_ledger::testing::undo_migration_0033(&db).unwrap();
        cf_ledger::testing::undo_migration_0032(&db).unwrap();
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
    let renamed = format!("claude-opus-5-5-high-{session}");
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
            Some("claude-opus-5-5".to_owned()),
        )
    };
    assert_eq!(seen(member), named("claude-opus-5-5-high"));
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

/// Edit member (HMD-12) while the member's window is at work: the member and the window take
/// the name of the model the member runs on now, at once, and the window goes on. Everything
/// that finds it goes by its id (its token, its conversation, its task's assignee), so the
/// task is still its own, what the chief writes to the new name is delivered to the same
/// window, and its result finishes the task. A window open keeps what it runs on; the name
/// it left is kept.
#[test]
fn a_member_edited_while_its_window_is_at_work_renames_both_and_the_window_goes_on() {
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
    let old = before.assignee.clone().expect("a window has it");
    let window = context.id(project.id, &old);
    let launches = context.adapter.prepared().len();
    let member = context
        .project(project.id)
        .participants
        .into_iter()
        .find(|participant| participant.handle == "zeus")
        .unwrap();
    let edited = context
        .ledger
        .borrow_mut()
        .edit_member(
            project.id,
            "zeus",
            &cf_ledger::MemberEdit {
                harness: "claude-code".to_owned(),
                model: Some("claude-fable-5-1".to_owned()),
                effort: Some("max".to_owned()),
                model_name: Some("claude-fable-5-1".to_owned()),
                tier: member.tier.clone().unwrap(),
            },
        )
        .unwrap();
    assert_eq!(edited.handle, "claude-fable-5-1-max");
    let session = old.strip_prefix("zeus-").unwrap();
    let renamed = format!("claude-fable-5-1-max-{session}");
    context.pass().unwrap();
    // The window is the same participant, under its new name, and its task is its own.
    let now = context.project(project.id);
    let seen = now.participants.iter().find(|p| p.id == window).unwrap();
    assert_eq!(
        (
            seen.handle.as_str(),
            seen.former_handles.as_slice(),
            seen.member.as_deref()
        ),
        (
            renamed.as_str(),
            [old.clone()].as_slice(),
            Some("claude-fable-5-1-max")
        )
    );
    let task = context.task(project.id, 1).task;
    assert_eq!(
        (task.state.as_str(), task.assignee.as_deref()),
        ("working", Some(renamed.as_str()))
    );
    // Its result finishes the task; no second window was opened for the rename.
    context
        .ledger
        .borrow_mut()
        .record_result(project.id, 1, "The parser writes YAML.")
        .unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "the window went on"
    );

    // What the chief writes goes by the new name to the same participant, and the name it
    // left is no name of anybody's: a note to it is refused.
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project.id,
            &NewNote {
                from: Some("chief".to_owned()),
                to: renamed.clone(),
                body: "Output must be YAML".to_owned(),
                task: None,
            },
        )
        .unwrap();
    assert_eq!(context.message(note.id).recipient_id, window);
    let refused = context.ledger.borrow_mut().note(
        project.id,
        &NewNote {
            from: Some("chief".to_owned()),
            to: old.clone(),
            body: "Output must be YAML".to_owned(),
            task: None,
        },
    );
    assert!(refused.is_err(), "the old name is nobody's now");
    context.close();
}
