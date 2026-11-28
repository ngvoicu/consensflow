//! Where a question recognised at a turn's end goes once a lead gives tasks (the leads design,
//! section 3; step 4 of the build): to the decider of the task its window holds, the lead for a
//! lead's task. Last words that wait for the lead by its handle are a question on a lead's task,
//! and a result on the chief's, as they are today.

use cf_engine::testing::Context;
use cf_ledger::{NewLead, NewTask};

const QUESTION: &str =
    "Should I continue with the live runs, or stop here so you can review the findings first?";

/// A lead of the project, made as the ledger alone makes one until the page's step.
fn with_a_lead(context: &Context, project: i64) -> String {
    context
        .ledger
        .borrow_mut()
        .add_lead(
            project,
            &NewLead {
                harness: "claude-code".into(),
                model: None,
                effort: None,
                model_name: None,
                scope: "the site".into(),
                own_hands: false,
            },
        )
        .expect("a lead")
        .handle
}

/// A task `from` gave zeus by name.
fn gives(context: &Context, project: i64, from: &str, body: &str) -> i64 {
    context
        .create_task(
            project,
            NewTask {
                from: from.to_owned(),
                to: Some("zeus".to_owned()),
                body: body.to_owned(),
                ..NewTask::default()
            },
        )
        .task
        .number
}

/// The questions `handle` holds.
fn questions(context: &Context, project: i64, handle: &str) -> usize {
    context
        .inbox(context.id(project, handle))
        .into_iter()
        .filter(|message| message.kind == "question")
        .count()
}

#[test]
fn a_question_a_turn_ends_on_goes_to_the_lead_that_gave_the_task_and_the_chief_hears_none() {
    let words = [
        QUESTION,
        "The parser is half done. I'm waiting for your answer on the grammar.",
        "The parser is half done. Waiting for the chief's go.",
        "The parser is half done. Waiting for @lead-1's decision to go on.",
        "The parser is half done. Waiting on lead-1.",
    ];
    for said in words {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        let lead = with_a_lead(&context, project.id);
        context.pass().unwrap();
        let number = gives(&context, project.id, &lead, "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        context.adapter.answer("zeus", said);
        context.pass().unwrap();
        assert_eq!(
            context.task(project.id, number).task.state,
            "waiting",
            "{said}"
        );
        assert_eq!(
            (
                questions(&context, project.id, &lead),
                questions(&context, project.id, "chief"),
                questions(&context, project.id, "human"),
            ),
            (1, 0, 0),
            "{said}"
        );
    }
}

#[test]
fn the_chiefs_task_whose_last_words_wait_for_a_lead_by_name_has_them_as_its_result() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let lead = with_a_lead(&context, project.id);
    context.pass().unwrap();
    let number = gives(&context, project.id, "chief", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    context.adapter.answer(
        "zeus",
        "The parser is done. Waiting for @lead-1's decision to go on.",
    );
    context.pass().unwrap();
    assert_eq!(context.task(project.id, number).task.state, "done");
    assert_eq!(
        (
            questions(&context, project.id, &lead),
            questions(&context, project.id, "chief"),
        ),
        (0, 0)
    );
}

#[test]
fn a_question_a_turn_ends_on_for_the_chiefs_or_the_humans_task_goes_to_the_chief_with_a_lead_there()
{
    for requester in ["chief", "human"] {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        let lead = with_a_lead(&context, project.id);
        context.pass().unwrap();
        let number = gives(&context, project.id, requester, "Parser");
        context.pass().unwrap();
        context.pass().unwrap();
        context.adapter.answer("zeus", QUESTION);
        context.pass().unwrap();
        assert_eq!(
            context.task(project.id, number).task.state,
            "waiting",
            "{requester}"
        );
        assert_eq!(
            (
                questions(&context, project.id, "chief"),
                questions(&context, project.id, &lead),
            ),
            (1, 0),
            "{requester}"
        );
    }
}
