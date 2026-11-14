use cf_base::env::Env;
use cf_engine::roles::{role_instructions, role_instructions_for, with_staff_rules, ProjectSpecs};
use cf_proto::{cf_word::CfWord, specs::Variant};

use super::support::{assert_same_text, GOLDEN};

#[test]
fn own_hands_shares_the_commands_and_board_instructions_word_for_word() {
    let env = Env::default();
    let cf = CfWord::bare();
    let off = role_instructions_for(&env, "chief", &[], &cf, None, false).unwrap();
    let on = role_instructions_for(&env, "chief", &[], &cf, None, true).unwrap();
    for (start, end) in [
        ("## Your commands\n", "## What you do\n"),
        ("1. The human works with you here", "## What you never do\n"),
        ("## Choose the role, not the member\n", "## The staff\n"),
    ] {
        let section = |text: &str| {
            text.split_once(start)
                .unwrap()
                .1
                .split_once(end)
                .unwrap()
                .0
                .to_owned()
        };
        assert_eq!(section(&on), section(&off), "shared: {start}");
    }
    let continuation = "## The one exception: continuing a window\n";
    assert_eq!(
        on.split_once(continuation).unwrap().1,
        off.split_once(continuation).unwrap().1,
        "the continuation, reviews, tiers and staff stay shared"
    );
    assert!(on.contains("Everything below about the board (tasks, tiers, reviews, follow-ups) is for work the human explicitly asks you to put there: add no task, review or follow-up otherwise."));
}

#[test]
fn own_hands_specs_do_small_asks_and_phase_work_in_the_project_folder() {
    let specs = ProjectSpecs {
        variant: Variant::Core,
        folder: "plans",
        skill: "/skills/specmint/SKILL.md",
    };
    let role = |on| {
        role_instructions_for(
            &Env::default(),
            "chief",
            &[],
            &CfWord::bare(),
            Some(&specs),
            on,
        )
        .unwrap()
    };
    let off = role(false);
    let on = role(true);
    let section = |text: &str| {
        text.split_once("## This project's specs\n\n")
            .unwrap()
            .1
            .to_owned()
    };
    let off = section(&off);
    let on = section(&on);
    assert!(
        on.contains("A small ask is no spec: you do it yourself."),
        "{on}"
    );
    assert!(on.contains("Write the spec before you start its work, and do it in phase order. After you finish an item, mark the spec's progress as the skill says: its checkbox in SPEC.md and its count in the registry."), "{on}");
    assert!(!on.contains("it goes straight on the board as a task"));
    assert!(!on.contains("Then put them on the board in phase order"));
    assert!(off.contains("A small ask is no spec: it goes straight on the board as a task."));
    assert!(off.contains("Write the spec before any of its tasks go on the board. Then put them on the board in phase order:"));
}

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
    assert!(role.contains("Everything below about the board (tasks, tiers, reviews, follow-ups) is for work the human explicitly asks you to put there: add no task, review or follow-up otherwise."));
    let section = role.split_once("## This project's specs").unwrap().1;
    assert!(
        section.contains("Use a spec when the human asks for one, or when an ask is big enough")
    );
    assert!(section.contains("A small ask is no spec"));
    assert!(section
        .contains("What Specmint calls research, a subagent or implementation you do yourself"));
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
        assert!(own.contains("A tier may be served by a member's backup while that member is out of quota; a member on Vacation takes no new tasks, and you still name only the tier."));
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
            "is for work the human explicitly asks you to put there",
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
