//! A chief's launch that is to say the role first: its window is the human's, and
//! is never closed for being slow; where it exits before the role was told, or
//! cannot be told it, the message the role went ahead of stays in its queue and
//! the chief is tried again at the backoff; and what goes into its window after
//! the role is never given up for being slow. A member's is in `launch`.

use cf_ledger::MessageView;

use super::launch::{kills_of, never_ready, refusing_pastes, told_the_human};
use super::{
    chief_launches, codex, project_of, recorded, replacements, said_to_the_chief, specs,
    through_a_turn, REPLACES,
};

#[test]
fn a_chiefs_window_that_is_never_ready_for_the_role_is_never_closed_for_being_slow_and_takes_it_when_it_is_ready(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    let waiting = context.note(project.id, "human", "chief", "Any news?");
    never_ready(&context);
    context.resume_project(project.id).unwrap();

    // Hours of it: the chief's window is the human's.
    for _ in 0..6 {
        context.advance(10 * 60_000);
        context.pass().unwrap();
    }
    assert_eq!(
        kills_of(&context, "chief"),
        0,
        "it is never closed for being slow"
    );
    assert_eq!(chief_launches(&context.codex).len(), 2, "no other launch");
    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 1);
    assert_eq!(context.message(notes[0].id).state, "queued");
    assert_eq!(
        context.message(waiting.id).state,
        "queued",
        "what the role went ahead of waits for it, and is not failed"
    );

    // The window is ready at last: the role, and then the words that waited.
    context.codex.ready.replace(None);
    through_a_turn(&context);
    assert_eq!(context.message(notes[0].id).state, "delivered");
    through_a_turn(&context);
    assert_eq!(context.message(waiting.id).state, "delivered");
    let texts = said_to_the_chief(&context);
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(texts[0].contains(REPLACES), "{}", texts[0]);
    assert!(texts[1].contains("Any news?"), "{}", texts[1]);
    context.close();
}

#[test]
fn a_chiefs_window_that_exits_before_the_role_was_told_keeps_what_it_displaced_queued_and_the_chief_is_tried_again_at_the_backoff(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    let waiting = context.note(project.id, "human", "chief", "Any news?");
    context.resume_project(project.id).unwrap();
    assert_eq!(chief_launches(&context.codex).len(), 2);

    context.exit("chief");
    assert_eq!(
        context.message(waiting.id).state,
        "queued",
        "what the role went ahead of is not failed: it waits for the next window"
    );
    assert_eq!(context.message(waiting.id).attempts, 0);
    assert_eq!(
        context.project(project.id).state,
        "open",
        "a window that went before it came up closes no project"
    );
    assert_eq!(
        told_the_human(&context, project.id, "The chief could not start"),
        1
    );

    // It is tried again at the backoff, not before: the role is said again, and
    // then what waited.
    context.pass().unwrap();
    assert_eq!(chief_launches(&context.codex).len(), 2, "not before");
    context.advance(5_000);
    context.pass().unwrap();
    assert_eq!(chief_launches(&context.codex).len(), 3, "at the backoff");
    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 2);
    assert_eq!(
        context.message(notes[0].id).state,
        "cancelled",
        "the first was written for a window that never took it"
    );
    through_a_turn(&context);
    assert_eq!(context.message(notes[1].id).state, "delivered");
    through_a_turn(&context);
    assert_eq!(context.message(waiting.id).state, "delivered");
    context.close();
}

#[test]
fn a_chiefs_window_that_the_human_closes_with_the_project_before_the_role_was_told_is_not_tried_again(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    let waiting = context.note(project.id, "human", "chief", "Any news?");
    context.resume_project(project.id).unwrap();

    // The human closes the project before the window was told anything.
    context.close_project(project.id).unwrap();
    assert_eq!(context.project(project.id).state, "suspended");
    assert_eq!(context.message(waiting.id).state, "queued");
    assert_eq!(
        told_the_human(&context, project.id, "The chief could not start"),
        0,
        "it did not fail to come up: the human closed it"
    );
    for _ in 0..3 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(chief_launches(&context.codex).len(), 2, "no launch follows");
    assert_eq!(replacements(&context, chief).len(), 1);
    context.close();
}

#[test]
fn a_chiefs_window_that_went_by_itself_after_its_project_was_closed_is_no_launch_that_failed() {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    let waiting = context.note(project.id, "human", "chief", "Any news?");
    context.resume_project(project.id).unwrap();

    // The human closes the project, and the host will not close the chief's window.
    context.host.refuse_kills.set(true);
    context
        .close_project(project.id)
        .expect_err("the window was kept");
    assert_eq!(context.project(project.id).state, "suspended");

    // It goes by itself later, before it was told its role.
    context.host.refuse_kills.set(false);
    context.exit("chief");
    assert_eq!(
        told_the_human(&context, project.id, "The chief could not start"),
        0,
        "a project that is closed has no chief that failed to come up"
    );
    assert_eq!(context.message(waiting.id).state, "queued");
    for _ in 0..3 {
        context.advance(60_000);
        context.pass().unwrap();
    }
    assert_eq!(chief_launches(&context.codex).len(), 2, "no launch follows");
    assert_eq!(replacements(&context, chief).len(), 1);
    context.close();
}

#[test]
fn a_chiefs_replacement_the_harness_refuses_as_often_as_a_message_is_given_attempts_closes_the_window_keeps_what_it_displaced_queued_and_the_chief_is_tried_again_at_the_backoff(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    let waiting = context.note(project.id, "human", "chief", "Any news?");
    refusing_pastes(&context);
    context.resume_project(project.id).unwrap();
    for _ in 0..8 {
        context.pass().unwrap();
    }

    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 1);
    assert_eq!(context.message(notes[0].id).state, "failed");
    assert_eq!(context.message(waiting.id).state, "queued");
    assert_eq!(
        context.message(waiting.id).attempts,
        0,
        "what the role went ahead of was not given, and not given up"
    );
    assert_eq!(kills_of(&context, "chief"), 1, "the window is closed");
    assert_eq!(
        context.project(project.id).state,
        "open",
        "and the project is not closed with it"
    );
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief"),
        "the conversation was told nothing"
    );
    assert_eq!(
        told_the_human(&context, project.id, "The chief could not start"),
        1
    );
    assert!(said_to_the_chief(&context).is_empty());

    // It is tried again at the backoff, not before, and the role is said anew.
    context.codex.after_prepare.replace(None);
    context.pass().unwrap();
    assert_eq!(chief_launches(&context.codex).len(), 2, "not before");
    context.advance(5_000);
    context.pass().unwrap();
    assert_eq!(chief_launches(&context.codex).len(), 3, "at the backoff");
    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 2);
    through_a_turn(&context);
    assert_eq!(context.message(notes[1].id).state, "delivered");
    through_a_turn(&context);
    assert_eq!(context.message(waiting.id).state, "delivered");
    assert_eq!(
        told_the_human(&context, project.id, "The chief could not start"),
        1,
        "and the human is not told again"
    );

    context.close();
}

#[test]
fn a_chiefs_window_the_host_would_not_close_once_its_replacement_was_given_up_is_given_nothing_under_the_role_it_has(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    let waiting = context.note(project.id, "human", "chief", "Any news?");
    refusing_pastes(&context);
    context.host.refuse_kills.set(true);
    context.resume_project(project.id).unwrap();
    for _ in 0..10 {
        context.pass().unwrap();
    }

    // The note that says the role was refused as often as a message is given
    // attempts, and the window stays: it is not given what waited for it.
    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 1);
    assert_eq!(context.message(notes[0].id).state, "failed");
    assert_eq!(context.message(waiting.id).state, "queued");
    assert_eq!(context.message(waiting.id).attempts, 0);
    assert!(said_to_the_chief(&context).is_empty());
    assert_eq!(chief_launches(&context.codex).len(), 2);
    assert_eq!(
        told_the_human(&context, project.id, "The chief could not start"),
        0,
        "the launch is not settled while its window is open"
    );
    let asked = kills_of(&context, "chief");
    assert!(asked > 1, "the host is asked again at every look: {asked}");

    // The host takes the kill at last: the launch is settled as one that did not
    // come up, and the chief is tried again.
    context.host.refuse_kills.set(false);
    context.codex.after_prepare.replace(None);
    context.pass().unwrap();
    assert_eq!(kills_of(&context, "chief"), asked + 1);
    assert_eq!(
        told_the_human(&context, project.id, "The chief could not start"),
        1
    );
    context.advance(5_000);
    context.pass().unwrap();
    assert_eq!(chief_launches(&context.codex).len(), 3);
    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 2);
    through_a_turn(&context);
    assert_eq!(context.message(notes[1].id).state, "delivered");
    through_a_turn(&context);
    assert_eq!(context.message(waiting.id).state, "delivered");
    context.close();
}

#[test]
fn a_chief_that_is_tried_again_to_say_the_role_is_tried_ever_more_slowly_and_the_human_is_told_once(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    context.note(project.id, "human", "chief", "Any news?");
    refusing_pastes(&context);
    context.resume_project(project.id).unwrap();
    for _ in 0..8 {
        context.pass().unwrap();
    }
    assert_eq!(told_the_human(&context, project.id, "could not start"), 1);

    // The second window fails to take the role too: the wait is longer, and the
    // human is not told again.
    context.advance(5_000);
    context.pass().unwrap();
    for _ in 0..8 {
        context.pass().unwrap();
    }
    assert_eq!(chief_launches(&context.codex).len(), 3);
    assert_eq!(
        told_the_human(&context, project.id, "could not start"),
        1,
        "told once, though the launch came up twice"
    );
    context.advance(5_000);
    context.pass().unwrap();
    assert_eq!(
        chief_launches(&context.codex).len(),
        3,
        "ten seconds is the wait now, and five have passed"
    );
    context.advance(5_000);
    context.pass().unwrap();
    assert_eq!(chief_launches(&context.codex).len(), 4);
    assert_eq!(replacements(&context, chief).len(), 3);
    assert_eq!(
        context.project(project.id).state,
        "open",
        "nothing here closed the project"
    );
    context.close();
}

#[test]
fn a_chiefs_message_that_goes_in_after_the_role_is_never_given_up_for_being_slow_and_goes_back_to_the_queue_with_its_window(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    // The human's words begin a turn that is not over when the project is closed:
    // the chief is told its turn was cut, which is its first message when it is
    // resumed, after the role.
    context.note(project.id, "human", "chief", "Plan the work.");
    context.pass().unwrap();
    context.pass().unwrap();
    context.close_project(project.id).unwrap();
    specs(&context, project.id, true);
    context.resume_project(project.id).unwrap();
    let cut: Vec<MessageView> = context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.body.contains("stopped in the middle of your turn"))
        .collect();
    assert_eq!(cut.len(), 1);
    through_a_turn(&context);
    assert_eq!(
        context.message(replacements(&context, chief)[0].id).state,
        "delivered"
    );

    // The note goes in after the role, and the window's record never shows it.
    context.codex.with("chief", |agent| agent.arrive = false);
    context.pass().unwrap();
    assert_eq!(context.message(cut[0].id).state, "delivering");
    for _ in 0..5 {
        context.advance(31_000);
        context.pass().unwrap();
    }
    assert_eq!(
        context.message(cut[0].id).state,
        "delivering",
        "waited for, as the chief's first message is, and not given up"
    );
    assert_eq!(
        context.message(cut[0].id).attempts,
        1,
        "and not given again"
    );
    assert_eq!(kills_of(&context, "chief"), 0);

    // The window goes: the note is given back to the queue with its attempt.
    context.exit("chief");
    assert_eq!(context.message(cut[0].id).state, "queued");
    assert_eq!(context.message(cut[0].id).attempts, 0);
    context.resume_project(project.id).unwrap();
    through_a_turn(&context);
    assert_eq!(context.message(cut[0].id).state, "delivered");
    context.close();
}
