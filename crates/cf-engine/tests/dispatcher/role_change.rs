//! A role that changed under a conversation whose harness keeps its first
//! instructions (Codex): the launch that resumes the conversation opens its
//! window with no first message, and once the window is up it is given a note
//! from ConsensFlow first, the new text in full and said to replace the earlier
//! one, then the message the launch would have begun with, then the rest of what
//! waits; the text recorded, once the note is delivered, is then the new one, so
//! it is given once. A conversation that is
//! fresh, a role that did not change and a harness that takes its role at every
//! launch are given nothing extra. What is less plain (a window that never took
//! it, a text nobody recorded, a skill copy of ConsensFlow's, a variant that
//! changed) is in `unusual`, a member's session in `members`, a launch that
//! cannot say the role (its window exits, is never ready, or refuses it) in
//! `launch` for a member and `chief_launch` for the chief, what tells a
//! replacement from any other message in `identity`, and a conversation whose
//! harness holds nothing of it, which opens afresh and is told no role, in
//! `fresh`.
//! Node's dispatcher had no such harness, so no trace holds these.

mod chief_launch;
mod cleared;
mod fresh;
mod identity;
mod launch;
mod members;
mod skill;
mod told;
mod unusual;

use cf_engine::testing::{Context, FakeAdapter};
use cf_engine::SwitchWhen;
use cf_harness::records::Role;
use cf_ledger::{MessageView, ProjectView, SpecsChange};
use cf_proto::specs::Variant;
use serde_json::{json, Value};

use crate::chiefs::{to, with_codex};

/// The words that tell a note that replaces a role from any other.
const REPLACES: &str = "replace the ones you were given then";

/// Where the human has Specmint installed for Codex, so that the text a chief
/// is given names a path that stays from one launch to the next.
const SKILL: &str = "/home/me/.agents/skills/specmint-core/SKILL.md";

/// The chief's role text with specs on, as the fake role texts write it.
const WITH_SPECS: &str = "instructions for chief with specs: core in .specs at /home/me/.agents/skills/specmint-core/SKILL.md";

/// A test's engine whose Codex keeps the first instructions of its threads, as
/// the real one does.
fn codex() -> Context {
    let context = with_codex();
    context.codex.keeps_first_instructions.set(true);
    context
        .codex
        .specmint_installed
        .replace(Some(SKILL.to_owned()));
    context
}

/// A project whose chief is on `harness`, specs off: diana its chief, zeus a worker.
fn project_of(context: &Context, harness: &str) -> ProjectView {
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

/// The human turns specs on for the project, or off.
fn specs(context: &Context, project: i64, on: bool) {
    specs_of(context, project, on, Variant::Core);
}

/// The human turns specs on for the project, or off, with the variant `variant`.
fn specs_of(context: &Context, project: i64, on: bool, variant: Variant) {
    context
        .ledger
        .borrow_mut()
        .set_specs(
            project,
            &SpecsChange {
                on: Some(on),
                folder: None,
                variant: Some(variant),
            },
        )
        .unwrap();
}

/// The human closes the project and resumes it: the chief's window opens again.
fn relaunch(context: &Context, project: i64) {
    context.close_project(project).unwrap();
    context.resume_project(project).unwrap();
}

/// The launches of the chief's window on `adapter`, oldest first.
fn chief_launches(adapter: &FakeAdapter) -> Vec<Value> {
    adapter
        .prepared()
        .into_iter()
        .filter(|launch| launch["role"] == "chief")
        .collect()
}

/// The notes ConsensFlow wrote to `participant` that replace its role, oldest first.
fn replacements(context: &Context, participant: i64) -> Vec<MessageView> {
    let mut notes: Vec<MessageView> = context
        .inbox(participant)
        .into_iter()
        .filter(|message| {
            message.sender.is_none() && message.kind == "note" && message.body.contains(REPLACES)
        })
        .collect();
    notes.reverse();
    notes
}

/// The role text the participant's current conversation is recorded as last given.
fn recorded(context: &Context, participant: i64) -> Option<String> {
    let ledger = context.ledger.borrow();
    let current = ledger
        .current_conversation(participant)
        .unwrap()
        .expect("a conversation");
    ledger.conversation_role_text(current.id).unwrap()
}

/// What the user said to the chief's window, in order: what its record holds.
fn said_to_the_chief(context: &Context) -> Vec<String> {
    context
        .codex
        .agent("chief")
        .items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| item.text.to_string())
        .collect()
}

#[test]
fn a_resumed_conversation_whose_harness_keeps_its_first_instructions_is_given_a_changed_role_first_and_once(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");

    // It began with the role it had with specs off, and a fresh conversation is
    // given nothing extra: its role is the text it starts with.
    let began = chief_launches(&context.codex);
    assert_eq!(began.len(), 1);
    assert_eq!(began[0]["instructions"], "instructions for chief");
    assert_eq!(began[0]["message"], Value::Null);
    assert!(replacements(&context, chief).is_empty());
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief"),
        "the ledger has the text it began with"
    );

    // The human turns specs on while it runs: its window is as it was. Close, Resume.
    specs(&context, project.id, true);
    assert_eq!(chief_launches(&context.codex).len(), 1);
    relaunch(&context, project.id);

    // The window resumed its thread, with no first message: the role is on its
    // command line once, as its instructions, and not again as a message.
    let launches = chief_launches(&context.codex);
    assert_eq!(launches.len(), 2);
    let native = format!("native-{}", began[0]["launchId"].as_str().unwrap());
    assert_eq!(launches[1]["resume"], native);
    assert_eq!(launches[1]["instructions"], WITH_SPECS);
    assert_eq!(launches[1]["message"], Value::Null);
    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 1, "one replacement, written for this launch");
    let told = &notes[0];
    assert!(
        told.body.ends_with(&format!("\n\n{WITH_SPECS}")),
        "the role text in full, last: {}",
        told.body
    );
    assert!(
        told.body.contains("follow these from now on"),
        "{}",
        told.body
    );
    // The conversation has not been told yet: what is recorded is what it has.
    assert_eq!(context.message(told.id).state, "queued");
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief")
    );

    // Once the window is up, the role is the first thing it is given, whole, as
    // any message is, by paste; and it is recorded as told once the window's own
    // record shows it, and not before.
    context.pass().unwrap();
    assert_eq!(context.message(told.id).state, "delivering");
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief"),
        "handed over, not yet seen to have arrived"
    );
    let said = said_to_the_chief(&context);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with(&format!(
            "[ConsensFlow m-{} · note from ConsensFlow]\n",
            told.id
        )),
        "the replacement, under its header: {}",
        said[0]
    );
    assert!(said[0].contains(WITH_SPECS));
    context.pass().unwrap();
    assert_eq!(context.message(told.id).state, "delivered");
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some(WITH_SPECS),
        "told, and recorded as told"
    );

    // The window answers, and the next Resume has nothing to say of the role.
    context.codex.answer("chief", "Understood.");
    context.pass().unwrap();
    relaunch(&context, project.id);
    let launches = chief_launches(&context.codex);
    assert_eq!(launches.len(), 3);
    assert_eq!(launches[2]["resume"], native);
    assert_eq!(launches[2]["instructions"], WITH_SPECS);
    assert_eq!(
        launches[2]["message"],
        Value::Null,
        "given once, not at every launch"
    );
    assert_eq!(replacements(&context, chief).len(), 1);
    context.close();
}

#[test]
fn a_resumed_conversation_whose_role_did_not_change_is_given_nothing_extra() {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    relaunch(&context, project.id);
    relaunch(&context, project.id);
    let launches = chief_launches(&context.codex);
    assert_eq!(launches.len(), 3);
    for resumed in &launches[1..] {
        assert!(resumed["resume"].is_string());
        assert_eq!(resumed["instructions"], "instructions for chief");
        assert_eq!(resumed["message"], Value::Null);
    }
    assert!(replacements(&context, chief).is_empty());
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief")
    );
    context.close();
}

#[test]
fn a_harness_that_takes_its_role_at_every_launch_is_given_nothing_extra_whether_the_role_changed_or_not(
) {
    // Claude Code, and a Codex that is not said to keep its first instructions.
    for (context, harness) in [(Context::new(), "claude-code"), (with_codex(), "codex")] {
        let project = project_of(&context, harness);
        let chief = context.id(project.id, "chief");
        let launches = |context: &Context| {
            chief_launches(if harness == "codex" {
                &context.codex
            } else {
                &context.adapter
            })
        };
        specs(&context, project.id, true);
        relaunch(&context, project.id);
        specs(&context, project.id, false);
        relaunch(&context, project.id);
        let all = launches(&context);
        assert_eq!(all.len(), 3, "{harness}");
        assert!(
            all[1]["instructions"]
                .as_str()
                .is_some_and(|text| text.contains("with specs")),
            "{harness}: the harness is given the changed role itself"
        );
        for launch in &all {
            assert_eq!(launch["message"], Value::Null, "{harness}");
        }
        assert!(replacements(&context, chief).is_empty(), "{harness}");
        let current = context
            .ledger
            .borrow()
            .current_conversation(chief)
            .unwrap()
            .expect("a conversation");
        assert_eq!(
            context
                .ledger
                .borrow()
                .conversation_role_text(current.id)
                .unwrap(),
            None,
            "{harness}: nothing is recorded of a harness that never needs it"
        );
        context.close();
    }
}

#[test]
fn a_fresh_conversation_is_given_the_role_it_has_now_and_nothing_extra() {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    // The chief has worked a turn, which a chief that takes over reads of.
    context.note(project.id, "human", "chief", "Plan the work.");
    context.pass().unwrap();
    context.codex.answer("chief", "On it.");
    context.pass().unwrap();
    specs(&context, project.id, true);
    // The human switches the chief to another Codex agent: a new conversation,
    // whose first message is the handoff and whose role is what it starts with.
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    let launches = chief_launches(&context.codex);
    assert_eq!(launches.len(), 2);
    let fresh = &launches[1];
    assert_eq!(fresh["resume"], Value::Null);
    assert_eq!(fresh["instructions"], WITH_SPECS);
    assert!(
        fresh["message"]
            .as_str()
            .is_some_and(|text| text.contains("You are the chief now")),
        "the handoff, and no replacement: {fresh}"
    );
    assert!(replacements(&context, chief).is_empty());
    assert_eq!(recorded(&context, chief).as_deref(), Some(WITH_SPECS));
    context.close();
}

/// The window takes what it was handed, the engine sees it arrive, and the
/// window answers: what a message goes through before the next one goes in.
fn through_a_turn(context: &Context) {
    context.pass().unwrap();
    context.pass().unwrap();
    context.codex.answer("chief", "Understood.");
}

#[test]
fn a_replacement_goes_ahead_of_the_words_that_waited_for_the_window_and_ahead_of_the_rest_and_those_follow_it_in_their_order(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    // Two sets of words wait; the first of them was to be the launch's first message.
    let first = context.note(project.id, "human", "chief", "Any news?");
    let second = context.note(project.id, "human", "chief", "And the tests?");
    context.resume_project(project.id).unwrap();

    let launches = chief_launches(&context.codex);
    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].id > second.id,
        "written after what waited, which is why the queue alone would say it last"
    );
    assert_eq!(
        launches[1]["message"],
        Value::Null,
        "the window opens with no first message"
    );
    for message in [&first, &second] {
        assert_eq!(
            context.message(message.id).state,
            "queued",
            "what waited is still waiting"
        );
    }

    // The role goes in first, then the words that were to be first, then the rest.
    through_a_turn(&context);
    assert_eq!(context.message(notes[0].id).state, "delivered");
    assert_eq!(context.message(first.id).state, "queued");
    assert_eq!(said_to_the_chief(&context).len(), 1);
    through_a_turn(&context);
    assert_eq!(context.message(first.id).state, "delivered");
    assert_eq!(context.message(second.id).state, "queued");
    through_a_turn(&context);
    assert_eq!(context.message(second.id).state, "delivered");
    let texts = said_to_the_chief(&context);
    assert_eq!(texts.len(), 3, "{texts:?}");
    assert!(texts[0].contains(REPLACES), "{}", texts[0]);
    assert!(texts[1].contains("Any news?"), "{}", texts[1]);
    assert!(texts[2].contains("And the tests?"), "{}", texts[2]);
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some(WITH_SPECS),
        "the words that followed the role are no role: what it was told is what it was"
    );
    context.close();
}

#[test]
fn a_replacement_goes_ahead_of_the_note_that_tells_the_chief_its_turn_was_cut_and_that_one_follows_it_ahead_of_the_rest(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    // The human's words begin a turn that is not over when the project is closed.
    context.note(project.id, "human", "chief", "Plan the work.");
    context.pass().unwrap();
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    let waiting = context.note(project.id, "human", "chief", "Any news?");
    context.resume_project(project.id).unwrap();

    // The chief is told its turn was cut, and its role is replaced: the role first.
    let launches = chief_launches(&context.codex);
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[1]["message"], Value::Null);
    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 1);
    let cut: Vec<MessageView> = context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.body.contains("stopped in the middle of your turn"))
        .collect();
    assert_eq!(cut.len(), 1, "told its turn was cut, once");
    assert_eq!(cut[0].state, "queued", "and not yet: it follows the role");

    // The role, then the note that its turn was cut, then what waited.
    through_a_turn(&context);
    assert_eq!(context.message(notes[0].id).state, "delivered");
    assert_eq!(context.message(cut[0].id).state, "queued");
    through_a_turn(&context);
    assert_eq!(context.message(cut[0].id).state, "delivered");
    assert_eq!(context.message(waiting.id).state, "queued");
    through_a_turn(&context);
    assert_eq!(context.message(waiting.id).state, "delivered");
    let texts = said_to_the_chief(&context);
    assert_eq!(texts.len(), 3, "{texts:?}");
    assert!(texts[0].contains(REPLACES), "{}", texts[0]);
    assert!(
        texts[1].contains("stopped in the middle of your turn"),
        "{}",
        texts[1]
    );
    assert!(texts[2].contains("Any news?"), "{}", texts[2]);
    context.close();
}
