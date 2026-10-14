//! A conversation that is not resumed has nothing to be told. Whether a window goes
//! on with its conversation is what its harness says of it (`resumable`, and a
//! Codex thread that never had a turn has no record for a resume to find), and a
//! launch decides that first: a conversation its harness holds nothing of opens
//! its window afresh, with the role as it is now on its command line, and is given
//! no note that says a role changed. Were the role compared with the conversation
//! the ledger holds before the harness is asked, a note would be written for a
//! conversation the window does not open on, saying a role it already has. So is
//! a launch given up while its harness is asked (a pause came): it has written
//! nothing of a role, which the launch that follows says first. Node's
//! dispatcher asked its harness nothing of the kind, so no trace holds these.

use cf_engine::testing::Context;
use serde_json::Value;

use super::members::{finished_session, follow_up, said_to};
use super::{
    chief_launches, codex, project_of, recorded, relaunch, replacements, said_to_the_chief, specs,
    REPLACES, WITH_SPECS,
};
use crate::fixtures::native_of;

#[test]
fn a_chief_whose_harness_holds_nothing_of_its_conversation_opens_afresh_with_its_role_and_is_told_no_other(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    assert_eq!(chief_launches(&context.codex).len(), 1);

    // The human turns specs on, and the chief is closed before its first turn:
    // what Codex kept of its thread is nothing (it has no rollout), and the role
    // the thread began with is the old one all the same, in the ledger.
    specs(&context, project.id, true);
    context.codex.holds_nothing("chief");
    relaunch(&context, project.id);

    let launches = chief_launches(&context.codex);
    assert_eq!(launches.len(), 2);
    assert_eq!(
        launches[1]["resume"],
        Value::Null,
        "a thread with no record is none to resume: the window opens afresh"
    );
    assert_eq!(
        launches[1]["instructions"], WITH_SPECS,
        "the role as it is now is what it begins with"
    );
    assert!(
        replacements(&context, chief).is_empty(),
        "nothing is written to tell it a role it begins with"
    );
    assert!(
        context
            .ledger
            .borrow()
            .role_notes_to(chief)
            .unwrap()
            .is_empty(),
        "and the ledger knows of no note that says it a role"
    );
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some(WITH_SPECS),
        "its conversation is recorded as begun with the role it has now"
    );

    // Nor is one given once the window is up, or at any pass after it.
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(replacements(&context, chief).is_empty());
    assert!(context
        .ledger
        .borrow()
        .role_notes_to(chief)
        .unwrap()
        .is_empty());
    assert!(
        said_to_the_chief(&context)
            .iter()
            .all(|text| !text.contains(super::REPLACES)),
        "{:?}",
        said_to_the_chief(&context)
    );
    context.close();
}

#[test]
fn a_member_whose_harness_holds_nothing_of_its_conversation_opens_afresh_with_its_brief_and_is_told_no_role(
) {
    let context: Context = codex();
    let (project, session, member) = finished_session(&context);

    // A release rewrote the texts; the chief gives the session a follow-up, and
    // its harness holds nothing of the conversation it was on.
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    context.codex.holds_nothing(&session);
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    context.pass().unwrap();

    let launches = context.codex.prepared();
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[1]["resume"], Value::Null, "afresh");
    assert_eq!(
        launches[1]["instructions"],
        "instructions for worker (rewritten)"
    );
    assert!(
        launches[1]["message"]
            .as_str()
            .is_some_and(|message| message.contains("Now the lexer")),
        "the brief is the first message of the window, on its command line: {}",
        launches[1]
    );
    assert!(
        replacements(&context, member).is_empty(),
        "no note says a role it begins with"
    );
    assert!(
        context
            .ledger
            .borrow()
            .role_notes_to(member)
            .unwrap()
            .is_empty(),
        "and the ledger knows of no note that says it a role"
    );
    assert_eq!(
        recorded(&context, member).as_deref(),
        Some("instructions for worker (rewritten)"),
        "its new conversation is recorded as begun with the role it has now"
    );

    // The brief is the one thing the window is given, by the launch.
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert!(replacements(&context, member).is_empty());
    assert!(context
        .ledger
        .borrow()
        .role_notes_to(member)
        .unwrap()
        .is_empty());
    assert_eq!(context.task(project.id, 2).task.state, "working");
    assert!(
        said_to(&context, &session)
            .iter()
            .all(|text| !text.contains(super::REPLACES)),
        "{:?}",
        said_to(&context, &session)
    );
    context.close();
}

#[test]
fn a_member_whose_launch_is_given_up_while_its_harness_is_asked_has_written_nothing_of_a_role_and_the_next_launch_says_it_first(
) {
    let context = codex();
    let (project, session, member) = finished_session(&context);
    context.roles.rewritten.replace(" (rewritten)".to_owned());
    let conversation = native_of(&context, member).expect("its conversation");
    let asking = context
        .codex
        .resumable_holds
        .hold(move |asked| asked[0] == conversation.as_str());

    // The follow-up begins a launch, which waits for what the harness says of the
    // conversation it would resume.
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    let brief = context
        .inbox(member)
        .into_iter()
        .find(|message| message.kind == "task" && message.task_number == Some(2))
        .expect("the follow-up's brief");
    assert_eq!(brief.state, "delivering", "its launch has begun it");

    // The pause comes while the harness is asked: the launch is given up.
    context.human_pause(project.id, 2);
    asking.open();
    context.settle();
    assert_eq!(context.codex.prepared().len(), 1, "no window is prepared");
    assert_eq!(context.message(brief.id).state, "queued", "given back");
    assert!(
        context
            .ledger
            .borrow()
            .role_notes_to(member)
            .unwrap()
            .is_empty(),
        "and a launch that was given up has written nothing of a role"
    );

    // The words that resume the task bring the window back, and the role is the
    // first thing it is told.
    context.resume_task(project.id, 2, "Go on");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    assert_eq!(context.codex.prepared().len(), 2);
    assert_eq!(
        context.ledger.borrow().role_notes_to(member).unwrap().len(),
        1,
        "the launch that came wrote it, once"
    );
    context.codex.answer(&session, "Understood.");
    for _ in 0..3 {
        context.pass().unwrap();
    }
    let said = said_to(&context, &session);
    assert!(said[0].contains(REPLACES), "the role first: {said:?}");
    assert!(
        said[1..].iter().any(|text| text.contains("Now the lexer")),
        "and then the follow-up: {said:?}"
    );
    context.close();
}
