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
            "Specs are now on in this project: the human turned them on while you work. This is now a section of your instructions:\n{}\n\nThis is context, not a task: say in one short line that you have it, and wait for the next message.",
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
            "Specs are now off in this project: the human turned them off while you work. Plan new work as tasks, not specs; the specs already in `docs/specs` stay. The section of your instructions on this project's specs no longer applies.\n\n\n{CONTEXT}"
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
            "The human changed this project's specs while you work: the specs are now kept in `plans`, not `docs/specs`. The specs already in `docs/specs` stay where they are. This section replaces the one on specs in your instructions; follow it from now on:\n"
        ),
        "{note}"
    );
    assert!(note.contains(&now.section()) && note.ends_with(CONTEXT));
    assert!(!note.contains("Specmint skill is now"));
}

#[test]
fn a_new_variant_names_both_and_leaves_the_folder_unsaid() {
    let was = setting("docs/specs", Variant::Core);
    let now = given("docs/specs", Variant::Tdd, SKILL);
    let note = specs_note(Some(&was), Some(&now)).unwrap();
    assert!(
        note.starts_with(
            "The human changed this project's specs while you work: the Specmint skill is now `specmint-tdd`, not `specmint-core`. This section replaces"
        ),
        "{note}"
    );
    assert!(!note.contains("stay where they are"));
}

#[test]
fn both_changed_say_both() {
    let was = setting("docs/specs", Variant::Core);
    let now = given("plans", Variant::Tdd, SKILL);
    let note = specs_note(Some(&was), Some(&now)).unwrap();
    assert!(
        note.contains("the specs are now kept in `plans`, not `docs/specs`, and the Specmint skill is now `specmint-tdd`, not `specmint-core`."),
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
        wrote.starts_with("The human wrote the rules for your staff while you work."),
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
        away.starts_with("The human took away the rules for your staff while you work."),
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
    assert!(note.contains("Specs are now on") && note.contains("rules for your staff"));
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
