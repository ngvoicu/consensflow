//! What a replacement does where things are not as plain as a chief resumed once:
//! a window that never took it, a daemon that stopped with it on its way, and a
//! conversation whose text nobody recorded. A member's session is in `members`.

use std::rc::Rc;

use cf_engine::SwitchWhen;
use cf_ledger::MessageView;
use cf_proto::specs::Variant;
use serde_json::Value;

use super::{
    chief_launches, codex, project_of, recorded, relaunch, replacements, said_to_the_chief, specs,
    specs_of, REPLACES,
};
use crate::chiefs::to;

#[test]
fn a_replacement_the_window_never_took_leaves_the_record_as_it_was_and_the_next_launch_withdraws_it_and_says_the_role_it_has_then(
) {
    let context = codex();
    context.codex.specmint_installed.replace(None);
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs_of(&context, project.id, true, Variant::Core);
    // The next launch fails before the window opens: its replacement goes back to the queue.
    context
        .codex
        .prepare
        .replace(Some(Rc::new(|_| Err("no Codex".to_owned()))));
    context.resume_project(project.id).unwrap();
    let first = replacements(&context, chief);
    assert_eq!(first.len(), 1);
    assert_eq!(context.message(first[0].id).state, "queued");
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief"),
        "the conversation has been told nothing: the record is what it has"
    );

    // The human changes the role again, and the launch goes through: its window
    // is opened, the role pasted, and the window's own record shows it.
    context.codex.prepare.replace(None);
    specs_of(&context, project.id, true, Variant::Tdd);
    context.advance(10 * 60_000);
    context.pass().unwrap();
    context.pass().unwrap();
    context.pass().unwrap();

    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 2);
    assert_eq!(
        context.message(first[0].id).state,
        "cancelled",
        "the older one says what is not the role now"
    );
    let newest = &notes[1];
    assert!(
        newest.body.contains("specmint/fake/tdd/"),
        "the role as it is now: {}",
        newest.body
    );
    assert_eq!(context.message(newest.id).state, "delivered");
    assert_eq!(
        recorded(&context, chief),
        Some(
            newest
                .body
                .split_once("\n\n")
                .expect("a role after its opening")
                .1
                .to_owned()
        ),
        "recorded as told once it was"
    );
    context.close();
}

#[test]
fn a_replacement_the_window_never_took_is_withdrawn_and_nothing_is_said_where_the_role_is_again_what_the_conversation_was_told(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    context
        .codex
        .prepare
        .replace(Some(Rc::new(|_| Err("no Codex".to_owned()))));
    context.resume_project(project.id).unwrap();
    let first = replacements(&context, chief);
    assert_eq!(first.len(), 1);

    // The human takes the specs off again: the role is the one the conversation began
    // with, and the note that waits says what it never was told and no longer is.
    context.codex.prepare.replace(None);
    specs(&context, project.id, false);
    context.advance(10 * 60_000);
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(context.message(first[0].id).state, "cancelled");
    assert_eq!(
        replacements(&context, chief).len(),
        1,
        "no other is written"
    );
    let launches = chief_launches(&context.codex);
    let last = launches.last().expect("a launch");
    assert!(last["resume"].is_string(), "{last}");
    assert_eq!(last["instructions"], "instructions for chief");
    assert_eq!(
        last["message"],
        Value::Null,
        "the withdrawn note is not its first message"
    );
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief")
    );
    context.close();
}

#[test]
fn a_replacement_that_waited_for_a_conversation_is_withdrawn_when_the_chief_begins_another_and_is_never_told_to_it(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    // The chief has worked a turn, which a chief that takes over reads of.
    context.note(project.id, "human", "chief", "Plan the work.");
    context.pass().unwrap();
    context.codex.answer("chief", "On it.");
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    // The next launch fails before the window opens: its replacement goes back to the queue.
    context
        .codex
        .prepare
        .replace(Some(Rc::new(|_| Err("no Codex".to_owned()))));
    context.resume_project(project.id).unwrap();
    let waiting = replacements(&context, chief);
    assert_eq!(waiting.len(), 1);
    assert_eq!(context.message(waiting[0].id).state, "queued");

    // The human turns specs off and has another Codex agent take over: a
    // conversation of its own, which the replacement was not written for.
    context.codex.prepare.replace(None);
    specs(&context, project.id, false);
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    assert_eq!(
        context.message(waiting[0].id).state,
        "cancelled",
        "it would tell the new chief a role that is no longer its role"
    );
    let launches = chief_launches(&context.codex);
    let fresh = launches.last().expect("a launch");
    assert_eq!(fresh["resume"], Value::Null);
    assert_eq!(fresh["instructions"], "instructions for chief");
    assert!(
        fresh["message"]
            .as_str()
            .is_some_and(|text| text.contains("You are the chief now") && !text.contains(REPLACES)),
        "the handoff alone: {fresh}"
    );
    assert_eq!(
        replacements(&context, chief).len(),
        1,
        "none is written for it"
    );
    context.close();
}

#[test]
fn a_handoff_that_waited_for_a_window_which_closed_before_showing_it_goes_after_the_replacement_and_ahead_of_the_rest(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    // The chief has worked a turn, which a chief that takes over reads of.
    context.note(project.id, "human", "chief", "Plan the work.");
    context.pass().unwrap();
    context.pass().unwrap();
    context.codex.answer("chief", "On it.");
    context.pass().unwrap();

    // The human has another Codex agent take over. Its window opens, never shows
    // the handoff it was given, and closes: the handoff waits for the next.
    let codex = Rc::downgrade(&context.codex);
    *context.codex.after_prepare.borrow_mut() = Some(Rc::new(move |handle| {
        if let Some(codex) = codex.upgrade() {
            codex.with(handle, |agent| agent.items.clear());
        }
    }));
    context
        .switch_chief(project.id, to("codex", "astraeus"), SwitchWhen::Now, false)
        .unwrap();
    context.close_project(project.id).unwrap();
    context.codex.after_prepare.replace(None);
    let handoff: Vec<MessageView> = context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.body.contains("You are the chief now"))
        .collect();
    assert_eq!(handoff.len(), 1);
    assert_eq!(handoff[0].state, "queued", "it never arrived");

    // The human turns specs on, something else waits, and the chief is resumed.
    specs(&context, project.id, true);
    let waiting = context.note(project.id, "human", "chief", "Any news?");
    context.resume_project(project.id).unwrap();
    let launches = chief_launches(&context.codex);
    let last = launches.last().expect("a launch");
    assert!(last["resume"].is_string(), "{last}");
    assert_eq!(
        last["message"],
        Value::Null,
        "neither the role nor the handoff is on the command line"
    );
    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 1);

    // The role, then the handoff, then what waited.
    for message in [notes[0].id, handoff[0].id, waiting.id] {
        assert_ne!(context.message(message).state, "delivered");
        context.pass().unwrap();
        context.pass().unwrap();
        context.codex.answer("chief", "Understood.");
        assert_eq!(context.message(message).state, "delivered");
    }
    let texts = said_to_the_chief(&context);
    assert_eq!(texts.len(), 3, "{texts:?}");
    assert!(texts[0].contains(REPLACES), "{}", texts[0]);
    assert!(texts[1].contains("You are the chief now"), "{}", texts[1]);
    assert!(texts[2].contains("Any news?"), "{}", texts[2]);
    context.close();
}

#[test]
fn a_replacement_in_flight_when_the_daemon_stopped_is_recorded_as_told_once_the_restart_finds_it_in_the_copy_of_its_window(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    // The window takes the role by paste, and the harness's own record shows it;
    // the daemon stops before it confirmed it, and its copy of the window never
    // held it.
    context.resume_project(project.id).unwrap();
    context.pass().unwrap();
    let note = replacements(&context, chief).remove(0);
    assert_eq!(context.message(note.id).state, "delivering");
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief"),
        "not told, as far as the daemon knows"
    );

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    context.make().settle_after_restart().unwrap();
    assert_eq!(context.message(note.id).state, "delivered");
    assert!(
        recorded(&context, chief)
            .as_deref()
            .is_some_and(|told| told.contains("with specs")),
        "told, and recorded as told: {:?}",
        recorded(&context, chief)
    );
    context.close();
}

#[test]
fn a_replacement_a_restart_finds_in_no_record_goes_back_to_the_queue_and_is_not_recorded() {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    context.resume_project(project.id).unwrap();
    context.pass().unwrap();
    let note = replacements(&context, chief).remove(0);
    assert_eq!(context.message(note.id).state, "delivering");
    // The window opened, but its record never showed the note before the stop.
    context.codex.with("chief", |agent| agent.items.clear());

    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    context.make().settle_after_restart().unwrap();
    assert_eq!(
        context.message(note.id).state,
        "queued",
        "it never showed: it waits for a window"
    );
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief")
    );
    context.close();
}

#[test]
fn a_conversation_whose_text_is_not_known_counts_as_changed_and_is_told_once() {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    // A conversation from before the record, or one the human switched their
    // window to: the ledger has no text for it.
    let legacy = context
        .ledger
        .borrow_mut()
        .start_conversation(chief, "codex")
        .unwrap();
    context
        .ledger
        .borrow_mut()
        .bind_conversation(legacy.id, "thread-legacy")
        .unwrap();
    assert_eq!(
        context
            .ledger
            .borrow()
            .conversation_role_text(legacy.id)
            .unwrap(),
        None
    );
    relaunch(&context, project.id);
    let launches = chief_launches(&context.codex);
    assert_eq!(launches[1]["resume"], "thread-legacy");
    assert_eq!(replacements(&context, chief).len(), 1, "told");
    assert_eq!(recorded(&context, chief), None, "and not yet recorded");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief"),
        "and recorded as told, once it was"
    );
    context.codex.answer("chief", "Understood.");
    context.pass().unwrap();
    relaunch(&context, project.id);
    assert_eq!(replacements(&context, chief).len(), 1, "and told once");
    assert_eq!(chief_launches(&context.codex)[2]["message"], Value::Null);
    context.close();
}
