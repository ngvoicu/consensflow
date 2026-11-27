//! The chief uses every role its staff holds, and its text names no other (brain
//! `architecture/role-leverage.md`): each held role has its card line and its paragraphs,
//! the roles nobody holds are named once, and no text has a command, a flag or a line of one.

use cf_base::env::Env;
use cf_engine::roles::{
    guide_instructions, ordinary_guide_instructions, role_instructions, role_instructions_for,
    ProjectSpecs, StaffRow,
};
use cf_ledger::roles::seam::with_a_scout;
use cf_proto::agents::WorkTier;
use cf_proto::cf_word::{CfWord, Shell};
use cf_proto::specs::Variant;
use regex::Regex;

use super::support::assert_same_text;

/// Every role of the registry, in its order.
const ROLES: [&str; 9] = [
    "worker",
    "frontend",
    "architect",
    "researcher",
    "advisor",
    "reviewer",
    "tester",
    "integrator",
    "designer",
];

/// One member to each of `roles`, at the standard tier (the image designer has none).
fn staff(roles: &[&str]) -> Vec<StaffRow> {
    roles
        .iter()
        .map(|role| StaffRow {
            name: format!("{role}-one"),
            model: None,
            roles: vec![(*role).to_owned()],
            work_tier: (*role != "designer").then_some(WorkTier::Standard),
        })
        .collect()
}

/// The chief's text for each of the fourteen staffs of the design's proof, whole.
#[test]
fn the_chiefs_text_for_each_of_the_fourteen_staffs_is_the_golden_whole() {
    let mut staffs: Vec<(String, Vec<&str>)> = ROLES
        .iter()
        .map(|role| (format!("only-{role}"), vec![*role]))
        .collect();
    staffs.extend([
        ("all".to_owned(), ROLES.to_vec()),
        (
            "board".to_owned(),
            vec![
                "worker",
                "frontend",
                "architect",
                "reviewer",
                "tester",
                "designer",
            ],
        ),
        ("worker-reviewer".to_owned(), vec!["worker", "reviewer"]),
        ("advisor-worker".to_owned(), vec!["advisor", "worker"]),
        ("nobody".to_owned(), vec![]),
    ]);
    assert_eq!(staffs.len(), 14);
    for (name, roles) in staffs {
        let text = role_instructions(
            &Env::default(),
            "chief",
            &staff(&roles),
            &CfWord::bare(),
            None,
        )
        .unwrap();
        let file = format!(
            "{}/tests/goldens/chief-roles/{name}.md",
            env!("CARGO_MANIFEST_DIR")
        );
        let golden = std::fs::read_to_string(&file).unwrap();
        assert_same_text(&text, &golden, &name);
    }
}

/// What a role is switched to on the card, and the name the texts say.
fn flag_and_name(role: &str) -> (&'static str, &'static str) {
    match role {
        "worker" => ("", "worker"),
        "frontend" => ("--frontend", "frontend designer"),
        "architect" => ("--architect", "architect"),
        "researcher" => ("--research", "researcher"),
        "advisor" => ("--advice", "advisor"),
        "reviewer" => ("--review", "reviewer"),
        "tester" => ("--test", "tester"),
        "integrator" => ("--integrate", "integrator"),
        "designer" => ("--design", "image designer"),
        other => panic!("no role {other}"),
    }
}

/// Every role has a paragraph that says when work goes to it, which begins so.
fn paragraph_of(role: &str) -> &'static str {
    match role {
        "worker" => "Work that can run on its own in a fresh window (a fix, a feature",
        "frontend" => "How a screen looks, feels and moves is the frontend designer's call",
        "architect" => "Work big enough to need a design (a new module",
        "researcher" => "A question of fact you need answered before you plan or decide",
        "advisor" => "A decision of yours that has more than one defensible answer",
        "reviewer" => "Finished work that your own targeted check cannot cover goes to a reviewer",
        "tester" => "A change that can be run goes to a tester before you accept it only where",
        "integrator" => "The integrator lands accepted work",
        "designer" => "An image comes from the image designer (`--design`)",
        other => panic!("no role {other}"),
    }
}

/// The words in which "worker" stands for any member whose window, message or work is spoken
/// of (the design's known limit): they name no role of the staff. They are the body's.
const ANY_MEMBER: [&str; 8] = [
    "a worker's task",
    "a worker starts from nothing",
    "result from @worker",
    "note from @worker",
    "question from @worker",
    "A worker's question",
    "A worker's window",
    "board for a fresh worker",
];

/// `text` without the line that names the roles nobody holds, the image fallback, the staff
/// table and the words in which "worker" is any member.
fn without_the_exceptions(text: &str) -> String {
    let mut kept = Vec::new();
    for paragraph in text.split("\n\n") {
        if paragraph.starts_with("Nobody on your staff is ")
            || paragraph.starts_with(
                "A member that needs images asks you with a drawing order for each. With no image",
            )
            || paragraph.starts_with("| Member |")
        {
            continue;
        }
        kept.push(paragraph);
    }
    let mut text = kept.join("\n\n").replace('\n', " ");
    for words in ANY_MEMBER {
        text = text.replace(words, "");
    }
    // A member's windows, and what a member's task carries, in the specs' section.
    text.replace("carries all its worker needs", "")
}

/// The commands a text teaches that add a task: what follows `task add` in each.
fn task_adds(text: &str) -> Vec<String> {
    text.match_indices("task add ")
        .map(|(at, _)| {
            text[at + "task add ".len()..]
                .chars()
                .take(40)
                .collect::<String>()
        })
        .collect()
}

/// Whether one task add is of a role `held` has, or is of none: a follow-up, a plan, a step of
/// the chief's own, or (without a switch) a worker's.
fn allowed(add: &str, held: &[&str]) -> bool {
    if ["--after", "--self", "--plan"]
        .iter()
        .any(|other| add.starts_with(other))
    {
        return true;
    }
    if add.starts_with("--tier ") {
        return held.contains(&"worker");
    }
    held.iter().any(|role| {
        let (flag, _) = flag_and_name(role);
        !flag.is_empty() && add.starts_with(&format!("{flag} "))
    })
}

/// What a chief is given in each text of the staff `held`: its role, the ordinary guide, and
/// the guide of a chief that works with its own hands.
fn chiefs_texts(
    held: &[&str],
    cf: &CfWord,
    specs: Option<&ProjectSpecs<'_>>,
) -> Vec<(&'static str, String)> {
    let staff = staff(held);
    let role = role_instructions(&Env::default(), "chief", &staff, cf, specs).unwrap();
    let section = role
        .find("\n## This project's specs\n")
        .map(|at| role[at..].to_owned());
    vec![
        ("the guide", {
            ordinary_guide_instructions(&staff, cf, section.as_deref(), None).unwrap()
        }),
        (
            "the own-hands guide",
            guide_instructions(&staff, cf, specs.map(|specs| specs.folder), None).unwrap(),
        ),
        ("the role", role),
    ]
}

/// For every set of roles a staff can hold, with specs on and off, on both shells, in the
/// chief's text, the ordinary guide and the own-hands guide: no flag, command or name of a
/// role nobody holds (but by the line that names them, and the image fallback), and each held
/// role's card line and paragraph.
#[test]
fn every_text_of_every_staff_names_and_commands_only_the_roles_it_holds() {
    let specs = ProjectSpecs {
        variant: Variant::Core,
        folder: "plans",
        skill: "/skills/specmint/SKILL.md",
    };
    let switches =
        Regex::new(r"--(frontend|architect|research|advice|review|test|integrate|design)\b")
            .unwrap();
    let mut wrong = Vec::new();
    for set in 0..(1_u32 << ROLES.len()) {
        let held: Vec<&str> = ROLES
            .iter()
            .enumerate()
            .filter(|(at, _)| set & (1 << at) != 0)
            .map(|(_, role)| *role)
            .collect();
        for cf in [
            CfWord::bare(),
            CfWord::new("C:/Program Files/ConsensFlow/bin/cf.exe", Shell::PowerShell),
        ] {
            for specs in [None, Some(&specs)] {
                for (what, text) in chiefs_texts(&held, &cf, specs) {
                    let label = format!("{what} of {held:?}, specs {}", specs.is_some());
                    for add in task_adds(&text) {
                        if !allowed(&add, &held) {
                            wrong.push(format!("{label}: task add {add:?}"));
                        }
                    }
                    for found in switches.find_iter(&text) {
                        if !held
                            .iter()
                            .any(|role| flag_and_name(role).0 == found.as_str())
                        {
                            wrong.push(format!("{label}: {}", found.as_str()));
                        }
                    }
                    let plain = without_the_exceptions(&text);
                    for role in ROLES.iter().filter(|role| !held.contains(role)) {
                        let (_, name) = flag_and_name(role);
                        let named = Regex::new(&format!(r"\b{name}s?\b")).unwrap();
                        if let Some(found) = named.find(&plain) {
                            // "image designer" and "frontend designer" hold the word
                            // "designer": each is matched whole.
                            let at = found.start().saturating_sub(40);
                            let end = (found.end() + 40).min(plain.len());
                            let around = plain
                                .get(plain.floor_char_boundary(at)..plain.ceil_char_boundary(end))
                                .unwrap_or_default();
                            wrong.push(format!("{label}: names the {name}: {around:?}"));
                        }
                    }
                    for role in &held {
                        if !text
                            .replace('\n', " ")
                            .contains(&paragraph_of(role).replace('\n', " "))
                        {
                            wrong.push(format!("{label}: no paragraph of the {role}"));
                        }
                    }
                    for role in &held {
                        let card = match flag_and_name(role).0 {
                            "" => "task add --tier <tier> \"…\"".to_owned(),
                            "--design" => "task add --design \"…\"".to_owned(),
                            flag => format!("task add {flag} --tier <tier> \"…\""),
                        };
                        if !text.contains(&card) {
                            wrong.push(format!("{label}: no card line {card:?}"));
                        }
                    }
                }
            }
        }
        if wrong.len() > 40 {
            break;
        }
    }
    assert!(
        wrong.is_empty(),
        "{} faults, the first: {:#?}",
        wrong.len(),
        &wrong[..wrong.len().min(20)]
    );
}

/// The example of how a text is passed to `cf` adds no task: it holds on every staff.
#[test]
fn the_shell_lesson_adds_no_task_on_either_shell() {
    for cf in [
        CfWord::bare(),
        CfWord::new("C:/Program Files/ConsensFlow/bin/cf.exe", Shell::PowerShell),
    ] {
        let text = role_instructions(&Env::default(), "chief", &[], &cf, None).unwrap();
        let lesson = &text[text.find("Any \"…\" can be").unwrap()
            ..text.find("Put commands that go together").unwrap()];
        assert!(!lesson.contains("task add"), "{lesson}");
        assert!(lesson.contains("task reopen T-3"), "{lesson}");
    }
}

/// The short text of a chief that works with its own hands says no role, whatever the staff
/// holds: it is what it was, to the byte.
#[test]
fn the_short_own_hands_text_is_the_same_whatever_the_staff() {
    let own = |staff: &[StaffRow]| {
        role_instructions_for(&Env::default(), "chief", staff, &CfWord::bare(), None, true).unwrap()
    };
    let nobody = own(&[]);
    assert_eq!(own(&staff(&ROLES)), nobody);
    for role in ROLES {
        assert!(!nobody.contains(flag_and_name(role).1), "{role}");
    }
}

/// The advisor is asked for a decision page.
#[test]
fn the_advisor_is_asked_for_a_decision_page() {
    let text = role_instructions(&Env::default(), "advisor", &[], &CfWord::bare(), None).unwrap();
    assert!(
        text.contains("Begin with what you recommend and what the chief has to decide, in a few lines. Then the options, each with what it costs and buys and its evidence, what would change your recommendation, and the tasks that follow from it."),
        "{text}"
    );
}

/// A role of the registry with no card line or paragraph in `chief.md` is refused by name, so
/// that a new role cannot go unsaid.
#[test]
fn a_role_of_the_registry_with_no_part_is_refused_by_name() {
    with_a_scout(|| {
        let failed =
            role_instructions(&Env::default(), "chief", &[], &CfWord::bare(), None).unwrap_err();
        assert_eq!(
            failed.to_string(),
            "the chief's role has no marked part scout: command"
        );
    });
}
