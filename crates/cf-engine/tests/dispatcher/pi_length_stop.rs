//! A Pi output-limit stop reaches the dispatcher's turn-end through the Pi
//! reader. The rig has no Pi stand-in, so this test gives its real reading to
//! the dispatcher's fake window, retaining the window's delivered task item.

use std::fs;
use std::time::UNIX_EPOCH;

use cf_base::env::Env;
use cf_engine::testing::Context;
use cf_harness::records::{self, Options, Reading, Role, Settlement};
use cf_ledger::{MemberEdit, NewTask};
use cf_proto::agents::Harness;
use serde_json::json;

use crate::fixtures::notes;

#[test]
fn a_pi_reply_cut_at_the_output_limit_fails_the_task_and_tells_the_chief_once() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let worker = context
        .project(project.id)
        .participants
        .into_iter()
        .find(|participant| participant.handle == "zeus")
        .unwrap();
    context
        .ledger
        .borrow_mut()
        .edit_member(
            project.id,
            "zeus",
            &MemberEdit {
                harness: "pi".to_owned(),
                model: None,
                effort: None,
                model_name: None,
                tier: worker.tier.unwrap(),
            },
        )
        .unwrap();
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Inspect the parser".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let task = context.task(project.id, 1).task;
    assert_eq!(task.state, "working");
    let handle = task.assignee.unwrap();
    let worker_id = context.id(project.id, &handle);
    assert_eq!(
        context.dispatcher.window_harness(worker_id).as_deref(),
        Some("pi")
    );
    context.open_window(project.id, &handle).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("made-session.jsonl");
    let rows = [
        json!({"type":"session", "version":3, "id":"made-session"}),
        json!({"type":"message", "id":"u1", "message": {"role":"user", "content":[{"type":"text", "text":"Inspect the parser"}]}}),
        json!({"type":"message", "id":"a1", "parentId":"u1", "message": {"role":"assistant", "content":[{"type":"thinking", "thinking":"Check the parser."},{"type":"text", "text":"The parser appears to"}], "stopReason":"length"}}),
    ];
    fs::write(
        &file,
        format!(
            "{}\n",
            rows.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )
    .unwrap();
    let written = i64::try_from(
        fs::metadata(&file)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let env = Env::from_vars([("PI_CODING_AGENT_SESSION_DIR", dir.path().to_str().unwrap())]);
    let zone = records::zone("UTC").unwrap();
    let early = records::answers(
        Harness::Pi,
        "made-session",
        &env,
        &Options::default(),
        &zone,
        written + 1_000,
    );
    let Reading::Known(record) = early.as_ref() else {
        panic!("Pi's made session was not read: {early:?}")
    };
    context.adapter.with("zeus", |agent| {
        agent.items.extend(
            record
                .items
                .iter()
                .filter(|item| item.role == Role::Assistant)
                .cloned(),
        );
        agent.settled = record.settlement == Settlement::Settled;
        agent.failed = record.failed;
        agent.failure = record.failure.clone();
        agent.failure_kind = record.failure_kind.clone();
    });
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "working",
        "Pi could still continue"
    );
    let reading = records::answers(
        Harness::Pi,
        "made-session",
        &env,
        &Options::default(),
        &zone,
        written + 121_000,
    );
    let Reading::Known(record) = reading.as_ref() else {
        panic!("Pi's made session was not read: {reading:?}")
    };
    assert!(
        record.failed && record.settlement == Settlement::Settled,
        "Pi's reading: {record:?}"
    );

    // The engine's fake Window uses the same settled/failed/reason fields as
    // Pi's Window::observe; its existing user item is the delivered task.
    context.adapter.with("zeus", |agent| {
        agent.settled = record.settlement == Settlement::Settled;
        agent.failed = record.failed;
        agent.failure = record.failure.clone();
        agent.failure_kind = record.failure_kind.clone();
    });
    context
        .host
        .set_snapshot(json!({"tail":["The parser appears to"]}));
    let launches = context.adapter.prepared().len();
    context.pass().unwrap();
    context.pass().unwrap();

    let thread = context.task(project.id, 1);
    assert_eq!(
        thread.task.state,
        "failed",
        "a cut reply ended the Pi turn; window: {:?}; thread: {thread:?}",
        context.adapter.agent("zeus")
    );
    assert!(thread
        .messages
        .iter()
        .all(|message| message.kind != "result"));
    assert_eq!(
        context.adapter.prepared().len(),
        launches,
        "the product did not retry"
    );
    let chief = context.id(project.id, "chief");
    let failures: Vec<_> = notes(&context, chief)
        .into_iter()
        .filter(|note| note.starts_with("T-1 failed:"))
        .collect();
    assert_eq!(failures, [format!("T-1 failed: @{handle}'s harness reported a failure: the member's reply was cut at the model's output limit; nothing was taken as its result. Reopen it with: cf task reopen T-1 \"…\", or accept it with what you checked: cf task accept T-1 \"…\"")]);
}
