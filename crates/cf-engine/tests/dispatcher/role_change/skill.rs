//! What a chief with specs on is told of its skill, and when: the copy of
//! Specmint where Codex has none is ConsensFlow's, kept at one place for every
//! launch, so a Resume that changes nothing says nothing; what does change (the
//! variant, or the human's own copy turning up) is said once.

use serde_json::Value;

use super::{chief_launches, codex, project_of, recorded, relaunch, replacements, specs_of, SKILL};
use cf_engine::testing::Context;
use cf_proto::specs::Variant;

/// The place the fake harness keeps ConsensFlow's copy of `variant`.
fn kept(variant: &str) -> String {
    format!(
        "/consensflow/integrations/specmint/fake/{variant}/.claude/skills/specmint-{variant}/SKILL.md"
    )
}

/// The chief of a project on Codex, which has no Specmint installed, and has been
/// told its role again once with specs on (core): its window answered.
fn told_once() -> (Context, i64, i64) {
    let context = codex();
    context.codex.specmint_installed.replace(None);
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    specs_of(&context, project.id, true, Variant::Core);
    relaunch(&context, project.id);
    context.pass().unwrap();
    context.codex.answer("chief", "Understood.");
    context.pass().unwrap();
    (context, project.id, chief)
}

#[test]
fn two_launches_of_a_chief_whose_specs_did_not_change_say_nothing_of_the_role_though_its_skill_is_a_copy_of_ours(
) {
    let (context, project, chief) = told_once();
    relaunch(&context, project);
    context.pass().unwrap();
    context.codex.answer("chief", "Still here.");
    context.pass().unwrap();
    relaunch(&context, project);

    let launches = chief_launches(&context.codex);
    assert_eq!(launches.len(), 4);
    assert_eq!(
        replacements(&context, chief).len(),
        1,
        "told at the first Resume with specs on, and never again"
    );
    for launch in &launches[2..] {
        assert!(launch["resume"].is_string(), "{launch}");
        assert_eq!(launch["message"], Value::Null, "nothing to say: {launch}");
    }
    // The role every launch is given names the one place the copy is kept.
    for launch in &launches[1..] {
        let text = launch["instructions"].as_str().unwrap();
        assert!(text.contains(&kept("core")), "{text}");
    }
    assert_eq!(launches[1]["instructions"], launches[3]["instructions"]);
    context.close();
}

#[test]
fn a_chief_whose_project_changed_the_variant_of_its_specs_is_told_the_role_again_with_the_new_skill_and_once(
) {
    let (context, project, chief) = told_once();
    specs_of(&context, project, true, Variant::Tdd);
    relaunch(&context, project);
    context.pass().unwrap();
    context.codex.answer("chief", "Understood.");
    context.pass().unwrap();
    relaunch(&context, project);

    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 2, "once for core and once for tdd");
    assert!(notes[0].body.contains(&kept("core")), "{}", notes[0].body);
    assert!(notes[1].body.contains(&kept("tdd")), "{}", notes[1].body);
    assert!(
        !notes[1].body.contains(&kept("core")),
        "the new note names the new skill alone"
    );
    assert_eq!(context.message(notes[1].id).state, "delivered");
    let launches = chief_launches(&context.codex);
    assert_eq!(launches.len(), 4);
    assert!(launches[2]["message"]
        .as_str()
        .is_some_and(|message| message.contains(&kept("tdd"))));
    assert_eq!(launches[3]["message"], Value::Null, "and given once");
    assert!(recorded(&context, chief)
        .as_deref()
        .is_some_and(|told| told.contains(&kept("tdd"))));
    context.close();
}

#[test]
fn a_skill_the_human_installs_after_the_chief_was_told_of_ours_is_a_change_and_told_once() {
    let (context, project, chief) = told_once();
    context
        .codex
        .specmint_installed
        .replace(Some(SKILL.to_owned()));
    relaunch(&context, project);
    context.pass().unwrap();
    context.codex.answer("chief", "Understood.");
    context.pass().unwrap();
    relaunch(&context, project);

    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 2);
    assert!(notes[1].body.contains(SKILL), "{}", notes[1].body);
    assert!(!notes[1].body.contains(&kept("core")));
    assert_eq!(chief_launches(&context.codex)[3]["message"], Value::Null);
    context.close();
}
