//! What a replacement does where things are not as plain as a chief resumed once:
//! a window that never took it, a daemon that stopped with it on its way, a
//! conversation whose text nobody recorded, and a member's session resumed for a
//! follow-up.

use std::rc::Rc;

use cf_engine::SwitchWhen;
use cf_harness::records::Role;
use cf_proto::specs::Variant;
use serde_json::{json, Value};

use super::{
    chief_launches, codex, project_of, recorded, relaunch, replacements, specs, specs_of, REPLACES,
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

    // The human changes the role again, and the launch goes through.
    context.codex.prepare.replace(None);
    specs_of(&context, project.id, true, Variant::Tdd);
    context.advance(10 * 60_000);
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
fn a_replacement_in_flight_when_the_daemon_stopped_is_recorded_as_told_once_the_restart_finds_it_in_the_copy_of_its_window(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    // The window takes the role as its first message, and the harness's own record
    // shows it; the daemon stops before it confirmed it, and its copy of the
    // window never held it.
    context.resume_project(project.id).unwrap();
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

#[test]
fn a_members_replacement_that_failed_to_land_leaves_the_record_as_it_was_and_its_next_launch_says_the_role_again(
) {
    let context = codex();
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [{ "agent": "diana", "harness": "codex", "roles": ["worker"], "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project");
    context.pool_task(project.id, "worker", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = context
        .project(project.id)
        .participants
        .into_iter()
        .find(|participant| participant.member_id.is_some())
        .expect("a session")
        .handle;
    let member = context.id(project.id, &session);
    context.codex.answer(&session, "Parser done");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");

    // A release rewrote the texts; the chief gives the session a follow-up, and
    // the session's next launch fails before its window opens. The role it was to
    // be told was that launch's first message, and failed with it.
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    context
        .codex
        .prepare
        .replace(Some(Rc::new(|_| Err("no Codex".to_owned()))));
    context.create_task(
        project.id,
        cf_ledger::NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..cf_ledger::NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let failed = replacements(&context, member);
    assert_eq!(failed.len(), 1);
    assert_eq!(context.message(failed[0].id).state, "failed");
    assert_eq!(
        recorded(&context, member).as_deref(),
        Some("instructions for worker"),
        "the session was told nothing: the record is what it has"
    );
    assert_eq!(
        context.task(project.id, 2).task.state,
        "failed",
        "the brief it went ahead of fails with the launch, as it always did"
    );
    // And that is the end of it: no launch is tried again at every pass, with a
    // new replacement written ahead of nothing each time.
    for _ in 0..3 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(
        replacements(&context, member).len(),
        1,
        "no other is written"
    );

    // The window can open now. The chief gives the session another follow-up,
    // and its launch says the role again, first.
    context.codex.prepare.replace(None);
    context.create_task(
        project.id,
        cf_ledger::NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer, again".to_owned(),
            ..cf_ledger::NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let notes = replacements(&context, member);
    assert_eq!(
        notes.len(),
        2,
        "the role is said again, as a note of its own"
    );
    let launches = context.codex.prepared();
    let first = launches
        .last()
        .and_then(|launch| launch["message"].as_str())
        .expect("a first message");
    assert!(
        first.contains(REPLACES) && first.ends_with("\n\ninstructions for worker (rewritten)"),
        "{first}"
    );
    context.codex.answer(&session, "Understood.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(notes[1].id).state, "delivered");
    assert_eq!(
        recorded(&context, member).as_deref(),
        Some("instructions for worker (rewritten)"),
        "told, and recorded as told, once it landed"
    );
    context.close();
}

#[test]
fn a_members_session_resumed_for_a_follow_up_is_given_a_rewritten_role_first_and_the_follow_up_after_it(
) {
    let context = codex();
    let project = context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": "claude-code", "agent": "apollo" },
            "staff": [{ "agent": "diana", "harness": "codex", "roles": ["worker"], "tier": "standard" }],
        }))
        .unwrap()
        .expect("the project");
    context.pool_task(project.id, "worker", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = context
        .project(project.id)
        .participants
        .into_iter()
        .find(|participant| participant.member_id.is_some())
        .expect("a session")
        .handle;
    let started = context.codex.prepared();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0]["instructions"], "instructions for worker");
    context.codex.answer(&session, "Parser done");
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "done");

    // A release rewrote the texts; the chief gives the session a follow-up.
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    context.create_task(
        project.id,
        cf_ledger::NewTask {
            from: "chief".to_owned(),
            after: Some(1),
            body: "Now the lexer".to_owned(),
            ..cf_ledger::NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();

    let launches = context.codex.prepared();
    assert_eq!(launches.len(), 2);
    assert!(launches[1]["resume"].is_string(), "{}", launches[1]);
    assert_eq!(
        launches[1]["instructions"],
        "instructions for worker (rewritten)"
    );
    let first = launches[1]["message"].as_str().expect("a first message");
    assert!(
        first.contains(REPLACES) && first.ends_with("\n\ninstructions for worker (rewritten)"),
        "the rewritten role is first: {first}"
    );
    let follow_up = context.task(project.id, 2);
    assert_eq!(
        follow_up.task.state, "queued",
        "the follow-up waits for its turn, and has not been lost"
    );
    context.codex.answer(&session, "Understood.");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 2).task.state, "working");
    let texts: Vec<String> = context
        .codex
        .agent(&session)
        .items
        .iter()
        .filter(|item| item.role == Role::User)
        .map(|item| item.text.to_string())
        .collect();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(texts[0].contains(REPLACES), "{}", texts[0]);
    assert!(texts[1].contains("Now the lexer"), "{}", texts[1]);
    context.close();
}
