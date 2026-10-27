//! The note a Claude launch gives the human when Claude Code's computer use is on
//! for the project's repository and ConsensFlow lacks a macOS permission it needs
//! (T-540): once for each missing state of a project, not at every launch nor at
//! every start of the app, and said again once mended and lost. No case of Node's,
//! which had no such note.

use cf_engine::computer_use::{note_for, NOTE_OPENING};
use cf_engine::seams::ScreenPermissions;
use cf_engine::testing::Context;
use cf_ledger::MessageView;

use crate::fixtures::Tiers;

/// The project's folder in the test context.
const FOLDER: &str = "/work/app";

/// The human's notes of computer use, oldest first.
fn computer_use_notes(context: &Context, project: i64) -> Vec<MessageView> {
    let mut notes: Vec<MessageView> = context
        .inbox(context.id(project, "human"))
        .into_iter()
        .filter(|message| message.kind == "note" && message.body.starts_with(NOTE_OPENING))
        .collect();
    notes.reverse();
    notes
}

fn granted(accessibility: bool, screen_recording: bool) -> Option<ScreenPermissions> {
    Some(ScreenPermissions {
        accessibility,
        screen_recording,
    })
}

/// Opens a task for the standard worker and passes until its window is up: one launch.
fn launch_once(tiers: &Tiers<'_>, body: &str) {
    tiers.open_body(body);
    tiers.context.pass().unwrap();
    tiers.context.pass().unwrap();
}

#[test]
fn a_claude_launch_where_computer_use_is_on_and_a_permission_is_missing_tells_the_human_once() {
    let context = Context::new();
    context
        .computer_use
        .on_in
        .borrow_mut()
        .insert(FOLDER.to_owned());
    context.computer_use.permissions.set(granted(true, false));
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    launch_once(&tiers, "Write the parser");
    launch_once(&tiers, "Write the lexer");
    let notes = computer_use_notes(&context, tiers.project.id);
    assert_eq!(notes.len(), 1, "once for the state, not for each launch");
    assert_eq!(
        notes[0].body,
        "Claude Code's computer use is on for this project, but ConsensFlow can't control \
         the screen until you allow it Screen & System Audio Recording in System Settings → \
         Privacy & Security."
    );
    assert_eq!(notes[0].task_number, None);
}

#[test]
fn each_missing_state_is_said_once_and_a_state_mended_and_lost_is_said_again() {
    let context = Context::new();
    context
        .computer_use
        .on_in
        .borrow_mut()
        .insert(FOLDER.to_owned());
    context.computer_use.permissions.set(granted(false, false));
    let tiers = Tiers::new(&context, &["zeus", "diana", "athena", "hermes", "apollo"]);
    launch_once(&tiers, "One");
    // Accessibility given: what is missing now is another state.
    context.computer_use.permissions.set(granted(true, false));
    launch_once(&tiers, "Two");
    // Both given: nothing is said, and what was said is forgotten.
    context.computer_use.permissions.set(granted(true, true));
    launch_once(&tiers, "Three");
    // Lost again: said again, and once.
    context.computer_use.permissions.set(granted(true, false));
    launch_once(&tiers, "Four");
    launch_once(&tiers, "Five");
    let bodies: Vec<String> = computer_use_notes(&context, tiers.project.id)
        .into_iter()
        .map(|note| note.body)
        .collect();
    let both = note_for(ScreenPermissions {
        accessibility: true,
        screen_recording: true,
    });
    let screen = note_for(ScreenPermissions {
        accessibility: false,
        screen_recording: true,
    });
    assert_eq!(bodies, [both, screen.clone(), screen]);
}

#[test]
fn a_missing_state_said_before_a_restart_is_not_said_again_and_another_is() {
    let context = Context::new();
    context
        .computer_use
        .on_in
        .borrow_mut()
        .insert(FOLDER.to_owned());
    context.computer_use.permissions.set(granted(true, false));
    let tiers = Tiers::new(&context, &["zeus", "diana", "athena"]);
    launch_once(&tiers, "Write the parser");
    // The app quits and starts again: a new engine on the same ledger.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(tiers.project.id).unwrap();
    tiers.open_body("Write the lexer");
    after.pass().unwrap();
    after.pass().unwrap();
    assert_eq!(
        computer_use_notes(&context, tiers.project.id).len(),
        1,
        "the state the last note said is not said at the next start"
    );
    // Another state is said, by the engine of the new start.
    context.computer_use.permissions.set(granted(false, false));
    tiers.open_body("Write the printer");
    after.pass().unwrap();
    after.pass().unwrap();
    let bodies: Vec<String> = computer_use_notes(&context, tiers.project.id)
        .into_iter()
        .map(|note| note.body)
        .collect();
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    assert!(
        bodies[1].contains("allow it Accessibility and Screen"),
        "{bodies:?}"
    );
}

#[test]
fn a_members_note_with_the_same_words_is_not_consensflows_and_does_not_stand_for_it() {
    let context = Context::new();
    // Computer use is off while the project opens: no launch is told yet.
    context.computer_use.permissions.set(granted(true, false));
    let tiers = Tiers::new(&context, &["zeus"]);
    assert!(computer_use_notes(&context, tiers.project.id).is_empty());
    context
        .computer_use
        .on_in
        .borrow_mut()
        .insert(FOLDER.to_owned());
    // The chief writes the human the very words of the note.
    let words = note_for(ScreenPermissions {
        accessibility: false,
        screen_recording: true,
    });
    context
        .ledger
        .borrow_mut()
        .note(
            tiers.project.id,
            &cf_ledger::NewNote {
                from: Some("chief".to_owned()),
                to: "human".to_owned(),
                task: None,
                body: words.clone(),
            },
        )
        .unwrap();
    launch_once(&tiers, "Write the parser");
    let own: Vec<MessageView> = computer_use_notes(&context, tiers.project.id)
        .into_iter()
        .filter(|note| note.sender.is_none())
        .collect();
    assert_eq!(own.len(), 1, "ConsensFlow says it itself");
    assert_eq!(own[0].body, words);
}

#[test]
fn nothing_is_said_where_computer_use_is_off_or_every_permission_is_given_or_none_runs() {
    for (on, permissions) in [
        (false, granted(false, false)),
        (true, granted(true, true)),
        // A system where Claude Code has no computer use (Windows).
        (true, None),
    ] {
        let context = Context::new();
        if on {
            context
                .computer_use
                .on_in
                .borrow_mut()
                .insert(FOLDER.to_owned());
        }
        context.computer_use.permissions.set(permissions);
        let tiers = Tiers::new(&context, &["zeus"]);
        launch_once(&tiers, "Write the parser");
        assert!(
            computer_use_notes(&context, tiers.project.id).is_empty(),
            "on {on}, {permissions:?}"
        );
    }
}

#[test]
fn the_note_names_what_is_missing_in_plain_words() {
    let said = |accessibility, screen_recording| {
        note_for(ScreenPermissions {
            accessibility,
            screen_recording,
        })
    };
    assert!(
        said(true, true).contains("allow it Accessibility and Screen & System Audio Recording in")
    );
    assert!(said(true, false).contains("allow it Accessibility in"));
    assert!(said(false, true).contains("allow it Screen & System Audio Recording in"));
    for body in [said(true, true), said(true, false), said(false, true)] {
        assert!(body.starts_with(NOTE_OPENING), "{body}");
    }
}
