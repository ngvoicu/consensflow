use cf_base::env::Env;
use cf_catalog::WorkTier;

use super::*;
use crate::roles::{role_instructions, StaffRow};

const CF: &str = "/opt/consensflow/bin/cf";

fn staff() -> Vec<StaffRow> {
    vec![StaffRow {
        name: "zeus".to_owned(),
        roles: vec!["worker".to_owned()],
        work_tier: Some(WorkTier::Standard),
    }]
}

fn given(variant: Variant) -> ProjectSpecs<'static> {
    ProjectSpecs {
        variant,
        folder: "docs/specs",
        skill: "/home/me/.consensflow/integrations/claude/launch/role/.claude/skills/specmint-tdd/SKILL.md",
    }
}

fn text(role: &str, specs: Option<&ProjectSpecs<'_>>) -> String {
    role_instructions(&Env::default(), role, &staff(), Some(CF), specs).unwrap()
}

#[test]
fn with_specs_off_a_chief_s_text_holds_no_word_of_them_and_is_the_text_it_was() {
    let chief = text("chief", None);
    for word in ["spec", "Spec", "Specmint", "phase", "--needs `"] {
        assert!(
            !chief.contains(word),
            "{word:?} in a chief's text with specs off"
        );
    }
    // The text is chief.md, the staff, and the note of this window's cf, in that order.
    assert!(chief.ends_with(&format!(
        "\n## This window's cf\n\nHere `cf` is {CF}. If `cf` says a command is unknown, or answers as another program, another `cf` comes first on this shell's PATH: run {CF} instead.\n"
    )));
    assert!(chief.contains("| zeus | worker | "));
}

#[test]
fn a_chief_s_text_with_specs_on_has_one_section_more_before_its_cf_and_the_rest_is_the_same() {
    let base = text("chief", None);
    let specs = given(Variant::Tdd);
    let with = text("chief", Some(&specs));
    let (before_cf, cf) = base.split_once("\n## This window's cf").unwrap();
    assert_eq!(
        with,
        format!("{before_cf}{}\n## This window's cf{cf}", specs.section())
    );
    assert_eq!(with.matches("## This project's specs").count(), 1);
}

#[test]
fn the_section_names_the_skill_where_its_file_is_the_folder_and_where_specmint_says_dot_specs() {
    for variant in Variant::ALL {
        let section = given(variant).section();
        assert!(
            section.starts_with("\n## This project's specs\n\n"),
            "{section}"
        );
        assert!(section.ends_with('\n'));
        assert!(
            section.contains(&format!("`{}`", variant.skill_name())),
            "{section}"
        );
        assert!(
            section.contains(
                "its SKILL.md is at /home/me/.consensflow/integrations/claude/launch/role/.claude/skills/specmint-tdd/SKILL.md"
            ),
            "{section}"
        );
        assert!(
            section.contains("Where Specmint says `.specs`, it means `docs/specs`"),
            "{section}"
        );
        // The live test of specs reads the skill's file and the folder out of these
        // words (`crates/cf-e2e/tests/live_specs/launched.rs`): a change of them is
        // a change of its patterns.
        assert!(
            section.contains("/SKILL.md, and the files it names are beside it;"),
            "{section}"
        );
    }
}

#[test]
fn the_section_says_when_to_use_a_spec_what_goes_to_the_board_and_how_a_spec_becomes_tasks() {
    let section = given(Variant::Core).section();
    for said in [
        // When: the human asks, or the ask is big enough for phases; a small ask goes straight on the board.
        "when the human asks for one, or when an ask is big enough that its work comes in phases",
        "A small ask is no spec: it goes straight on the board as a task",
        // Writing the spec is the chief's own work, the second exception to changing nothing
        // itself, and comes before its tasks; research, subagents and implementation are the board's.
        "The spec is a second exception to changing nothing yourself",
        "you write the spec and its registry in `docs/specs` with your own tools, and never give that writing to the staff",
        "Write the spec before any of its tasks go on the board",
        "Everything Specmint calls research, a subagent or implementation goes on the board as tasks for the staff",
        // From a spec: phase order, `--needs`, the spec item in each brief, progress marked after an acceptance.
        "in phase order",
        "`--needs`",
        "names the spec item it comes from",
        "After you accept a task, mark the spec's progress",
    ] {
        assert!(section.contains(said), "{said:?} in {section}");
    }
    assert!(
        section.lines().count() < 14,
        "a few lines, not a manual: {section}"
    );
}

#[test]
fn a_member_s_text_is_never_given_the_section_whatever_it_is_given() {
    let specs = given(Variant::Core);
    for role in ["advisor", "worker", "reviewer", "tester", "designer"] {
        let without = text(role, None);
        assert_eq!(text(role, Some(&specs)), without, "{role}");
        assert!(!without.contains("This project's specs"), "{role}");
    }
}
