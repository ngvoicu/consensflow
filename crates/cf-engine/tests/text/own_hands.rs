use cf_base::env::Env;
use cf_engine::roles::{
    guide_instructions, role_instructions, role_instructions_for, with_staff_rules, ProjectSpecs,
    StaffRow,
};
use cf_proto::{cf_word::CfWord, specs::Variant};

use super::support::{assert_same_text, GOLDEN};

const SHORT: &str = include_str!("../../../../skill/core/chief-own-hands.md");
const CHIEF: &str = include_str!("../../../../skill/core/chief.md");

fn own(cf: &CfWord, specs: Option<&ProjectSpecs<'_>>) -> String {
    role_instructions_for(&Env::default(), "chief", &[], cf, specs, true).unwrap()
}

fn specs() -> ProjectSpecs<'static> {
    ProjectSpecs {
        variant: Variant::Core,
        folder: "plans",
        skill: "/skills/specmint/SKILL.md",
    }
}

/// `text` without the wording between two markers, markers included.
fn without(text: &str, start: &str, end: &str) -> String {
    let (before, rest) = text.split_once(start).unwrap();
    let (_, after) = rest.split_once(end).unwrap();
    format!("{before}{after}")
}

#[test]
fn own_hands_gives_a_short_text_that_is_whole_in_itself() {
    let cf = CfWord::bare();
    let text = own(&cf, None);
    let flat = text.replace('\n', " ");
    let written = text.lines().filter(|line| !line.is_empty()).count();
    assert!(written < 20, "{written} lines: {text}");
    assert!(text.starts_with("---\nname: consensflow-chief\ndescription: "));
    for words in [
        "never run `cf`",
        "Use it only when the human\nexplicitly asks",
        "run `cf guide`",
        "begins `[ConsensFlow m-… · T-… · …]`",
        "Work already on the board\ngoes on, and its results come to you",
        "turn \"Do with your own hands\" off on your pane, and you are told",
    ] {
        assert!(flat.contains(&words.replace('\n', " ")), "{words}: {text}");
    }
    // How to work is the harness's own: the text says nothing of it.
    for words in [
        "coding agent",
        "worktree",
        "branch",
        "commit",
        "push",
        "folder",
    ] {
        assert!(!text.contains(words), "{words}: {text}");
    }
    // What the board's guide holds is not in front of it.
    for words in [
        "task add",
        "## The staff",
        "## Reviews",
        "{{",
        "The project's rules for the staff",
    ] {
        assert!(!text.contains(words), "{words}: {text}");
    }
    assert!(text.contains(
        SHORT
            .split_once("<!-- guide:")
            .unwrap()
            .0
            .split("---\n\n")
            .nth(1)
            .unwrap()
            .lines()
            .next()
            .unwrap()
    ));
}

#[test]
fn the_short_text_names_the_window_s_own_cf() {
    let cf =
        CfWord::of_word("/Applications/ConsensFlow.app/Contents/Resources/cli/bin/cf").unwrap();
    let text = own(&cf, None);
    assert!(
        text.contains("run `/Applications/ConsensFlow.app/Contents/Resources/cli/bin/cf guide`")
    );
    assert!(!text.contains("{{cf}}"));
}

#[test]
fn the_staff_s_rules_are_not_in_the_short_text_and_the_specs_stay() {
    let cf = CfWord::bare();
    let text = own(&cf, Some(&specs()));
    let section = text.split_once("## This project's specs\n\n").unwrap().1;
    assert!(section.contains("A small ask is no spec: you do it yourself."));
    assert!(section.contains("What Specmint calls research or implementation you do yourself"));
    assert!(!own(&cf, None).contains("## This project's specs"));
}

#[test]
fn the_guide_is_the_ordinary_text_with_its_four_parts_said_for_the_board() {
    let cf = CfWord::bare();
    let ordinary = role_instructions(&Env::default(), "chief", &[], &cf, None).unwrap();
    let guide = guide_instructions(&[], &cf, None).unwrap();
    // Without the frontmatter and the three parts that differ, the two are one text.
    let ordinary_rest = ordinary.split_once("\n---\n\n").unwrap().1;
    let ordinary_rest = ordinary_rest
        .replacen(
            part(
                CHIEF,
                "<!-- own_hands: opening -->\n",
                "<!-- own_hands: end_opening -->\n",
            ),
            "",
            1,
        )
        .replacen(
            part(
                CHIEF,
                "<!-- own_hands: duty -->\n",
                "<!-- own_hands: end_duty -->\n",
            ),
            "",
            1,
        )
        .replacen(
            part(
                CHIEF,
                "<!-- own_hands: never -->\n",
                "<!-- own_hands: end_never -->\n",
            ),
            "",
            1,
        );
    let guide_rest = guide
        .replacen(&format!("{}\n", guided("opening")), "", 1)
        .replacen(&format!("{}\n", guided("duty")), "", 1)
        .replacen(&format!("{}\n", guided("never")), "", 1);
    // The ordinary text names the frontmatter's heading once, and so does the guide.
    assert_eq!(guide_rest, ordinary_rest);
    assert!(guide.starts_with("# ConsensFlow Chief of Staff\n\nThis is the guide to the board"));
    assert!(guide.contains("## The staff"));
    assert!(guide.contains("task add --review"));
    assert!(!guide.contains("---\nname:"));
    assert!(!guide.contains("You do not change the project yourself"));
}

/// The text between `start` and `end` in `text`, each line end included.
fn part<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    text.split_once(start).unwrap().1.split_once(end).unwrap().0
}

/// What the short text's file says in the guide's part `name`, as the guide has it.
fn guided(name: &str) -> &'static str {
    let marker = format!("<!-- guide: {name} -->\n");
    let rest = SHORT.split_once(marker.as_str()).unwrap().1;
    let end = rest.find("<!-- guide:").unwrap_or(rest.len());
    rest[..end].trim_end_matches('\n')
}

#[test]
fn the_guide_has_the_staff_the_tiers_the_window_s_cf_and_the_staff_s_rules() {
    let cf = CfWord::of_word("/opt/cf/bin/cf").unwrap();
    let staff = [StaffRow {
        name: "crisp-comet".to_owned(),
        model: None,
        roles: vec!["worker".to_owned()],
        work_tier: Some(cf_proto::agents::WorkTier::Standard),
    }];
    let guide = guide_instructions(&staff, &cf, Some("The person's exact rules.")).unwrap();
    assert!(guide.contains("crisp-comet"));
    assert!(guide.contains("/opt/cf/bin/cf task add --tier"));
    assert!(!guide.contains("{{"));
    let heading = "## The project's rules for the staff";
    assert!(guide.contains(heading));
    assert!(guide.ends_with("The person's exact rules.\n"));
    // The same rules come after the ordinary chief's text as they do after the guide.
    let ordinary = with_staff_rules(
        role_instructions(&Env::default(), "chief", &staff, &cf, None).unwrap(),
        "chief",
        Some("The person's exact rules."),
    );
    assert_eq!(
        guide.split_once(heading).unwrap().1,
        ordinary.split_once(heading).unwrap().1
    );
    assert_eq!(
        guide_instructions(&staff, &cf, None).unwrap(),
        guide.split_once(&format!("\n{heading}")).unwrap().0
    );
}

#[test]
fn the_ordinary_chief_is_what_it_was_and_the_goldens_say_so() {
    let cf = CfWord::bare();
    for (choice, key) in [(None, "off_specs_off"), (Some(specs()), "off_specs_on")] {
        let ordinary =
            role_instructions(&Env::default(), "chief", &[], &cf, choice.as_ref()).unwrap();
        assert_eq!(
            role_instructions_for(&Env::default(), "chief", &[], &cf, choice.as_ref(), false)
                .unwrap(),
            ordinary
        );
        assert_same_text(
            &ordinary,
            GOLDEN["ownHandsChief"][key].as_str().unwrap(),
            key,
        );
        assert!(!ordinary.contains("guide"), "{key}");
    }
    // The ordinary text keeps the three parts of `chief.md` as the file has them.
    let ordinary = role_instructions(&Env::default(), "chief", &[], &cf, None).unwrap();
    for (start, end) in [
        (
            "<!-- own_hands: opening -->\n",
            "<!-- own_hands: end_opening -->\n",
        ),
        (
            "<!-- own_hands: duty -->\n",
            "<!-- own_hands: end_duty -->\n",
        ),
        (
            "<!-- own_hands: never -->\n",
            "<!-- own_hands: end_never -->\n",
        ),
    ] {
        assert!(ordinary.contains(part(CHIEF, start, end)));
    }
    assert!(
        without(
            CHIEF,
            "<!-- own_hands: opening -->\n",
            "<!-- own_hands: end_opening -->\n"
        )
        .len()
            < CHIEF.len()
    );
}

#[test]
fn the_own_hands_texts_are_the_goldens() {
    let cf = CfWord::bare();
    for (choice, key) in [(None, "on_specs_off"), (Some(specs()), "on_specs_on")] {
        let text = own(&cf, choice.as_ref());
        assert_same_text(&text, GOLDEN["ownHandsChief"][key].as_str().unwrap(), key);
    }
    let guide = guide_instructions(&[], &cf, None).unwrap();
    assert_same_text(&guide, GOLDEN["ownHandsGuide"].as_str().unwrap(), "guide");
    assert!(guide
        .contains("A tier may be served by a member's backup while that member is out of quota"));
}

#[test]
fn own_hands_changes_only_the_chief() {
    let ordinary =
        role_instructions(&Env::default(), "worker", &[], &CfWord::bare(), None).unwrap();
    assert_eq!(
        role_instructions_for(&Env::default(), "worker", &[], &CfWord::bare(), None, true).unwrap(),
        ordinary
    );
}
