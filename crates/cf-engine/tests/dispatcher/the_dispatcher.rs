//! The dispatcher: windows launched with their first message, looks,
//! deliveries proved by the record, results collected, and the windows
//! closed (`describe('the dispatcher')`).

use cf_engine::testing::Context;
use cf_engine::ActivityState;
use cf_harness::records::Role;
use serde_json::Value;

use crate::traces::held_to;

const SUITES: &[&str] = &["the dispatcher"];

#[test]
fn opens_a_project_with_its_chief_window_and_binds_the_chief_conversation() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.host.last("chief").expect("the chief's window");
    assert_eq!(chief.pane.id, format!("p{}-chief", project.id));
    assert_eq!(chief.argv, ["/bin/fake-agent", "chief"]);
    assert_eq!(chief.cwd, "/work/app");
    let env = |key: &str| {
        chief
            .env
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(env("FAKE_AGENT").as_deref(), Some("chief"));
    assert_eq!(env("CONSENSFLOW_PARTICIPANT").as_deref(), Some("chief"));
    assert_eq!(env("CONSENSFLOW_TOKEN").as_deref(), Some("token-chief"));
    let prepared = context.adapter.prepared();
    assert_eq!(
        prepared[0]["message"],
        Value::Null,
        "a chief opens without a task"
    );
    assert_eq!(prepared[0]["role"], "chief");
    assert_eq!(prepared[0]["instructions"], "instructions for chief");
    let chief_id = context.id(project.id, "chief");
    let conversation = context
        .ledger
        .borrow()
        .current_conversation(chief_id)
        .unwrap()
        .unwrap();
    let launch = prepared[0]["launchId"].as_str().unwrap();
    assert_eq!(
        conversation.native_session,
        Some(format!("native-{launch}"))
    );
    assert_eq!(
        context.dispatcher.activity(chief_id).state,
        ActivityState::Starting
    );
    context.pass().unwrap();
    assert_eq!(
        context.dispatcher.activity(chief_id).state,
        ActivityState::Idle
    );
    held_to(
        context.close(),
        SUITES,
        "opens a project with its chief window and binds the chief conversation",
    );
}

#[test]
fn launches_a_worker_with_its_task_as_the_first_message_and_records_its_answer_as_the_result() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let created = context.give(project.id, "zeus", "Write the parser");
    let message = created.message.expect("its brief");
    context.pass().unwrap();
    let first = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(first["participant"]["handle"], "zeus");
    let text = first["message"].as_str().unwrap();
    assert!(text.starts_with(&format!(
        "[ConsensFlow m-{} · T-1 · task from @chief]\n",
        message.id
    )));
    assert_eq!(
        context.task(project.id, 1).task.state,
        "queued",
        "not yet seen by the agent"
    );
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    let zeus = context.id(project.id, "zeus");
    assert_eq!(
        context.dispatcher.activity(zeus).state,
        ActivityState::Working
    );
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "done");
    let result = task.messages.iter().find(|m| m.kind == "result").unwrap();
    assert_eq!(
        (result.body.as_str(), result.recipient.as_str()),
        ("Parser done", "chief")
    );
    held_to(
        context.close(),
        SUITES,
        "launches a worker with its task as the first message and records its answer as the result",
    );
}

#[test]
fn keeps_everything_a_member_wrote_in_its_turn_not_only_its_last_message() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Write the report");
    context.pass().unwrap();
    context.pass().unwrap();
    // The report, a command, then a short last word.
    let report = format!(
        "{}The code at the end: CEDRU-7314",
        "Line of the report.\n".repeat(500)
    );
    let mut written = context.adapter.item(Role::Assistant, &report);
    written.complete = false;
    let command = context
        .adapter
        .item(Role::Tool, "git commit: 1 file changed");
    context
        .adapter
        .with("zeus", |agent| agent.items.extend([written, command]));
    context.adapter.answer("zeus", "Committed.");
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    let result = task.messages.iter().find(|m| m.kind == "result").unwrap();
    assert!(result.body.contains(&report), "the report, whole");
    assert!(result.body.contains("Committed."), "and the last word");
    assert!(
        !result.body.contains("git commit: 1 file changed"),
        "a tool's output is not the member's words"
    );
    held_to(
        context.close(),
        SUITES,
        "keeps everything a member wrote in its turn, not only its last message",
    );
}

#[test]
fn leaves_a_harnesss_commentary_out_of_a_result() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Review the commit");
    context.pass().unwrap();
    context.pass().unwrap();
    // Codex writes notes as it works, each marked as commentary, and ends
    // the turn with its final answer.
    let note = |text: &str| {
        let mut item = context.adapter.item(Role::Assistant, text);
        item.complete = false;
        item.commentary = true;
        item
    };
    let first = note("I'll read the commit, then check it on disk.");
    let tool = context.adapter.item(Role::Tool, "git show --stat");
    let second = note("The diff matches; checking the counts now.");
    context
        .adapter
        .with("zeus", |agent| agent.items.extend([first, tool, second]));
    context.adapter.answer("zeus", "PASS: no findings.");
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    let result = task.messages.iter().find(|m| m.kind == "result").unwrap();
    assert_eq!(result.body, "PASS: no findings.");
    held_to(
        context.close(),
        SUITES,
        "leaves a harness's commentary out of a result: Codex's progress notes are not its answer",
    );
}
