//! The tasks the dispatcher gives out, and what it keeps of them: a brief
//! the human has not approved, a task that needs others, the copy of a
//! window's conversation, and a question a worker or a member asks the chief
//! (`describe('the dispatcher')`).

use std::sync::Arc;

use cf_harness::records::Role;
use cf_ledger::NewTask;
use serde_json::{json, Value};

use crate::matching::found;
use crate::traces::held_to;
use cf_engine::testing::Context;

const SUITES: &[&str] = &["the dispatcher"];

#[test]
fn opens_no_window_for_a_brief_the_human_has_not_approved_and_delivers_a_result_only_once_approved()
{
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context
        .ledger
        .borrow_mut()
        .set_gate(project.id, true)
        .unwrap();
    context.pool_task(project.id, "worker", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let thread = context.task(project.id, 1);
    let brief = thread.messages[0].clone();
    assert_eq!(
        (thread.task.state.as_str(), brief.state.as_str()),
        ("queued", "gated"),
        "assigned, but held for the human"
    );
    assert!(context.host.last("zeus").is_none(), "no window yet");
    context
        .ledger
        .borrow_mut()
        .approve_message(brief.id, "human")
        .unwrap();
    context.pass().unwrap();
    let launch = context.adapter.prepared().last().cloned().unwrap();
    assert_eq!(launch["participant"]["handle"], json!(brief.recipient));
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser done");
    context.pass().unwrap();
    let thread = context.task(project.id, 1);
    let result = thread.messages.iter().find(|m| m.kind == "result");
    let result = result.unwrap();
    assert_eq!(result.state, "gated");
    context.pass().unwrap();
    assert!(
        context.adapter.agent("chief").items.is_empty(),
        "the chief waits for the human"
    );
    context
        .ledger
        .borrow_mut()
        .approve_message(result.id, "human")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = context.id(project.id, "chief");
    assert_eq!(context.inbox(chief)[0].state, "delivered");
    let seen = context
        .adapter
        .agent("chief")
        .items
        .last()
        .cloned()
        .unwrap();
    assert!(found(
        r"result from @zeus-amber-pine\]\nParser done",
        &seen.text
    ));
    held_to(
        context.close(),
        SUITES,
        "opens no window for a brief the human has not approved, and delivers a result only once approved",
    );
}

#[test]
fn gives_out_a_task_only_once_every_task_it_needs_is_accepted_and_says_nothing_while_it_waits() {
    let context = Context::new();
    let project = context.with_staff(&["zeus", "diana"]);
    context.pool_task(project.id, "worker", "Lexer");
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Parser".to_owned(),
            needs: vec![1],
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let states = || -> Vec<String> {
        [1, 2]
            .map(|number| context.task(project.id, number).task.state)
            .to_vec()
    };
    assert_eq!(
        states(),
        ["working", "open"],
        "the parser waits for the lexer"
    );
    let chief = context.id(project.id, "chief");
    assert!(
        !context.inbox(chief).iter().any(|m| m.kind == "note"),
        "no \"waits for a free worker\" note: it waits for its need"
    );
    context.adapter.answer("zeus", "Lexer done");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(states(), ["done", "open"], "done is not accepted");
    context
        .ledger
        .borrow_mut()
        .accept_task(project.id, 1, "chief")
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(states(), ["accepted", "working"]);
    let launch = context.adapter.prepared().last().cloned().unwrap();
    assert!(found(
        r"T-2 · task from @chief\]\nParser$",
        launch["message"].as_str().unwrap()
    ));
    held_to(
        context.close(),
        SUITES,
        "gives out a task only once every task it needs is accepted, and says nothing while it waits",
    );
}

#[test]
fn keeps_its_own_copy_of_each_windows_conversation_item_by_item_as_it_grows() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let copied = || context.ledger.borrow().transcript(project.id, 1, None);
    let before = copied().unwrap();
    let heads: Vec<(String, String)> = before
        .items
        .iter()
        .map(|item| {
            let first = item.text.split('\n').next().unwrap_or_default();
            (item.role.clone(), first.to_owned())
        })
        .collect();
    assert_eq!(
        heads,
        [(
            "user".to_owned(),
            "[ConsensFlow m-1 · T-1 · task from @chief]".to_owned()
        )],
        "the brief, as the window got it"
    );
    let mut half = context.adapter.item(Role::Assistant, "Half");
    half.complete = false;
    context.adapter.with("zeus", |agent| {
        agent.items.push(half);
        agent.settled = false;
    });
    context.pass().unwrap();
    let half = copied().unwrap().items.pop().unwrap();
    assert_eq!(
        (half.role.as_str(), half.text.as_str(), half.complete),
        ("assistant", "Half", false)
    );
    context.adapter.with("zeus", |agent| {
        let last = agent.items.last_mut().unwrap();
        last.text = Arc::from("Half done, then all done");
        last.complete = true;
        agent.settled = true;
    });
    context.pass().unwrap();
    let all = copied().unwrap();
    assert_eq!(all.total, 2);
    assert_eq!(
        (all.items[1].text.as_str(), all.items[1].complete),
        ("Half done, then all done", true)
    );
    assert_eq!(
        context.task(project.id, 1).task.state,
        "done",
        "and the result was collected"
    );
    held_to(
        context.close(),
        SUITES,
        "keeps its own copy of each window's conversation, item by item, as it grows",
    );
}

#[test]
fn leaves_a_task_waiting_on_a_question_alone_and_resumes_it_with_the_answer() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let question = context.ask(project.id, "zeus", "chief", 1, "Which format?");
    context.adapter.answer("zeus", "I asked the chief.");
    context.pass().unwrap();
    assert_eq!(
        context.task(project.id, 1).task.state,
        "waiting",
        "no result from a waiting turn"
    );

    context.pass().unwrap();
    context.adapter.answer("chief", "JSON, I will reply");
    context
        .ledger
        .borrow_mut()
        .answer(
            question.id,
            question.recipient_id,
            Some(&json!("JSON")),
            None,
        )
        .unwrap();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.answer("zeus", "Parser done, in JSON");
    context.pass().unwrap();
    let task = context.task(project.id, 1);
    assert_eq!(task.task.state, "done");
    assert_eq!(task.messages.last().unwrap().body, "Parser done, in JSON");
    held_to(
        context.close(),
        SUITES,
        "leaves a task waiting on a question alone, and resumes it with the answer",
    );
}

/// What a window's conversation holds, as text on lines.
fn saw(context: &Context, handle: &str) -> String {
    let items = context.adapter.agent(handle).items;
    let texts: Vec<&str> = items.iter().map(|item| &*item.text).collect();
    texts.join("\n")
}

#[test]
fn carries_an_advisors_and_a_reviewers_question_to_the_chief_and_the_answer_back_to_their_own_window(
) {
    let context = Context::new();
    let member = |agent: &str, role: &str| -> Value {
        json!({ "agent": agent, "harness": "claude-code", "role": role, "tier": "standard" })
    };
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [member("athena", "advisor"), member("calliope", "reviewer")],
        }))
        .unwrap();
    let task = |number: i64| context.task(project.id, number);
    for (pool, agent, body) in [
        ("advisor", "athena", "Which law applies to the page?"),
        ("reviewer", "calliope", "Review the legislation page"),
    ] {
        let created = context.pool_task(project.id, pool, body).task;
        context.pass().unwrap();
        context.pass().unwrap();
        let session = task(created.number).task.assignee.unwrap();
        assert!(
            found(&format!("^{agent}-"), &session),
            "{pool}: its own session"
        );
        let question = context.ask(
            project.id,
            &session,
            "chief",
            created.number,
            &format!("{pool}: which audience, managers or HR?"),
        );
        context.adapter.answer(agent, "I asked the chief.");
        context.pass().unwrap();
        assert_eq!(
            task(created.number).task.state,
            "waiting",
            "{pool}: waits for the answer, no result"
        );
        // The chief's window gets the question, answers it.
        context.pass().unwrap();
        assert!(found(
            &format!(r"question from @{session}\]\n{pool}: which audience"),
            &saw(&context, "chief")
        ));
        context.adapter.answer("chief", "Answered.");
        context
            .ledger
            .borrow_mut()
            .answer(
                question.id,
                question.recipient_id,
                Some(&json!("Managers first.")),
                None,
            )
            .unwrap();
        context.pass().unwrap();
        context.pass().unwrap();
        assert!(
            found(
                r"answer from @chief\]\nManagers first\.",
                &saw(&context, agent)
            ),
            "{pool}: the answer in its own window"
        );
        assert_eq!(task(created.number).task.state, "working");
        context
            .adapter
            .answer(agent, &format!("{pool} findings, for managers"));
        context.pass().unwrap();
        assert_eq!(task(created.number).task.state, "done");
        assert_eq!(
            task(created.number).messages.last().unwrap().body,
            format!("{pool} findings, for managers")
        );
        // The chief takes one message at a time: it reads the result first.
        context.pass().unwrap();
        context.adapter.answer("chief", "Read.");
        context.pass().unwrap();
    }
    held_to(
        context.close(),
        SUITES,
        "carries an advisor’s and a reviewer’s question to the chief and the answer back to their own window",
    );
}
