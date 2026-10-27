//! A worker's conversation that another Claude shows beside its window: one the human
//! started outside ConsensFlow (`claude --resume <id>` in a terminal of their own). Its
//! turns are written into the same record as the window's, in records no different from
//! the window's, so an answer in it may be that one's, written after the task's message:
//! none is taken for the task's result. The task stays working, its requester is told
//! once, and Finish takes the answer they choose to.

use cf_engine::testing::Context;

/// The notes the chief was given that another Claude shows T-1's conversation.
fn told(context: &Context, project: i64) -> Vec<String> {
    context
        .task(project, 1)
        .messages
        .iter()
        .filter(|message| message.kind == "note" && message.body.contains("another Claude"))
        .map(|message| message.body.clone())
        .collect()
}

/// T-1 given to zeus, whose window took it; another Claude then shows its conversation.
fn shared() -> (Context, i64) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Land it");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.with("zeus", |agent| agent.shared = true);
    (context, project.id)
}

#[test]
fn an_answer_in_a_record_another_claude_writes_too_is_no_result_and_the_requester_is_told_once() {
    let (context, project) = shared();
    context.adapter.answer("zeus", "Landed.");
    context.pass().unwrap();
    context.pass().unwrap();
    let task = context.task(project, 1).task;
    assert_eq!(task.state, "working", "no result taken");
    assert_eq!(
        told(&context, project),
        ["T-1: another Claude than @zeus's window has shown its conversation (one started outside ConsensFlow, or an earlier window's that never ended). Its turns are written into the same record, and nothing tells them from the window's, so no answer there is taken for T-1's result. `cf task finish T-1` takes the last answer."]
    );
    context.advance(600_000);
    context.adapter.answer("zeus", "Landed again.");
    context.pass().unwrap();
    assert_eq!(told(&context, project).len(), 1, "told once");
    assert_eq!(context.task(project, 1).task.state, "working");
    let finished = context.finish_task(project, 1, "chief").unwrap();
    assert_eq!(finished.task.state, "done");
    let results: Vec<String> = context
        .task(project, 1)
        .messages
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect();
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(
        results[0].contains("Landed again.")
            && results[0].ends_with(
                "(ConsensFlow: Finished at @chief's word: what @zeus said last is the result.)"
            ),
        "{results:?}"
    );
}

#[test]
fn a_window_no_other_claude_shares_gives_its_answer_as_the_result() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Land it");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer("zeus", "Landed.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");
    assert!(told(&context, project.id).is_empty());
}
