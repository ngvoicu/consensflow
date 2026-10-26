//! A ready-made agent a release took out of the catalog (Khnum, Devin's
//! SWE-1.6 Slow, 2026-10-25: Devin no longer offers it). A project still
//! staffed with it, or whose chief runs on it, breaks nothing silently: its
//! member takes no work and its chief no window, as for any agent that is
//! gone, and what the human reads says it left the catalog, why, and to pick
//! another agent: defining it again would run a model its harness no longer
//! has.

use cf_engine::testing::Context;
use cf_engine::SwitchWhen;

use crate::chiefs::{to, to_human, with_codex};
use crate::fixtures::{assert_match, Tiers};

const WHY: &str = "Devin no longer offers SWE-1.6 Slow";

/// Every agent but Khnum is saved, on the model `m`; Khnum left the catalog.
fn after_the_release(context: &Context) {
    context.roster.gone.borrow_mut().insert("khnum".to_owned());
    context
        .roster
        .retired
        .borrow_mut()
        .insert("khnum".to_owned(), WHY.to_owned());
}

#[test]
fn a_member_on_an_agent_the_catalog_dropped_takes_no_work_and_the_chief_hears_it_left_and_to_pick_another(
) {
    let context = Context::new();
    context.roster.model.replace(Some("m".to_owned()));
    after_the_release(&context);
    let tiers = Tiers::new(&context, &["khnum"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let task = tiers.task(1).task;
    assert_eq!((task.state.as_str(), task.assignee), ("open", None));
    assert_match(
        tiers.notes("chief").last().unwrap(),
        r"T-1 waits for a free standard worker: @khnum has no agent any more \(khnum left the catalog: Devin no longer offers SWE-1\.6 Slow; remove @khnum from the staff and add another agent\)",
    );
    assert_eq!(
        context.host.opened().len(),
        1,
        "only the chief window opened"
    );
}

#[test]
fn a_task_khnum_held_before_the_release_goes_back_to_the_board_saying_it_left_the_catalog() {
    let context = Context::new();
    context.roster.model.replace(Some("m".to_owned()));
    let tiers = Tiers::new(&context, &["khnum"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let task = tiers.task(1).task;
    assert_eq!(
        (task.state.as_str(), task.assignee.as_deref()),
        ("working", Some("khnum-amber-pine"))
    );
    // The human installs the release without Khnum and the daemon starts again.
    after_the_release(&context);
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(tiers.project.id).unwrap();
    after.pass().unwrap();
    let task = tiers.task(1).task;
    assert_eq!((task.state.as_str(), task.assignee), ("open", None));
    assert_match(
        &task.body,
        r"Reassigned from @khnum-amber-pine \(khnum left the catalog: Devin no longer offers SWE-1\.6 Slow\)",
    );
    after.pass().unwrap();
    assert_match(
        tiers.notes("chief").last().unwrap(),
        "@khnum has no agent any more \\(khnum left the catalog",
    );
}

#[test]
fn a_chief_on_an_agent_the_catalog_dropped_stays_closed_and_the_human_is_told_to_switch_it() {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    // The stand-ins open Codex windows, not Devin's: what is told is the agent's, whatever its harness.
    context
        .switch_chief(project.id, to("codex", "khnum"), SwitchWhen::Now, false)
        .unwrap();
    context.close_project(project.id).unwrap();
    after_the_release(&context);
    let opened = context.host.opened().len();
    context.resume_project(project.id).unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.host.opened().len(),
        opened,
        "no window on a model its harness no longer has"
    );
    let told = to_human(&context, project.id)
        .into_iter()
        .find(|body| body.contains("left the catalog"))
        .unwrap();
    assert_eq!(
        told,
        "The chief runs on khnum, which left the catalog: Devin no longer offers SWE-1.6 Slow. Switch the chief to another agent."
    );
    // The human switches it to another agent, and the chief opens on that one.
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    context.pass().unwrap();
    assert!(
        context.host.opened().len() > opened,
        "the chief opens again"
    );
}
