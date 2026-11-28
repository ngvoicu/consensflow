//! The roles a chief's staff holds are a part of what its window is told (brain
//! `architecture/role-leverage.md`): a role gained or lost is one note at the window's next
//! rest, with the roles' section whole, on every harness; every text built for the window
//! takes its role lines from the held roles it tells; a conversation the window shows after a
//! switch is told what it was not given; and a failed specs change holds nothing else back.

use cf_base::env::Env;
use cf_engine::roles::{role_instructions_holding, StaffRow};
use cf_engine::testing::Context;
use cf_harness::contract::FirstMessage;
use cf_ledger::{NewMember, NewTask, ProjectView};
use cf_proto::agents::WorkTier;
use cf_proto::cf_word::{CfWord, Shell};
use cf_proto::specs::Variant;
use serde_json::json;

use super::*;

/// What a note of the roles begins with.
const ROLES_CHANGED: &str = "Your staff's roles changed. This replaces the role lines of \"Your commands\" and the roles' paragraphs under \"Choose the role, not the member\":";

/// The roles' section of a chief's text for the roles `held`, as the real text has it: its
/// card lines, then the paragraphs of "Choose the role, not the member".
fn section_of(held: &[&str], cf: &CfWord) -> (String, String) {
    let row = |role: &&str| StaffRow {
        name: format!("{role}-one"),
        model: None,
        roles: vec![(*role).to_owned()],
        work_tier: Some(WorkTier::Standard),
    };
    let staff: Vec<StaffRow> = held.iter().map(row).collect();
    let text =
        role_instructions_holding(&Env::default(), "chief", &staff, held, cf, None, false).unwrap();
    let between = |from: &str, to: &str| {
        let after = &text[text.find(from).unwrap() + from.len()..];
        after[..after.find(to).unwrap()].to_owned()
    };
    let word = cf.as_str();
    let commands = between(
        "a command written in your reply does nothing.\n\n",
        &format!("    {word} task add --after T-3"),
    );
    let work = between(
        "member on Vacation takes no new tasks.\n\n",
        "\n\nA result you accepted stays on the board",
    );
    (commands, work)
}

/// A project whose chief is on `harness`, its staff zeus, a worker, and one member of each
/// of `others`: agent and role.
fn project_holding(context: &Context, harness: &str, others: &[(&str, &str)]) -> ProjectView {
    let mut staff = vec![
        json!({ "agent": "zeus", "harness": "claude-code", "roles": ["worker"], "tier": "standard" }),
    ];
    staff.extend(others.iter().map(|(agent, role)| {
        if *role == "designer" {
            json!({ "agent": agent, "harness": "claude-code", "roles": [role], "designer": true, "tier": "light" })
        } else {
            json!({ "agent": agent, "harness": "claude-code", "roles": [role], "tier": "standard" })
        }
    }));
    context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": harness, "model": "diana" },
            "staff": staff,
        }))
        .unwrap()
        .expect("the project")
}

/// A member of `agent` joins `project` in `role`.
fn joins(context: &Context, project: i64, agent: &str, role: &str, tier: &str) {
    context
        .ledger
        .borrow_mut()
        .add_member(
            project,
            &NewMember {
                model: None,
                effort: None,
                model_name: Some(agent.to_owned()),
                harness: "claude-code".to_owned(),
                designer: role == "designer",
                roles: vec![role.to_owned()],
                tier: tier.to_owned(),
            },
        )
        .unwrap();
}

fn leaves(context: &Context, project: i64, member: &str) {
    context
        .ledger
        .borrow_mut()
        .remove_member(project, member)
        .unwrap();
}

fn takes_roles(context: &Context, project: i64, member: &str, roles: &[&str]) {
    context
        .ledger
        .borrow_mut()
        .set_roles(project, member, roles)
        .unwrap();
}

/// Whether `note` is a whole note of the roles `held`, said with `cf`'s words.
#[track_caller]
fn says_the_section(note: &str, held: &[&str], cf: &CfWord) {
    let (commands, work) = section_of(held, cf);
    assert!(note.starts_with(ROLES_CHANGED), "{note}");
    assert!(
        note.contains(&format!("{ROLES_CHANGED}\n\n{commands}\n{work}\n")),
        "the section whole, for {held:?}: {note}"
    );
}

#[test]
fn an_advisor_gained_and_the_last_researcher_lost_are_each_one_note_with_the_section_whole() {
    let context = made(FirstMessage::Delivered);
    let project = project_holding(&context, "claude-code", &[("argus", "researcher")]);
    let chief = at_rest(&context, &project);
    assert!(role_notes(&context, chief).is_empty());
    let cf = CfWord::bare();

    joins(&context, project.id, "athena", "advisor", "standard");
    passes(&context);
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 1, "{notes:?}");
    says_the_section(&notes[0], &["worker", "researcher", "advisor"], &cf);
    rests(&context, &project);
    passes(&context);
    assert_eq!(role_notes(&context, chief).len(), 1);

    leaves(&context, project.id, "argus");
    passes(&context);
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 2, "{notes:?}");
    says_the_section(&notes[1], &["worker", "advisor"], &cf);
    assert!(!notes[1].contains("--research"), "{}", notes[1]);
    context.close();
}

#[test]
fn the_image_designer_lost_and_gained_gives_the_fallback_or_the_drawing_orders() {
    let context = made(FirstMessage::Delivered);
    let project = project_holding(&context, "claude-code", &[("iris", "designer")]);
    let chief = at_rest(&context, &project);
    let cf = CfWord::bare();

    leaves(&context, project.id, "iris");
    passes(&context);
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 1, "{notes:?}");
    says_the_section(&notes[0], &["worker"], &cf);
    assert!(!notes[0].contains("--design"), "{}", notes[0]);
    assert!(
        notes[0].contains("With no image\ndesigner, the member makes them itself"),
        "{}",
        notes[0]
    );
    rests(&context, &project);

    joins(&context, project.id, "iris", "designer", "light");
    passes(&context);
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 2, "{notes:?}");
    says_the_section(&notes[1], &["worker", "designer"], &cf);
    assert!(notes[1].contains("--design"), "{}", notes[1]);
    assert!(
        notes[1].contains("Give it a precise\ndrawing order"),
        "{}",
        notes[1]
    );
    assert!(!notes[1].contains("With no image"), "{}", notes[1]);
    context.close();
}

#[test]
fn a_second_holder_a_tier_edited_and_a_role_changed_with_own_hands_on_tell_nothing() {
    for own_hands in [false, true] {
        let context = made(FirstMessage::Delivered);
        let project = project_on(&context, "claude-code");
        let chief = at_rest(&context, &project);
        if own_hands {
            context
                .ledger
                .borrow_mut()
                .set_own_hands(project.id, true)
                .unwrap();
            passes(&context);
            rests(&context, &project);
        }
        let before = role_notes(&context, chief).len();
        if own_hands {
            joins(&context, project.id, "athena", "advisor", "standard");
        } else {
            joins(&context, project.id, "athena", "worker", "light");
        }
        passes(&context);
        context
            .ledger
            .borrow_mut()
            .edit_member(
                project.id,
                "zeus",
                &cf_ledger::MemberEdit {
                    harness: "claude-code".to_owned(),
                    model: None,
                    effort: None,
                    model_name: Some("zeus".to_owned()),
                    tier: "complex".to_owned(),
                },
            )
            .unwrap();
        passes(&context);
        passes(&context);
        assert_eq!(
            role_notes(&context, chief).len(),
            before,
            "own hands {own_hands}"
        );
        context.close();
    }
}

#[test]
fn a_role_gained_mid_turn_is_one_note_at_the_next_rest_on_every_harness_in_its_own_cf() {
    for cf in [
        CfWord::new(
            "/Users/someone/Apps/ConsensFlow.app/Contents/Resources/cli/bin/cf",
            Shell::Posix,
        ),
        CfWord::new(
            r"C:\Program Files\ConsensFlow\cli\bin\cf.exe",
            Shell::PowerShell,
        ),
    ] {
        for first in [FirstMessage::Delivered, FirstMessage::Opened] {
            for harness in HARNESSES {
                let context = made(first);
                *context.roles.cf.borrow_mut() = cf.clone();
                let project = project_on(&context, harness);
                let chief = at_rest(&context, &project);
                let label = format!("{harness}, {first:?}, {cf}");
                in_a_turn(&context, &project);
                let typed = context.host.inputs().len();
                joins(&context, project.id, "athena", "advisor", "standard");
                passes(&context);
                assert_eq!(context.host.inputs().len(), typed, "{label}: mid-turn");
                assert!(role_notes(&context, chief).is_empty(), "{label}: mid-turn");
                answers(&context, &project);
                let notes = told(&context, chief);
                assert_eq!(notes.len(), 1, "{label}: {notes:?}");
                says_the_section(&notes[0], &["worker", "advisor"], &cf);
                assert!(
                    notes[0].contains(&format!("\n    {} task add --advice", cf.as_str())),
                    "{label}: {}",
                    notes[0]
                );
                rests(&context, &project);
                passes(&context);
                assert_eq!(role_notes(&context, chief).len(), 1, "{label}");
                context.close();
            }
        }
    }
}

/// The role the current conversation of `chief` is recorded to hold.
fn recorded(context: &Context, chief: i64) -> Option<String> {
    let ledger = context.ledger.borrow();
    let current = ledger
        .current_conversation(chief)
        .unwrap()
        .expect("a conversation");
    ledger.conversation_role_text(current.id).unwrap()
}

#[test]
fn a_codex_resume_says_the_role_again_when_a_role_gained_and_told_is_gone_before_its_loss_went_in()
{
    let context = with_codex();
    context.codex.keeps_first_instructions.set(true);
    context.roles.with_held.set(true);
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "codex", "model": "diana" },
            "staff": [{ "agent": "zeus", "harness": "claude-code", "roles": ["worker"], "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project");
    passes(&context);
    let chief = context.id(project.id, "chief");
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief holding worker")
    );
    takes_roles(&context, project.id, "zeus", &["worker", "advisor"]);
    passes(&context);
    assert_eq!(told(&context, chief).len(), 1);
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief holding worker advisor"),
        "what was told is what is recorded"
    );
    // The role is removed, and the window goes before the second note goes in: the staff
    // table is the launch's to the byte, and the resume says the role again all the same.
    takes_roles(&context, project.id, "zeus", &["worker"]);
    relaunch(&context, project.id);
    passes(&context);
    let notes = role_notes(&context, chief);
    let replacements: Vec<&MessageView> = notes
        .iter()
        .filter(|note| {
            note.body
                .contains("Follow these instructions in place of the earlier ones")
        })
        .collect();
    assert_eq!(replacements.len(), 1, "{notes:?}");
    assert!(
        replacements[0]
            .body
            .ends_with("instructions for chief holding worker"),
        "{}",
        replacements[0].body
    );
    context.close();
}

#[test]
fn own_hands_off_gives_the_whole_role_with_the_roles_held_now_and_no_second_note() {
    for harness in HARNESSES {
        let context = made(FirstMessage::Delivered);
        context.roles.with_held.set(true);
        let project = project_on(&context, harness);
        let chief = at_rest(&context, &project);
        context
            .ledger
            .borrow_mut()
            .set_own_hands(project.id, true)
            .unwrap();
        passes(&context);
        rests(&context, &project);
        joins(&context, project.id, "athena", "advisor", "standard");
        passes(&context);
        context
            .ledger
            .borrow_mut()
            .set_own_hands(project.id, false)
            .unwrap();
        passes(&context);
        let notes = told(&context, chief);
        assert_eq!(notes.len(), 2, "{harness}: {notes:?}");
        assert!(notes[1].starts_with(OPENING), "{harness}: {}", notes[1]);
        assert!(
            notes[1].contains("instructions for chief holding worker advisor"),
            "{harness}: {}",
            notes[1]
        );
        rests(&context, &project);
        passes(&context);
        assert_eq!(role_notes(&context, chief).len(), 2, "{harness}");
        context.close();
    }
}

#[test]
fn a_task_of_a_role_lost_before_it_was_added_waits_open_and_is_told_once() {
    let context = made(FirstMessage::Delivered);
    let project = project_holding(&context, "claude-code", &[("athena", "advisor")]);
    let chief = at_rest(&context, &project);
    // The chief saw an advisor; its last holder loses the role; then the chief adds the task.
    takes_roles(&context, project.id, "athena", &["researcher"]);
    let number = context
        .create_task(
            project.id,
            NewTask {
                from: "chief".to_owned(),
                pool: Some("advisor".to_owned()),
                tier: Some("standard".to_owned()),
                body: "Which parser?".to_owned(),
                ..NewTask::default()
            },
        )
        .task
        .number;
    for _ in 0..6 {
        context.pass().unwrap();
    }
    let task = context.task(project.id, number).task;
    assert_eq!((task.state.as_str(), task.assignee), ("open", None));
    let waits: Vec<String> = context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.kind == "note" && message.body.contains(" waits for "))
        .map(|message| message.body)
        .collect();
    assert_eq!(waits.len(), 1, "{waits:?}");
    context.close();
}

#[test]
fn a_specs_change_that_cannot_be_put_in_place_holds_back_no_role_change_of_the_same_rest() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "claude-code");
    let chief = at_rest(&context, &project);
    *context.adapter.place_fails.borrow_mut() = Some("no space left on device".to_owned());
    says(
        &context,
        project.id,
        Some(true),
        "docs/specs",
        Some(Variant::Core),
    );
    joins(&context, project.id, "athena", "advisor", "standard");
    passes(&context);
    passes(&context);
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 1, "{notes:?}");
    says_the_section(&notes[0], &["worker", "advisor"], &CfWord::bare());
    assert!(!notes[0].contains("specs"), "{}", notes[0]);
    let refused: Vec<String> = to_human(&context, project.id)
        .into_iter()
        .filter(|words| words.contains("could not be told"))
        .collect();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].contains("no space left on device")
            && refused[0].contains("when it next starts"),
        "{}",
        refused[0]
    );
    // The specs it could not be given are not recorded as told: its next launch gives them,
    // and another change of its roles tells no second refusal and still goes in.
    rests(&context, &project);
    leaves(&context, project.id, "athena");
    passes(&context);
    passes(&context);
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 2, "{notes:?}");
    says_the_section(&notes[1], &["worker"], &CfWord::bare());
    assert!(!notes[1].contains("specs"), "{}", notes[1]);
    assert_eq!(
        to_human(&context, project.id)
            .iter()
            .filter(|words| words.contains("could not be told"))
            .count(),
        1
    );
    context.close();
}

/// The chief's window shows another conversation, which a hook (or the harness's own system
/// prompt) gave the role its window launched with, not the notes since.
fn clears(context: &Context) {
    context.adapter.switch_to("chief", "native-cleared");
    passes(context);
    passes(context);
}

#[test]
fn a_clear_after_a_role_gained_or_lost_tells_the_new_conversation_what_it_was_not_given_on_every_harness(
) {
    for harness in HARNESSES {
        for gains in [true, false] {
            let context = made(FirstMessage::Delivered);
            let others: &[(&str, &str)] = if gains { &[] } else { &[("athena", "advisor")] };
            let project = project_holding(&context, harness, others);
            let chief = at_rest(&context, &project);
            let label = format!("{harness}, gains {gains}");
            if gains {
                joins(&context, project.id, "athena", "advisor", "standard");
            } else {
                leaves(&context, project.id, "athena");
            }
            passes(&context);
            assert_eq!(told(&context, chief).len(), 1, "{label}: delivered");
            rests(&context, &project);
            clears(&context);
            let notes = told(&context, chief);
            assert_eq!(notes.len(), 2, "{label}: {notes:?}");
            let held: &[&str] = if gains {
                &["worker", "advisor"]
            } else {
                &["worker"]
            };
            says_the_section(&notes[1], held, &CfWord::bare());
            rests(&context, &project);
            passes(&context);
            assert_eq!(role_notes(&context, chief).len(), 2, "{label}: once");
            context.close();
        }
    }
}

#[test]
fn a_clear_with_nothing_told_since_the_launch_tells_the_new_conversation_nothing() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "devin");
    let chief = at_rest(&context, &project);
    clears(&context);
    passes(&context);
    assert!(role_notes(&context, chief).is_empty());
    context.close();
}

#[test]
fn a_clear_cancels_a_note_that_waited_for_the_conversation_left_and_says_it_anew() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "devin");
    let chief = at_rest(&context, &project);
    joins(&context, project.id, "athena", "advisor", "standard");
    passes(&context);
    rests(&context, &project);
    // The staff's rules change; the note waits behind a message, and the window is cleared.
    rules(&context, project.id, Some(RULES));
    context.pass().unwrap();
    context.note(project.id, "human", "chief", "Look at the board.");
    context.pass().unwrap();
    let waiting = role_notes(&context, chief)
        .pop()
        .expect("the note of the rules");
    assert_eq!(waiting.state, "queued", "{waiting:?}");
    clears(&context);
    assert_eq!(
        context.message(waiting.id).state,
        "cancelled",
        "it says what changed since the conversation left was told"
    );
    rests(&context, &project);
    passes(&context);
    let said = said_to_the_chief(&context.adapter);
    let last_roles = said
        .iter()
        .rev()
        .find(|line| line.contains(ROLES_CHANGED))
        .expect("the roles were said");
    assert!(last_roles.contains("--advice"), "{last_roles}");
    // What the new conversation was told holds both, whatever order its notes went in.
    assert!(
        said.iter().rev().any(|line| line.contains(RULES)),
        "{said:?}"
    );
    context.close();
}

#[test]
fn a_role_change_is_told_though_the_skill_of_specs_that_did_not_change_cannot_be_written_again() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "claude-code");
    let chief = at_rest(&context, &project);
    says(
        &context,
        project.id,
        Some(true),
        "docs/specs",
        Some(Variant::Core),
    );
    passes(&context);
    rests(&context, &project);
    assert_eq!(told(&context, chief).len(), 1);
    *context.adapter.place_fails.borrow_mut() = Some("no space left on device".to_owned());
    joins(&context, project.id, "athena", "advisor", "standard");
    passes(&context);
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 2, "{notes:?}");
    says_the_section(&notes[1], &["worker", "advisor"], &CfWord::bare());
    assert!(
        !to_human(&context, project.id)
            .iter()
            .any(|words| words.contains("could not be told")),
        "nothing was refused"
    );
    context.close();
}

/// Muse Code's `/clear` runs no hook, so its greeting waits to go in; the person types into the
/// new session first and its hook greets it. The greeting is withdrawn, and the session holds
/// the role the window launched with: a role gained since is told it, at the next rest.
#[test]
fn a_greeting_withdrawn_because_a_hook_greeted_the_session_leaves_a_role_gained_since_the_launch_to_be_told(
) {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "muse");
    let chief = at_rest(&context, &project);
    joins(&context, project.id, "athena", "advisor", "standard");
    passes(&context);
    rests(&context, &project);
    assert_eq!(told(&context, chief).len(), 1, "the gain, told");
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context.adapter.switch_to("chief", "native-cleared");
    context.adapter.with("chief", |agent| agent.settled = false);
    passes(&context);
    let greeting = role_notes(&context, chief)
        .into_iter()
        .find(|note| note.body.contains("began without your ConsensFlow role"))
        .expect("the greeting waits");
    assert_eq!(greeting.state, "queued");
    // The person types, and the hook greets the session with the launch's role.
    context.adapter.with("chief", |agent| {
        agent.ungreeted.clear();
        agent.settled = true;
    });
    passes(&context);
    passes(&context);
    assert_eq!(context.message(greeting.id).state, "cancelled");
    rests(&context, &project);
    passes(&context);
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 2, "the advisor said again: {notes:?}");
    says_the_section(&notes[1], &["worker", "advisor"], &CfWord::bare());
    context.close();
}

/// A conversation the window comes back to, already told the role as it is now, is taken as
/// told it: nothing is said to it again.
#[test]
fn a_return_to_a_conversation_already_told_the_role_as_it_is_now_tells_it_nothing_more() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "claude-code");
    let chief = at_rest(&context, &project);
    joins(&context, project.id, "athena", "advisor", "standard");
    passes(&context);
    rests(&context, &project);
    // A session no hook greeted is given the role as it is, and recorded told it.
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context.adapter.switch_to("chief", "native-cleared");
    passes(&context);
    passes(&context);
    rests(&context, &project);
    let before = role_notes(&context, chief).len();
    // A conversation a hook greeted holds the launch's role; then back to the first.
    context.adapter.switch_to("chief", "native-older");
    context.pass().unwrap();
    context.adapter.switch_to("chief", "native-cleared");
    passes(&context);
    passes(&context);
    rests(&context, &project);
    passes(&context);
    let notes = role_notes(&context, chief);
    assert_eq!(notes.len(), before, "{notes:?}");
    context.close();
}
