use cf_base::text::utf16_len;
use cf_proto::specs::Variant;

use super::*;
use crate::delivery_text::fits_whole;

fn setting(folder: &str, variant: Variant) -> SpecsSetting {
    SpecsSetting {
        folder: folder.to_owned(),
        variant,
    }
}

fn given<'a>(folder: &'a str, variant: Variant, skill: &'a str) -> ProjectSpecs<'a> {
    ProjectSpecs {
        variant,
        folder,
        skill,
    }
}

const SKILL: &str = "/home/me/.consensflow/integrations/specmint/claude/core-0123abcd/.claude/skills/specmint-core/SKILL.md";

#[test]
fn the_own_hands_on_note_is_the_short_text_whole_and_fits_in_one_piece() {
    use cf_base::env::Env;
    use cf_proto::cf_word::{CfWord, Shell};
    let specs = given("plans", Variant::Core, SKILL);
    for cf in [
        CfWord::bare(),
        CfWord::new(
            "/box/Applications/ConsensFlow.app/Contents/Resources/cli/bin/cf",
            Shell::Posix,
        ),
        CfWord::new(
            r"C:\box\Program Files\ConsensFlow\resources\cli\bin\cf.exe",
            Shell::PowerShell,
        ),
    ] {
        for chosen in [None, Some(&specs)] {
            let instructions = crate::roles::role_instructions_for(
                &Env::default(),
                "chief",
                &[],
                &cf,
                chosen,
                true,
            )
            .unwrap();
            let note = own_hands_on(&instructions);
            assert!(note.starts_with("The person turned \"Do with your own hands\" on."));
            // What the conversation holds once the note is in is the role, whole: the note
            // carries it word for word.
            assert!(note.contains(&instructions));
            let word = if crate::roles::PATH_ONCE {
                "cf"
            } else {
                cf.as_str()
            };
            assert!(note.contains(&format!("run `{word} guide`")));
            assert_eq!(note.contains("## This project's specs"), chosen.is_some());
            assert!(!note.contains("## The staff") && !note.contains("task add"));
            assert!(note.ends_with(CONTEXT));
            assert!(fits_whole(&note), "{} bytes", note.len());
            assert!(cf_base::role_read::role_read_whole(&note));
        }
    }
}

fn specs_note(was: Option<&SpecsSetting>, now: Option<&ProjectSpecs<'_>>) -> Option<String> {
    Change {
        specs: Some((was, now)),
        rules: None,
    }
    .note()
}

#[test]
fn nothing_that_changed_is_nothing_told() {
    assert_eq!(
        Change {
            specs: None,
            rules: None
        }
        .note(),
        None
    );
}

#[test]
fn specs_turned_on_give_the_section_and_say_it_is_context() {
    let now = given("docs/specs", Variant::Core, SKILL);
    let note = specs_note(None, Some(&now)).unwrap();
    assert_eq!(
        note,
        format!(
            "Your instructions now include:\n{}\n\n{CONTEXT}",
            now.section()
        )
    );
    assert!(note.contains(SKILL) && note.contains("`docs/specs`"));
    assert!(
        !note.contains("You are the chief"),
        "only the section of the role"
    );
}

#[test]
fn specs_turned_off_say_to_plan_as_tasks_and_that_the_specs_there_stay() {
    let was = setting("docs/specs", Variant::Core);
    let note = specs_note(Some(&was), None).unwrap();
    assert_eq!(
        note,
        format!(
            "Specs are now off. Plan new work as tasks, not specs; the specs in `docs/specs` stay. Your earlier specs instructions no longer apply.\n\n\n{CONTEXT}"
        )
    );
}

#[test]
fn a_new_folder_names_both_and_gives_the_section_with_the_skill_of_the_new_one() {
    let was = setting("docs/specs", Variant::Core);
    let now = given("plans", Variant::Core, SKILL);
    let note = specs_note(Some(&was), Some(&now)).unwrap();
    assert!(
        note.starts_with(
            "The project's specs changed. The specs in `docs/specs` stay where they are. This section replaces your earlier specs instructions:\n"
        ),
        "{note}"
    );
    assert!(note.contains(&now.section()) && note.ends_with(CONTEXT));
    assert!(note.contains("`plans`"));
    assert!(!note.contains("Specmint skill is now"));
}

#[test]
fn a_new_variant_names_both_and_leaves_the_folder_unsaid() {
    let was = setting("docs/specs", Variant::Core);
    let now = given("docs/specs", Variant::Tdd, SKILL);
    let note = specs_note(Some(&was), Some(&now)).unwrap();
    assert!(
        note.starts_with(
            "The project's specs changed. This section replaces your earlier specs instructions:"
        ),
        "{note}"
    );
    assert!(!note.contains("stay where they are"));
    assert!(note.contains("`specmint-tdd`"), "{note}");
    assert!(!note.contains("`specmint-core`"), "{note}");
}

#[test]
fn both_changed_say_both() {
    let was = setting("docs/specs", Variant::Core);
    let now = given("plans", Variant::Tdd, SKILL);
    let note = specs_note(Some(&was), Some(&now)).unwrap();
    assert!(
        note.contains("The specs in `docs/specs` stay where they are.")
            && note.contains("`plans`")
            && note.contains("`specmint-tdd`"),
        "{note}"
    );
}

#[test]
fn rules_written_changed_and_taken_away_each_have_their_words() {
    let rules = |was, now| {
        Change {
            specs: None,
            rules: Some((was, now)),
        }
        .note()
        .unwrap()
    };
    let wrote = rules(None, Some("- Push to origin only."));
    assert!(
        wrote.starts_with("The human wrote the rules for your staff."),
        "{wrote}"
    );
    assert!(wrote.contains(&with_staff_rules(
        String::new(),
        "chief",
        Some("- Push to origin only.")
    )));
    let changed = rules(Some("- A."), Some("- B."));
    assert!(changed.starts_with("The human changed the rules for your staff"));
    assert!(changed.contains("- B.") && !changed.contains("- A."));
    let away = rules(Some("- A."), None);
    assert!(
        away.starts_with("The human took away the rules for your staff."),
        "{away}"
    );
    assert!(!away.contains("- A."));
}

#[test]
fn specs_and_rules_changed_together_are_one_note() {
    let now = given("docs/specs", Variant::Core, SKILL);
    let note = Change {
        specs: Some((None, Some(&now))),
        rules: Some((None, Some("- A."))),
    }
    .note()
    .unwrap();
    assert_eq!(note.matches(CONTEXT).count(), 1);
    assert!(
        note.contains("Your instructions now include:") && note.contains("rules for your staff")
    );
}

#[test]
fn the_largest_note_a_setting_makes_goes_into_a_window_whole() {
    // The most rules the ledger takes, 8,000 bytes and 150 lines, in two-byte letters.
    let lines = "é".repeat(26);
    let rules = std::iter::repeat_n(lines.as_str(), 150)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rules.len() <= 8_000, "{}", rules.len());
    let folder = "a/very/long/folder/".repeat(10);
    let now = given(folder.trim_end_matches('/'), Variant::TddHtml, SKILL);
    let note = Change {
        specs: Some((None, Some(&now))),
        rules: Some((Some("- old"), Some(&rules))),
    }
    .note()
    .unwrap();
    // Well under what goes into a window whole (16,000 units): the rules are most of it.
    assert!(utf16_len(&note) < 8_000, "{}", utf16_len(&note));
    assert!(
        fits_whole(&note),
        "a note of a setting is never cut: it says nothing the window could miss"
    );
}

// T-1715: each distinct news form keeps the information needed to act on the new role.
#[test]
fn short_text_t1715_news_context() {
    assert_eq!(
        CONTEXT,
        "This is context, not a task: acknowledge it in one line and wait for the next message."
    );
}

#[test]
fn short_text_t1715_specs_on() {
    let now = given("docs/specs", Variant::Core, SKILL);
    assert_eq!(
        specs_said(None, Some(&now), false),
        format!("Your instructions now include:\n{}", now.section())
    );
}

#[test]
fn short_text_t1715_specs_off_ordinary() {
    let old = setting("old-specs", Variant::Tdd);
    assert_eq!(specs_said(Some(&old), None, false), "Specs are now off. Plan new work as tasks, not specs; the specs in `old-specs` stay. Your earlier specs instructions no longer apply.\n");
}

#[test]
fn short_text_t1715_specs_off_own_hands() {
    let old = setting("old-specs", Variant::Tdd);
    assert_eq!(specs_said(Some(&old), None, true), "Specs are now off; the specs in `old-specs` stay. Your earlier specs instructions no longer apply.\n");
}

#[test]
fn short_text_t1715_specs_changed() {
    let old = setting("old-specs", Variant::Tdd);
    let now = given("docs/specs", Variant::Core, SKILL);
    assert_eq!(specs_said(Some(&old), Some(&now), false), format!("The project's specs changed. The specs in `old-specs` stay where they are. This section replaces your earlier specs instructions:\n{}", now.section()));
}

#[test]
fn short_text_t1715_rules_written_and_changed() {
    for (was, verb) in [(None, "wrote"), (Some("old"), "changed")] {
        assert_eq!(rules_said(was, Some("new")), format!("The human {verb} the rules for your staff. New windows get them; open windows keep theirs. This section replaces your earlier rules:\n{}", with_staff_rules(String::new(), "chief", Some("new"))));
    }
}

#[test]
fn short_text_t1715_rules_removed() {
    assert_eq!(rules_said(Some("old"), None), "The human took away the rules for your staff. New windows get none; open windows keep theirs. Your earlier rules no longer apply.\n");
}

#[test]
fn short_text_t1715_own_hands() {
    assert_eq!(own_hands_on("role"), format!("The person turned \"Do with your own hands\" on. These instructions replace your earlier ones.\n\nrole\n\n{CONTEXT}"));
}
