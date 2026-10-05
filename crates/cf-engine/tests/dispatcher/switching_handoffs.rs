//! Switching the chief to another agent, while a handoff is still on its
//! way: a switch replaces the one the last switch wrote, whether the closed
//! project's Resume brings it back or a ledger from before 2026-10-03 titled
//! it for the lead.

use cf_engine::SwitchWhen;
use cf_ledger::ChiefSwitch;

use crate::chiefs::{assert_match, handoffs_of, to, with_codex};
use crate::traces::held_to;

const SUITES: &[&str] = &["switching the chief to another agent"];

#[test]
fn replaces_a_handoff_still_on_its_way_and_resume_of_a_closed_project_brings_the_chief_back_with_the_handoff_it_had_not_shown(
) {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    // The new window has not shown its handoff yet when the human switches again.
    context.codex.with("chief", |agent| agent.items.clear());
    let first = context
        .ledger
        .borrow()
        .inbox(chief, 100)
        .unwrap()
        .into_iter()
        .find(|message| message.body.starts_with("You are the chief now"))
        .unwrap();
    context
        .switch_chief(
            project.id,
            to("claude-code", "apollo"),
            SwitchWhen::Now,
            false,
        )
        .unwrap();
    assert_eq!(
        context.message(first.id).state,
        "cancelled",
        "that handoff was for Codex"
    );
    let handoffs: Vec<_> = handoffs_of(&context, project.id)
        .into_iter()
        .filter(|message| message.state != "cancelled")
        .collect();
    assert_eq!(handoffs.len(), 1);
    assert_match(
        &handoffs[0].body,
        r"from Codex \(astraeus\) to you, Claude Code \(apollo\)\.",
    );

    // Closed before any look saw the new chief show it: the handoff waits for Resume.
    context.driver().close_project(project.id).unwrap();
    context.driver().resume_project(project.id).unwrap();
    let prepared = context.adapter.prepared().last().cloned().unwrap();
    assert_match(
        prepared["message"].as_str().unwrap(),
        r"You are the chief now\. The human switched this project's chief from Codex \(astraeus\) to you, Claude Code \(apollo\)\.",
    );
    held_to(
        context.close(),
        SUITES,
        "replaces a handoff still on its way, and Resume of a closed project brings the chief back with the handoff it had not shown",
    );
}

#[test]
fn takes_a_handoff_titled_for_the_lead_as_a_ledger_from_before_2026_10_03_holds_it_as_the_handoff_it_goes_first_no_other_is_written_and_a_switch_replaces_it(
) {
    let context = with_codex();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.answer("chief", "Hello");
    context.pass().unwrap();
    // The build before switched the chief and wrote its handoff, and the
    // project closed before the new window showed it.
    context.driver().close_project(project.id).unwrap();
    context
        .ledger
        .borrow_mut()
        .switch_chief(
            project.id,
            &ChiefSwitch {
                harness: "codex".to_owned(),
                agent: "astraeus".to_owned(),
                cut: false,
            },
        )
        .unwrap();
    let old = context.note(
        project.id,
        None,
        "chief",
        "You are the lead now. The human switched this project's lead from Claude Code (apollo) to you, Codex (astraeus).",
    );

    context.driver().resume_project(project.id).unwrap();
    let launched = context.codex.prepared().last().cloned().unwrap();
    assert_match(
        launched["message"].as_str().unwrap(),
        r"\nYou are the lead now\.",
    );
    assert!(
        handoffs_of(&context, project.id).is_empty(),
        "no other is written"
    );

    // The window has not shown it yet when the human switches again.
    context.codex.with("chief", |agent| agent.items.clear());
    context
        .switch_chief(
            project.id,
            to("claude-code", "apollo"),
            SwitchWhen::Now,
            false,
        )
        .unwrap();
    assert_eq!(
        context.message(old.id).state,
        "cancelled",
        "that handoff was for Codex"
    );
    assert_eq!(handoffs_of(&context, project.id).len(), 1);
    let prepared = context.adapter.prepared().last().cloned().unwrap();
    assert_match(
        prepared["message"].as_str().unwrap(),
        r"You are the chief now\. The human switched this project's chief from Codex \(astraeus\) to you, Claude Code \(apollo\)\.",
    );
    held_to(
        context.close(),
        SUITES,
        "takes a handoff titled for the lead, as a ledger from before 2026-10-03 holds it, as the handoff: it goes first, no other is written, and a switch replaces it",
    );
}
