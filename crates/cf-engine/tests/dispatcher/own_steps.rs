//! The chief's own step (`cf task add --self`), finished by its word while its
//! brief was being pasted back into the chief's window (the field report of
//! 2026-10-15, B4): the paste refused, the brief goes no further, never pasted
//! again for a step already accepted, and nobody hears of a failure (the second
//! review, 2026-10-16). No Node recording holds it: Node refused the finish.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_ledger::NewTask;

use crate::fixtures::given;
use crate::work_in_flight::looks_at;

#[test]
fn a_brief_being_pasted_when_the_chief_finished_its_step_is_withdrawn_at_the_refusal_and_never_pasted(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    let launch = context.adapter.agent("chief").launch;
    let held = context.adapter.deliver_holds.hold(looks_at(launch));
    context.create_task(
        project.id,
        NewTask {
            from: "chief".to_owned(),
            to: Some("chief".to_owned()),
            body: "Check the parser".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    let brief = context.task(project.id, 1).messages[0].id;
    assert_eq!(context.message(brief).state, "delivering");
    context
        .ledger
        .borrow_mut()
        .record_result(project.id, 1, "Checked: it holds")
        .unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "accepted");

    context.adapter.with("chief", |agent| agent.admit = false);
    held.open();
    context.settle();
    let withdrawn = context.message(brief);
    assert_eq!(
        (withdrawn.state.as_str(), withdrawn.reason.as_deref()),
        (
            "cancelled",
            Some("T-1 was done before it came back to @chief")
        ),
        "withdrawn at the refusal, in the finish's words"
    );
    context.adapter.with("chief", |agent| agent.admit = true);
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(
        !given(&context, "chief")
            .iter()
            .any(|item| item.contains(&marker_of(brief))),
        "never pasted for a step accepted"
    );
    let human = context.id(project.id, "human");
    assert!(
        context.inbox(human).is_empty(),
        "nobody hears of a failure: {:?}",
        context.inbox(human)
    );
}
