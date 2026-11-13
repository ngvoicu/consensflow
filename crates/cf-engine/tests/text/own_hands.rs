use cf_base::env::Env;
use cf_engine::roles::{role_instructions, role_instructions_for, with_staff_rules, ProjectSpecs};
use cf_proto::{cf_word::CfWord, specs::Variant};

use super::support::{assert_same_text, GOLDEN};

#[test]
fn small_board_work_does_not_require_a_spec_with_own_hands() {
    let specs = ProjectSpecs {
        variant: Variant::Core,
        folder: "plans",
        skill: "/skills/specmint/SKILL.md",
    };
    let role = role_instructions_for(
        &Env::default(),
        "chief",
        &[],
        &CfWord::bare(),
        Some(&specs),
        true,
    )
    .unwrap();
    let section = role.split_once("## This project's specs").unwrap().1;
    assert!(
        section.contains("Use a spec when the human asks for one, or when an ask is big enough")
    );
    assert!(section.contains("For work with a spec that the human asks to put on the board"));
    assert!(!section.contains("For work the human asks to put on the board, write the spec first"));
}

#[test]
fn own_hands_changes_only_the_chief_and_specs_follow_the_switch() {
    let env = Env::default();
    let cf = CfWord::bare();
    let specs = ProjectSpecs {
        variant: Variant::Core,
        folder: "plans",
        skill: "/skills/specmint/SKILL.md",
    };
    for choice in [None, Some(&specs)] {
        let ordinary = role_instructions(&env, "chief", &[], &cf, choice).unwrap();
        assert_eq!(
            role_instructions_for(&env, "chief", &[], &cf, choice, false).unwrap(),
            ordinary
        );
        let own = role_instructions_for(&env, "chief", &[], &cf, choice, true).unwrap();
        assert!(
            own.len() < ordinary.len(),
            "the own-hands role stays shorter"
        );
        let suffix = if choice.is_some() {
            "specs_on"
        } else {
            "specs_off"
        };
        for (state, text) in [("off", &ordinary), ("on", &own)] {
            let key = format!("{state}_{suffix}");
            assert_same_text(text, GOLDEN["ownHandsChief"][&key].as_str().unwrap(), &key);
        }
        for words in [
            "only when the human explicitly asks",
            "project's own folder",
            "no worktree or new branch",
            "Commit or push when",
            "## Reviews",
            "## The staff",
            "task add --review",
            "task accept",
        ] {
            assert!(own.contains(words), "{words}: {own}");
        }
        assert!(!own.contains("You do not change the project yourself"));
        assert!(!own.contains("Every change is a task"));
        assert!(!own.contains("Ask them here in your terminal for the members your work needs"));
        if choice.is_some() {
            assert!(own.contains(
                "What Specmint calls research, a subagent or implementation you do yourself"
            ));
            assert!(own.contains("unless the human explicitly asks to put that work on the board"));
            assert!(
                !own.contains("Everything Specmint calls research beyond searching and reading")
            );
        } else {
            assert!(!own.contains("## This project's specs"));
        }
        let rules = Some("The person's exact rules.");
        let own_rules = with_staff_rules(own, "chief", rules);
        let ordinary_rules = with_staff_rules(ordinary, "chief", rules);
        let heading = "## The project's rules for the staff";
        assert_eq!(
            own_rules.split_once(heading).unwrap().1,
            ordinary_rules.split_once(heading).unwrap().1
        );
    }
    let ordinary = role_instructions(&env, "worker", &[], &cf, None).unwrap();
    assert_eq!(
        role_instructions_for(&env, "worker", &[], &cf, None, true).unwrap(),
        ordinary
    );
}
