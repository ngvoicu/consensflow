//! A setting that changes the chief's role while its window is open is told to
//! the window at its next rest (T-1182), on every harness: the project's specs
//! (on, off, the folder, the variant, and what the app says for a project that
//! follows it) and the project's rules for its staff. The note says what changed
//! and what to do now, and only the section of the role that changed. One change
//! is one note, none goes while the chief is in a turn, a closed project is told
//! nothing and its chief's next launch has the role as it is, and a Codex chief
//! (whose thread keeps what it began with) that was told is not told the same again
//! when it is resumed. Node's dispatcher had no such note, so no trace holds these.

use cf_engine::testing::{Context, FakeAdapter, Made};
use cf_harness::contract::FirstMessage;
use cf_harness::records::Role;
use cf_ledger::{MessageView, ProjectView, SpecsChange};
use cf_proto::specs::{AppSpecs, Variant};
use serde_json::{json, Value};

use crate::chiefs::{to_human, with_codex};

/// Every harness a chief's window runs on.
const HARNESSES: [&str; 5] = ["claude-code", "codex", "pi", "devin", "muse"];

/// The copy for a project that keeps its specs in `docs/specs`.
const DOCS_SKILL: &str =
    "/consensflow/integrations/specmint/fake/core-docs-specs/.claude/skills/specmint-core/SKILL.md";

/// The copy for a project that keeps its specs in `plans`.
const PLANS_SKILL: &str =
    "/consensflow/integrations/specmint/fake/core-plans/.claude/skills/specmint-core/SKILL.md";

/// Rules a human writes.
const RULES: &str = "- Push to origin only, never to upstream.";

/// The rules after the human changed them.
const CHANGED: &str = "- Push to origin only.";

/// A test's engine whose chief runs on `harness`, its windows taking their first
/// message as `first` says.
fn made(first: FirstMessage) -> Context {
    let context = Context::made(Made {
        harnesses: HARNESSES.to_vec(),
        ..Made::default()
    });
    context.adapter.first.set(first);
    context
}

/// A project whose chief is on `harness`, specs off: diana its chief, zeus a worker.
fn project_on(context: &Context, harness: &str) -> ProjectView {
    context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": harness, "agent": "diana" },
            "staff": [{ "agent": "zeus", "harness": "claude-code", "roles": ["worker"], "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project")
}

/// The passes a delivery takes to be handed over and seen in the record.
fn passes(context: &Context) {
    for _ in 0..4 {
        context.pass().unwrap();
    }
}

/// The chief is at rest: it launched, and has nothing on its way.
fn at_rest(context: &Context, project: &ProjectView) -> i64 {
    passes(context);
    let chief = context.id(project.id, "chief");
    assert_eq!(
        context.dispatcher.activity(chief).state,
        cf_engine::ActivityState::Idle
    );
    chief
}

/// The chief is in a turn: a note reached it, and it has not answered.
fn in_a_turn(context: &Context, project: &ProjectView) {
    context.note(project.id, "human", "chief", "Look at the board.");
    passes(context);
    assert_eq!(
        context
            .dispatcher
            .activity(context.id(project.id, "chief"))
            .state,
        cf_engine::ActivityState::Working
    );
}

/// The chief answers the turn it is in, and the looks go on.
fn answers(context: &Context, project: &ProjectView) {
    let _ = project;
    context.adapter.answer("chief", "Noted.");
    passes(context);
}

/// The chief answers the turn it is in, and nothing more is on its way: it is at rest.
fn rests(context: &Context, project: &ProjectView) {
    answers(context, project);
    assert_eq!(
        context
            .dispatcher
            .activity(context.id(project.id, "chief"))
            .state,
        cf_engine::ActivityState::Idle
    );
}

/// The project says this of its specs.
fn says(context: &Context, project: i64, on: Option<bool>, folder: &str, variant: Option<Variant>) {
    context
        .ledger
        .borrow_mut()
        .set_specs(
            project,
            &SpecsChange {
                on,
                folder: (!folder.is_empty()).then(|| folder.to_owned()),
                variant,
            },
        )
        .unwrap();
}

/// The project's rules for its staff, or none.
fn rules(context: &Context, project: i64, rules: Option<&str>) {
    context
        .ledger
        .borrow_mut()
        .set_staff_rules(project, rules)
        .unwrap();
}

/// The notes that tell `participant` something of its role, oldest first: what the
/// ledger knows for a note of a role.
fn role_notes(context: &Context, participant: i64) -> Vec<MessageView> {
    let ids = context.ledger.borrow().role_notes_to(participant).unwrap();
    ids.into_iter().map(|id| context.message(id)).collect()
}

/// The words of the notes that told `participant` of its role, delivered ones only.
fn told(context: &Context, participant: i64) -> Vec<String> {
    role_notes(context, participant)
        .into_iter()
        .filter(|note| note.state == "delivered" || note.state == "read")
        .map(|note| note.body)
        .collect()
}

/// What was typed into the chief's window as user turns, in order.
fn said_to_the_chief(adapter: &FakeAdapter) -> Vec<String> {
    adapter
        .agent("chief")
        .items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| item.text.to_string())
        .collect()
}

/// The launches of the chief's window on `adapter`, oldest first.
fn chief_launches(adapter: &FakeAdapter) -> Vec<Value> {
    adapter
        .prepared()
        .into_iter()
        .filter(|launch| launch["role"] == "chief")
        .collect()
}

/// The human closes the project and resumes it: the chief's window opens again.
fn relaunch(context: &Context, project: i64) {
    context.close_project(project).unwrap();
    context.resume_project(project).unwrap();
}

#[test]
fn specs_turned_on_then_off_then_moved_while_the_chief_runs_are_told_at_each_rest_once_on_every_harness(
) {
    for first in [FirstMessage::Delivered, FirstMessage::Opened] {
        for harness in HARNESSES {
            let context = made(first);
            let project = project_on(&context, harness);
            let chief = at_rest(&context, &project);
            let label = format!("{harness}, first message {first:?}");
            assert!(
                role_notes(&context, chief).is_empty(),
                "{label}: nothing yet"
            );
            assert_eq!(
                chief_launches(&context.adapter).len(),
                1,
                "{label}: one launch"
            );

            // Turned on, in a folder of its own and a variant, while the chief is in a turn:
            // nothing is written or typed until it is at rest.
            in_a_turn(&context, &project);
            let typed = context.host.inputs().len();
            says(
                &context,
                project.id,
                Some(true),
                "docs/specs",
                Some(Variant::Core),
            );
            passes(&context);
            assert_eq!(context.host.inputs().len(), typed, "{label}: mid-turn");
            assert!(role_notes(&context, chief).is_empty(), "{label}: mid-turn");
            assert!(
                context.adapter.specmint_placed.borrow().is_empty(),
                "{label}: no skill is put in place for a chief that is not told"
            );
            // At rest it is told, once, however many looks come before it goes in: the
            // turn it was in ends, the note goes in and begins a turn of its own.
            answers(&context, &project);
            let on = told(&context, chief);
            assert_eq!(on.len(), 1, "{label}: {on:?}");
            assert!(
                on[0].starts_with("Specs are now on in this project: "),
                "{label}: {}",
                on[0]
            );
            assert!(on[0].contains("## This project's specs"), "{label}");
            assert!(on[0].contains(DOCS_SKILL), "{label}: names the skill");
            assert!(
                on[0].contains("Your specs and their registry are in `docs/specs`"),
                "{label}"
            );
            assert!(
                on[0].contains("say in one short line that you have it"),
                "{label}"
            );
            assert!(
                !on[0].contains("You are the chief"),
                "{label}: only the section, not the whole role"
            );
            assert_eq!(
                *context.adapter.specmint_placed.borrow(),
                [(
                    Variant::Core,
                    "docs/specs".to_owned(),
                    DOCS_SKILL.to_owned()
                )],
                "{label}: the skill is put in its place before the chief is told where it is"
            );
            assert!(
                said_to_the_chief(&context.adapter)
                    .last()
                    .is_some_and(|last| last.contains("Specs are now on in this project: ")),
                "{label}: it went into the window"
            );
            rests(&context, &project);
            passes(&context);
            assert_eq!(told(&context, chief).len(), 1, "{label}: not told again");

            // Turned off: the specs already there stay.
            says(&context, project.id, Some(false), "", None);
            passes(&context);
            let off = told(&context, chief);
            assert_eq!(off.len(), 2, "{label}: {off:?}");
            assert!(
                off[1].starts_with("Specs are now off in this project: "),
                "{label}: {}",
                off[1]
            );
            assert!(
                off[1].contains(
                    "Plan new work as tasks, not specs; the specs already in `docs/specs` stay."
                ),
                "{label}: {}",
                off[1]
            );
            assert!(!off[1].contains("## This project's specs"), "{label}");
            rests(&context, &project);

            // Turned on again, then the folder moved: the note names the change and
            // gives the section anew, with the copy of the skill for the new folder.
            says(
                &context,
                project.id,
                Some(true),
                "docs/specs",
                Some(Variant::Core),
            );
            passes(&context);
            rests(&context, &project);
            assert_eq!(told(&context, chief).len(), 3, "{label}");
            says(
                &context,
                project.id,
                Some(true),
                "plans",
                Some(Variant::Core),
            );
            passes(&context);
            let moved = told(&context, chief);
            assert_eq!(moved.len(), 4, "{label}: {moved:?}");
            assert!(
                moved[3].contains("the specs are now kept in `plans`, not `docs/specs`"),
                "{label}: {}",
                moved[3]
            );
            assert!(
                moved[3].contains("The specs already in `docs/specs` stay where they are."),
                "{label}: {}",
                moved[3]
            );
            assert!(moved[3].contains(PLANS_SKILL), "{label}: {}", moved[3]);
            assert!(
                moved[3].contains("Your specs and their registry are in `plans`"),
                "{label}"
            );
            assert_eq!(
                context.adapter.specmint_placed.borrow().last().cloned(),
                Some((Variant::Core, "plans".to_owned(), PLANS_SKILL.to_owned())),
                "{label}: the copy for the new folder is in place"
            );
            // The window launched once, and was closed by none of this.
            assert_eq!(chief_launches(&context.adapter).len(), 1, "{label}");
            rests(&context, &project);
            passes(&context);
            assert_eq!(told(&context, chief).len(), 4, "{label}: settled");
            assert_eq!(
                role_notes(&context, chief).len(),
                4,
                "{label}: no note was left over"
            );
            context.close();
        }
    }
}

#[test]
fn a_variant_changed_is_told_with_the_new_skill_and_nothing_of_a_folder() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "claude-code");
    says(&context, project.id, Some(true), "", Some(Variant::Core));
    relaunch(&context, project.id);
    let chief = at_rest(&context, &project);
    assert!(role_notes(&context, chief).is_empty());
    says(&context, project.id, Some(true), "", Some(Variant::TddHtml));
    passes(&context);
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains("the Specmint skill is now `specmint-tdd-html`, not `specmint-core`"),
        "{}",
        notes[0]
    );
    assert!(!notes[0].contains("are now kept in"), "{}", notes[0]);
    assert!(!notes[0].contains("stay where they are"), "{}", notes[0]);
    assert!(notes[0].contains("specmint-tdd-html"), "{}", notes[0]);
}

#[test]
fn what_the_app_says_for_every_project_reaches_a_chief_whose_project_follows_it_and_not_one_that_says_off(
) {
    let context = made(FirstMessage::Delivered);
    let following = project_on(&context, "claude-code");
    let chief = at_rest(&context, &following);
    // The human turns specs on for every project, in a folder of its own.
    *context.roster.app_specs.borrow_mut() = AppSpecs {
        on: true,
        folder: Some("docs/specs".to_owned()),
        variant: Some(Variant::Core),
    };
    passes(&context);
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].starts_with("Specs are now on in this project: "),
        "{}",
        notes[0]
    );
    assert!(notes[0].contains(DOCS_SKILL), "{}", notes[0]);
    rests(&context, &following);
    // The app turns them off again: the project that follows is told.
    *context.roster.app_specs.borrow_mut() = AppSpecs::default();
    passes(&context);
    assert_eq!(told(&context, chief).len(), 2);
    rests(&context, &following);

    // A project that says off is told nothing of what the app says.
    says(&context, following.id, Some(false), "", None);
    passes(&context);
    *context.roster.app_specs.borrow_mut() = AppSpecs {
        on: true,
        folder: None,
        variant: None,
    };
    passes(&context);
    assert_eq!(
        told(&context, chief).len(),
        2,
        "the project's own word wins over the app's"
    );
}

#[test]
fn a_setting_that_comes_back_to_what_the_chief_was_told_before_it_rests_tells_it_nothing() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "claude-code");
    let chief = at_rest(&context, &project);
    // Turned on and off again while the chief is in a turn.
    in_a_turn(&context, &project);
    says(&context, project.id, Some(true), "", None);
    passes(&context);
    says(&context, project.id, Some(false), "", None);
    passes(&context);
    answers(&context, &project);
    passes(&context);
    assert!(
        role_notes(&context, chief).is_empty(),
        "{:?}",
        role_notes(&context, chief)
    );
    assert!(context.adapter.specmint_placed.borrow().is_empty());
}

#[test]
fn a_note_that_waits_for_its_turn_is_withdrawn_when_the_setting_changes_again_and_said_anew() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "claude-code");
    let chief = at_rest(&context, &project);
    // Changed twice while the chief is in a turn: nothing is written until it rests.
    in_a_turn(&context, &project);
    says(
        &context,
        project.id,
        Some(true),
        "docs/specs",
        Some(Variant::Core),
    );
    passes(&context);
    says(
        &context,
        project.id,
        Some(true),
        "plans",
        Some(Variant::Core),
    );
    passes(&context);
    assert!(
        role_notes(&context, chief).is_empty(),
        "nothing is written mid-turn"
    );
    // At its rest a look finds it, and a message that comes before the next look goes in
    // ahead of the note, which waits behind it.
    context.adapter.answer("chief", "Noted.");
    context.pass().unwrap();
    context.note(project.id, "human", "chief", "And the parser?");
    context.pass().unwrap();
    let waiting = role_notes(&context, chief);
    assert_eq!(waiting.len(), 1, "written once: {waiting:?}");
    assert_eq!(waiting[0].state, "queued", "{waiting:?}");
    assert!(waiting[0].body.contains("`plans`"));
    // It changes again while the chief reads the message ahead of it.
    says(
        &context,
        project.id,
        Some(true),
        "docs/specs",
        Some(Variant::Core),
    );
    answers(&context, &project);
    let after = role_notes(&context, chief);
    assert_eq!(after.len(), 2, "{after:?}");
    assert_eq!(
        after[0].state, "cancelled",
        "the first is withdrawn: {after:?}"
    );
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].starts_with("Specs are now on in this project: "),
        "the chief was told nothing before, so these are the specs turning on: {}",
        notes[0]
    );
    assert!(notes[0].contains("`docs/specs`"), "{}", notes[0]);
    assert!(
        !notes[0].contains("plans"),
        "only what the project says when the note is written: {}",
        notes[0]
    );
    rests(&context, &project);
    assert_eq!(role_notes(&context, chief).len(), 2, "no third note");
}

#[test]
fn the_rules_for_the_staff_written_changed_and_taken_away_are_told_to_the_chief_and_to_no_member() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "claude-code");
    let chief = at_rest(&context, &project);
    rules(&context, project.id, Some(RULES));
    passes(&context);
    let first = told(&context, chief);
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(
        first[0].starts_with("The human wrote the rules for your staff while you work."),
        "{}",
        first[0]
    );
    assert!(first[0].contains(RULES), "{}", first[0]);
    assert!(
        first[0].contains("those already open keep the rules they were given"),
        "{}",
        first[0]
    );
    rests(&context, &project);
    rules(&context, project.id, Some(CHANGED));
    passes(&context);
    let second = told(&context, chief);
    assert_eq!(second.len(), 2);
    assert!(
        second[1].starts_with("The human changed the rules for your staff while you work."),
        "{}",
        second[1]
    );
    assert!(second[1].contains(CHANGED) && !second[1].contains(RULES));
    rests(&context, &project);
    rules(&context, project.id, None);
    passes(&context);
    let third = told(&context, chief);
    assert_eq!(third.len(), 3);
    assert!(
        third[2].starts_with("The human took away the rules for your staff while you work."),
        "{}",
        third[2]
    );
    // A member of the staff is told nothing: its window is its task's.
    context.give(project.id, "zeus", "Write the parser");
    passes(&context);
    let zeus = context.id(project.id, "zeus");
    assert!(role_notes(&context, zeus).is_empty());
    context.close();
}

#[test]
fn a_member_added_to_the_staff_is_no_news_to_a_running_chief() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "claude-code");
    let chief = at_rest(&context, &project);
    context
        .ledger
        .borrow_mut()
        .add_member(
            project.id,
            &cf_ledger::NewMember {
                agent: "athena".to_owned(),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["advisor".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .unwrap();
    passes(&context);
    assert!(role_notes(&context, chief).is_empty());
}

#[test]
fn a_closed_project_is_told_nothing_and_its_chiefs_next_launch_has_the_role_as_it_is() {
    for harness in ["claude-code", "pi", "devin", "muse"] {
        let context = made(FirstMessage::Delivered);
        let project = project_on(&context, harness);
        let chief = at_rest(&context, &project);
        context.close_project(project.id).unwrap();
        let typed = context.host.inputs().len();
        says(
            &context,
            project.id,
            Some(true),
            "docs/specs",
            Some(Variant::Core),
        );
        rules(&context, project.id, Some(RULES));
        for _ in 0..4 {
            context.pass().unwrap();
        }
        assert!(
            role_notes(&context, chief).is_empty(),
            "{harness}: a closed project has no window to tell"
        );
        assert_eq!(context.host.inputs().len(), typed, "{harness}");
        assert!(
            context.adapter.specmint_placed.borrow().is_empty(),
            "{harness}"
        );
        context.resume_project(project.id).unwrap();
        passes(&context);
        let launches = chief_launches(&context.adapter);
        assert_eq!(launches.len(), 2, "{harness}");
        let instructions = launches[1]["instructions"].as_str().unwrap_or_default();
        assert!(
            instructions.contains("with specs: core in docs/specs"),
            "{harness}: {instructions}"
        );
        assert!(instructions.contains(RULES), "{harness}: {instructions}");
        assert!(
            role_notes(&context, chief).is_empty(),
            "{harness}: its launch has them, nothing says them again"
        );
        context.close();
    }
}

#[test]
fn a_project_opened_with_specs_has_them_in_its_chiefs_launch_and_is_told_nowhere() {
    let context = made(FirstMessage::Delivered);
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "diana" },
            "staff": [],
            "specs": { "on": true, "folder": "docs/specs", "variant": "core" },
        }))
        .unwrap()
        .expect("the project");
    let chief = at_rest(&context, &project);
    let launches = chief_launches(&context.adapter);
    assert_eq!(launches.len(), 1);
    assert!(
        launches[0]["instructions"]
            .as_str()
            .unwrap_or_default()
            .contains("with specs: core in docs/specs"),
        "{}",
        launches[0]["instructions"]
    );
    assert!(role_notes(&context, chief).is_empty());
}

#[test]
fn a_skill_that_cannot_be_put_in_place_is_told_to_the_human_once_and_the_chief_not_at_all() {
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
    passes(&context);
    passes(&context);
    assert!(role_notes(&context, chief).is_empty());
    let heard = to_human(&context, project.id);
    let refused: Vec<&String> = heard
        .iter()
        .filter(|words| words.contains("could not be told"))
        .collect();
    assert_eq!(refused.len(), 1, "{heard:?}");
    assert!(
        refused[0].contains("no space left on device")
            && refused[0].contains("when it next starts"),
        "{}",
        refused[0]
    );
    // A different change is tried anew; the next launch has the specs all the same.
    *context.adapter.place_fails.borrow_mut() = None;
    says(
        &context,
        project.id,
        Some(true),
        "plans",
        Some(Variant::Core),
    );
    passes(&context);
    assert_eq!(told(&context, chief).len(), 1);
}

#[test]
fn a_codex_chief_told_of_a_setting_now_is_not_told_the_same_again_when_it_is_resumed() {
    let context = with_codex();
    context.codex.keeps_first_instructions.set(true);
    context.codex.specmint_installed.replace(Some(
        "/home/me/.agents/skills/specmint-core/SKILL.md".to_owned(),
    ));
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "codex", "agent": "diana" },
            "staff": [{ "agent": "zeus", "harness": "claude-code", "roles": ["worker"], "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project");
    passes(&context);
    let chief = context.id(project.id, "chief");
    let began = {
        let ledger = context.ledger.borrow();
        let current = ledger
            .current_conversation(chief)
            .unwrap()
            .expect("a thread");
        ledger.conversation_role_text(current.id).unwrap()
    };
    assert_eq!(began.as_deref(), Some("instructions for chief"));

    // Specs turned on while it runs: told at its rest, and the thread's record is the new role.
    says(&context, project.id, Some(true), "", Some(Variant::Core));
    passes(&context);
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("Specs are now on in this project: "));
    let with_specs = "instructions for chief with specs: core in .specs at /home/me/.agents/skills/specmint-core/SKILL.md";
    let recorded = {
        let ledger = context.ledger.borrow();
        let current = ledger
            .current_conversation(chief)
            .unwrap()
            .expect("a thread");
        ledger.conversation_role_text(current.id).unwrap()
    };
    assert_eq!(
        recorded.as_deref(),
        Some(with_specs),
        "recorded when it went in"
    );

    // Resumed with nothing changed since: no note says the role again, whole or in part.
    relaunch(&context, project.id);
    passes(&context);
    let launches = chief_launches(&context.codex);
    assert_eq!(launches.len(), 2);
    assert!(launches[1]["resume"].is_string(), "resumed");
    assert_eq!(launches[1]["instructions"], with_specs);
    assert_eq!(
        role_notes(&context, chief).len(),
        1,
        "the note of the change is the only one: {:?}",
        role_notes(&context, chief)
    );

    // A change it was not told of (the window went first) is the resume's, in the whole role.
    says(
        &context,
        project.id,
        Some(true),
        "plans",
        Some(Variant::Core),
    );
    relaunch(&context, project.id);
    passes(&context);
    let notes = role_notes(&context, chief);
    let replacements: Vec<&MessageView> = notes
        .iter()
        .filter(|note| note.body.contains("replace the ones you were given then"))
        .collect();
    assert_eq!(replacements.len(), 1, "{notes:?}");
}

#[test]
fn a_change_that_one_look_finds_and_the_next_does_not_is_told_to_nobody() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "claude-code");
    let chief = at_rest(&context, &project);
    // What the app says is read from a file at each look, and a read that fails reads as
    // specs off: a look that finds the app saying "on" and the next that finds it saying
    // nothing is no change to tell a chief of.
    *context.roster.app_specs.borrow_mut() = AppSpecs {
        on: true,
        folder: None,
        variant: None,
    };
    context.pass().unwrap();
    assert!(
        role_notes(&context, chief).is_empty(),
        "one look is not enough"
    );
    *context.roster.app_specs.borrow_mut() = AppSpecs::default();
    passes(&context);
    assert!(role_notes(&context, chief).is_empty());
    assert!(context.adapter.specmint_placed.borrow().is_empty());
}

#[test]
fn a_note_that_waits_behind_a_message_goes_in_when_the_chief_rests_and_is_not_written_again() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "claude-code");
    let chief = at_rest(&context, &project);
    says(&context, project.id, Some(true), "", Some(Variant::Core));
    context.pass().unwrap();
    // A message comes before the second look: it goes in first, the note waits behind it.
    context.note(project.id, "human", "chief", "Look at the board.");
    context.pass().unwrap();
    let waiting = role_notes(&context, chief);
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0].state, "queued");
    // The chief rests after the message: the note it waits for goes in, the same one.
    answers(&context, &project);
    let after = role_notes(&context, chief);
    assert_eq!(after.len(), 1, "not written again: {after:?}");
    assert_eq!(after[0].id, waiting[0].id);
    assert_eq!(after[0].state, "delivered");
    rests(&context, &project);
    assert_eq!(role_notes(&context, chief).len(), 1);
}

#[test]
fn a_staff_that_changed_while_the_chief_ran_is_still_the_resumes_to_say_whole_though_specs_were_told_now(
) {
    let context = with_codex();
    context.codex.keeps_first_instructions.set(true);
    context.roles.with_staff.set(true);
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "codex", "agent": "diana" },
            "staff": [{ "agent": "zeus", "harness": "claude-code", "roles": ["worker"], "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project");
    passes(&context);
    let chief = context.id(project.id, "chief");
    let recorded = |context: &Context| {
        let ledger = context.ledger.borrow();
        let current = ledger
            .current_conversation(chief)
            .unwrap()
            .expect("a thread");
        ledger.conversation_role_text(current.id).unwrap()
    };
    assert_eq!(
        recorded(&context).as_deref(),
        Some("instructions for chief staff 1")
    );
    // A member joins: no note tells the chief of it (`cf staff` shows the staff as it is).
    context
        .ledger
        .borrow_mut()
        .add_member(
            project.id,
            &cf_ledger::NewMember {
                agent: "athena".to_owned(),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["advisor".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .unwrap();
    passes(&context);
    assert!(role_notes(&context, chief).is_empty());
    // Then specs are turned on: the note is delivered, and the conversation is recorded as
    // told the role it launched with, the specs added: not the staff it was never told.
    says(&context, project.id, Some(true), "", Some(Variant::Core));
    passes(&context);
    assert_eq!(told(&context, chief).len(), 1);
    let skill =
        "/consensflow/integrations/specmint/fake/core/.claude/skills/specmint-core/SKILL.md";
    assert_eq!(
        recorded(&context).as_deref(),
        Some(
            format!("instructions for chief staff 1 with specs: core in .specs at {skill}")
                .as_str()
        )
    );
    // Its resume says the whole role, with the staff that is now: once.
    relaunch(&context, project.id);
    passes(&context);
    let notes = role_notes(&context, chief);
    let replacements: Vec<&MessageView> = notes
        .iter()
        .filter(|note| note.body.contains("replace the ones you were given then"))
        .collect();
    assert_eq!(replacements.len(), 1, "{notes:?}");
    assert!(
        replacements[0].body.ends_with(&format!(
            "instructions for chief staff 2 with specs: core in .specs at {skill}"
        )),
        "{}",
        replacements[0].body
    );
}

#[test]
fn a_skill_that_could_not_be_put_in_place_is_tried_again_when_the_setting_comes_back_to_what_failed(
) {
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
    passes(&context);
    // The human turns them off again (nothing was told, so nothing differs), then on.
    says(&context, project.id, Some(false), "", None);
    passes(&context);
    *context.adapter.place_fails.borrow_mut() = None;
    says(
        &context,
        project.id,
        Some(true),
        "docs/specs",
        Some(Variant::Core),
    );
    passes(&context);
    let notes = told(&context, chief);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("Specs are now on in this project: "));
}

#[test]
fn a_chief_told_of_specs_now_and_then_cleared_is_given_the_role_with_them_and_not_told_again() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = at_rest(&context, &project);
    says(&context, project.id, Some(true), "", Some(Variant::Core));
    passes(&context);
    assert_eq!(told(&context, chief).len(), 1, "told now");
    // A /clear that no hook greeted (Muse's) starts a conversation of its own with no role:
    // the note that tells the first one does not stand for it.
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context.adapter.switch_to("chief", "native-cleared");
    passes(&context);
    let notes = role_notes(&context, chief);
    assert_eq!(
        notes.len(),
        2,
        "the news, and the role for the cleared session: {notes:?}"
    );
    assert!(
        notes[1]
            .body
            .contains("began without your ConsensFlow role instructions"),
        "{}",
        notes[1].body
    );
    assert!(
        notes[1].body.contains("with specs: core in .specs"),
        "the role it is given has them: {}",
        notes[1].body
    );
    assert_eq!(notes[1].state, "delivered");
    // It was told them in that role: nothing tells it again, at the rest after its turn on it.
    rests(&context, &project);
    passes(&context);
    assert_eq!(role_notes(&context, chief).len(), 2);
}

#[test]
fn a_clear_that_comes_with_a_change_one_look_has_found_is_given_the_role_with_it_and_told_no_more()
{
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = at_rest(&context, &project);
    // One look finds the specs turned on: nothing is written yet (two looks in a row tell).
    says(&context, project.id, Some(true), "", Some(Variant::Core));
    context.pass().unwrap();
    assert!(role_notes(&context, chief).is_empty());
    // A /clear no hook greeted comes before the second: the role it is given has the specs,
    // and that is all it is told of them.
    context.adapter.with("chief", |agent| {
        agent.ungreeted.push("native-cleared".into())
    });
    context.adapter.switch_to("chief", "native-cleared");
    passes(&context);
    passes(&context);
    let notes = role_notes(&context, chief);
    assert_eq!(
        notes.len(),
        1,
        "the role for the cleared session alone: {notes:?}"
    );
    assert!(
        notes[0]
            .body
            .contains("began without your ConsensFlow role instructions")
            && notes[0].body.contains("with specs: core in .specs"),
        "{}",
        notes[0].body
    );
    assert_eq!(notes[0].state, "delivered");
    // Its turn on that role over, the chief at rest is told nothing more of them.
    rests(&context, &project);
    passes(&context);
    assert_eq!(
        role_notes(&context, chief).len(),
        1,
        "{:?}",
        role_notes(&context, chief)
    );
}

#[test]
fn a_member_at_rest_while_it_waits_for_an_answer_is_told_nothing_of_the_rules_changed_under_it() {
    let context = made(FirstMessage::Delivered);
    let project = project_on(&context, "claude-code");
    at_rest(&context, &project);
    let task = context.give(project.id, "zeus", "Write the parser").task;
    passes(&context);
    let zeus = context.id(project.id, "zeus");
    // The member asks the chief and ends its turn: its window is open and at rest.
    context.ask(project.id, "zeus", "chief", task.number, "Which parser?");
    context.adapter.answer("zeus", "Asked.");
    passes(&context);
    assert_eq!(
        context.dispatcher.activity(zeus).state,
        cf_engine::ActivityState::Idle,
        "the member's window is at rest"
    );
    // The human changes the rules; the member's window is a task's, the next one is given them.
    rules(&context, project.id, Some(RULES));
    passes(&context);
    passes(&context);
    assert!(
        role_notes(&context, zeus).is_empty(),
        "{:?}",
        role_notes(&context, zeus)
    );
}
