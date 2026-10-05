//! A member whose saved agent is gone from the human's agents runs on no
//! default: it takes no work, and what it held goes back to the board.

use cf_engine::testing::Context;

use crate::fixtures::{assert_match, Tiers};
use crate::traces::held_to;

const SUITES: &[&str] = &["a member whose saved agent is gone"];

/// Every agent but `gone` is saved, on the model `m`.
fn without(gone: &str) -> Context {
    let context = Context::new();
    context.roster.model.replace(Some("m".to_owned()));
    context.roster.gone.borrow_mut().insert(gone.to_owned());
    context
}

#[test]
fn gives_a_member_whose_agent_is_gone_no_work_the_task_waits_and_the_chief_hears_why() {
    let context = without("zeus");
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let task = tiers.task(1).task;
    assert_eq!((task.state.as_str(), task.assignee), ("open", None));
    assert_match(
        tiers.notes("chief").last().unwrap(),
        r"T-1 waits for a free standard worker: @zeus has no agent any more \(zeus is not among your agents: define it, or remove the member\)",
    );
    assert_eq!(
        context.host.opened().len(),
        1,
        "only the chief window opened"
    );
    held_to(
        context.close(),
        SUITES,
        "gives a member whose agent is gone no work: the task waits and the chief hears why",
    );
}

#[test]
fn after_a_release_drops_the_agent_of_a_working_member_a_restart_sends_its_task_back_to_the_board()
{
    let context = Context::new();
    context.roster.model.replace(Some("m".to_owned()));
    let tiers = Tiers::new(&context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let task = tiers.task(1).task;
    assert_eq!(
        (task.state.as_str(), task.assignee.as_deref()),
        ("working", Some("zeus-amber-pine"))
    );
    // The human installs a release without zeus's entry and the daemon starts again.
    context.roster.gone.borrow_mut().insert("zeus".to_owned());
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.resume_after_restart().unwrap();
    after.pass().unwrap();
    let task = tiers.task(1).task;
    assert_eq!((task.state.as_str(), task.assignee), ("open", None));
    assert_match(
        &task.body,
        r"Reassigned from @zeus-amber-pine \(zeus is no longer among your agents\)",
    );
    after.pass().unwrap();
    assert_match(
        tiers.notes("chief").last().unwrap(),
        "waits for a free standard worker: @zeus has no agent any more",
    );
    assert_eq!(
        context.host.opened().len(),
        3,
        "the chief before and after the restart, zeus before it, nothing for zeus after"
    );
    held_to(
        context.close(),
        SUITES,
        "after a release drops the agent of a working member, a restart sends its task back to the board",
    );
}
