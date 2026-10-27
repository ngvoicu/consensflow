//! The shapes a harness's record keeps a paste in, each of which proves the paste
//! arrived: as it was given, inside the wrapper Claude Code 2.1.296 puts a paste
//! in, behind the placeholders of the images Claude Code 2.1.308 attached to a paste
//! that has their paths, and, for a member's fresh window, behind the brief it
//! received, which its launch gives first. Found on 2026-10-16 by the reviews of the
//! stricter proof of that day, which took every paste a real Claude records for
//! lacking: a Claude chief's result was pasted again; and, for the images, on
//! 2026-10-21 in the field (T-215: an image designer's result pasted three times,
//! then failed). Held to no Node trace: Node's dispatcher took any item that held
//! the header.

use cf_engine::delivery_text::header_id;
use cf_engine::testing::Context;
use cf_harness::records::Role;

use crate::fixtures::launches_of;

/// How many messages the chief's window was given that say `words`.
fn given_saying(context: &Context, words: &str) -> usize {
    context
        .adapter
        .agent("chief")
        .items
        .iter()
        .filter(|item| item.role == Role::User && item.text.contains(words))
        .count()
}

#[test]
fn confirms_a_paste_claude_code_recorded_in_its_wrapper_and_pastes_it_once() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.wraps_pastes.set(true);
    let note = context.note(project.id, "zeus", "chief", "Taken");
    context.pass().unwrap();
    let recorded = context
        .adapter
        .agent("chief")
        .items
        .last()
        .unwrap()
        .text
        .clone();
    assert!(
        recorded.starts_with("\n\n<pasted_content id="),
        "{recorded}"
    );
    context.pass().unwrap();
    // The chief answers, and its window is at rest past the arrival window.
    context.adapter.answer("chief", "Noted");
    context.advance(60_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        given_saying(&context, "Taken"),
        1,
        "the chief was given it once"
    );
    assert_eq!(context.message(note.id).state, "delivered");
}

/// A result of some 2,600 characters, as a worker's report is: Claude Code is given it in
/// pieces, two of a thousand characters, and what is left of it, short of a piece.
fn a_long_result() -> String {
    "A line of the result, as long as the lines of a worker's report.\n".repeat(40)
}

#[test]
fn confirms_a_paste_claude_code_recorded_in_pieces_with_the_last_left_bare_at_the_first_look() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.splits_pastes.set(true);
    let result = context.note(project.id, "zeus", "chief", &a_long_result());
    context.pass().unwrap();
    // As the field's record held it (T-763: m-4272, m-4273, m-4367, m-4382, m-4518): the pieces
    // in the wrapper, and the last of them bare after the last closing tag.
    let recorded = context
        .adapter
        .agent("chief")
        .items
        .last()
        .unwrap()
        .text
        .clone();
    let after_last_close = recorded
        .rsplit_once("</pasted_content id=\"b118\">")
        .unwrap()
        .1;
    assert!(
        recorded.starts_with("\n\n<pasted_content id="),
        "the pieces are in the wrapper"
    );
    assert!(
        after_last_close.starts_with("\n\n") && !after_last_close.trim().is_empty(),
        "a tail is loose after the last closing tag"
    );
    // The first look after it confirms: one attempt, nothing retried.
    context.pass().unwrap();
    let confirmed = context.message(result.id);
    assert_eq!(
        (confirmed.state.as_str(), confirmed.attempts),
        ("delivered", 1)
    );
    // The chief answers, and its window is at rest past the arrival window: nothing is pasted again.
    context.adapter.answer("chief", "Noted");
    context.advance(60_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        given_saying(&context, "A line of the result"),
        1,
        "the chief was given it once"
    );
    let events = context.ledger.borrow().events(project.id, 0, 500).unwrap();
    assert!(
        events.iter().all(|event| event.kind != "delivery.retried"),
        "{events:?}"
    );
}

#[test]
fn confirms_a_paste_claude_code_recorded_in_pieces_behind_the_placeholders_of_its_images_at_the_first_look(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.attaches_images.set(true);
    context.adapter.splits_pastes.set(true);
    let words = format!("{DESIGNER}\n{}", a_long_result());
    let result = context.note(project.id, "zeus", "chief", &words);
    context.pass().unwrap();
    context.pass().unwrap();
    let confirmed = context.message(result.id);
    assert_eq!(
        (confirmed.state.as_str(), confirmed.attempts),
        ("delivered", 1)
    );
}

/// A result as the designer's role asks it: the absolute path of each picture on a line of
/// its own, then a line on what they show.
const DESIGNER: &str = "/work/images/desk.png\n/work/images/hero.png\nWarm editorial desk scene.";

#[test]
fn confirms_a_paste_claude_code_recorded_behind_the_placeholders_of_its_images_and_pastes_it_once()
{
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.attaches_images.set(true);
    let result = context.note(project.id, "zeus", "chief", DESIGNER);
    context.pass().unwrap();
    // As the field's record held it: the header behind the placeholders, no path in the
    // words, and the sources as an item of their own after it.
    let items: Vec<String> = context
        .adapter
        .agent("chief")
        .items
        .iter()
        .map(|item| item.text.to_string())
        .collect();
    let at = items.len() - 2;
    assert!(
        items[at].starts_with(&format!(
            "[Image #1] [Image #2][ConsensFlow m-{} ",
            result.id
        )),
        "{items:?}"
    );
    assert!(!items[at].contains("desk.png"), "{items:?}");
    assert_eq!(
        items[at + 1],
        "[Image: source: /work/images/desk.png]\n[Image: source: /work/images/hero.png]"
    );
    // The sources are the harness's own words, as the reader makes of Claude Code's
    // companion record, and no message of the user's.
    let roles: Vec<Role> = context
        .adapter
        .agent("chief")
        .items
        .iter()
        .map(|item| item.role)
        .collect();
    assert_eq!((roles[at], roles[at + 1]), (Role::User, Role::Custom));
    context.pass().unwrap();
    // The first look after it confirms: one attempt, nothing retried (the field: pasted three
    // times and failed, T-215).
    let confirmed = context.message(result.id);
    assert_eq!(
        (confirmed.state.as_str(), confirmed.attempts),
        ("delivered", 1)
    );
    // The chief answers, and its window is at rest past the arrival window.
    context.adapter.answer("chief", "Noted");
    context.advance(60_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        given_saying(&context, "Warm editorial"),
        1,
        "the chief was given it once"
    );
    assert_eq!(context.message(result.id).state, "delivered");
}

#[test]
fn a_start_confirms_a_paste_the_record_shows_behind_the_placeholders_of_its_images_and_never_pastes_it_again(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.attaches_images.set(true);
    let result = context.note(project.id, "zeus", "chief", DESIGNER);
    context.pass().unwrap();
    assert_eq!(context.message(result.id).state, "delivering");

    // The daemon stops before a look confirms it, and starts again.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    assert_eq!(
        context.message(result.id).state,
        "delivered",
        "the record shows it behind the placeholders"
    );
    assert_eq!(given_saying(&context, "Warm editorial"), 1);
}

#[test]
fn confirms_the_first_message_a_fresh_window_was_given_behind_the_brief_it_received() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let zeus = context.id(project.id, "zeus");
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    // The quit cut zeus's turn, and its harness kept nothing of the conversation: its
    // fresh window is given the brief it received, then the note that the quit cut it.
    context.adapter.holds_nothing("zeus");
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    after.resume_project(project.id).unwrap();
    let first = context
        .ledger
        .borrow()
        .in_flight()
        .unwrap()
        .into_iter()
        .find(|message| message.recipient_id == zeus)
        .unwrap();
    let launch = launches_of(&context, "zeus").pop().unwrap();
    let given = launch["message"].as_str().unwrap();
    assert_ne!(header_id(given), Some(first.id), "the brief first: {given}");

    after.pass().unwrap();
    assert_eq!(
        context.message(first.id).state,
        "delivered",
        "its paste is the one behind the brief"
    );
}

#[test]
fn a_start_confirms_the_first_message_a_fresh_window_was_given_behind_its_brief_and_never_pastes_it_again(
) {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let zeus = context.id(project.id, "zeus");
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    // Zeus's window goes, and its harness keeps nothing of the conversation; the chief
    // resumes T-1, and a fresh window is given the brief, then the words that resume it.
    context.adapter.holds_nothing("zeus");
    context.exit("zeus");
    assert_eq!(context.task(project.id, 1).task.state, "paused");
    context
        .ledger
        .borrow_mut()
        .resume_task(project.id, 1, Some("chief"), "Go on")
        .unwrap();
    context.pass().unwrap();
    let first = context
        .ledger
        .borrow()
        .in_flight()
        .unwrap()
        .into_iter()
        .find(|message| message.recipient_id == zeus)
        .unwrap();
    let launch = launches_of(&context, "zeus").pop().unwrap();
    let given = launch["message"].as_str().unwrap();
    assert_ne!(header_id(given), Some(first.id), "the brief first: {given}");

    // The daemon stops before a look confirms it, and starts again.
    context.ledger.borrow_mut().suspend_for_restart().unwrap();
    let after = context.make();
    after.settle_after_restart().unwrap();
    assert_eq!(
        context.message(first.id).state,
        "delivered",
        "the record shows it behind the brief"
    );
}

/// Words that resume a task and name a picture by its path, on a line of their own.
const WITH_A_PICTURE: &str = "Carry on\n/work/images/desk.png";

#[test]
fn a_worker_resumed_with_words_that_name_a_picture_ends_its_turn_with_its_answer_as_the_result_the_sources_the_harness_wrote_beside_it_being_no_part_of_it(
) {
    // Turn-end (3.0.6-alpha.2): the result is the final answer to the task's latest
    // message once the turn ends. Here that message is behind Claude Code's placeholders,
    // and the harness wrote its own item after it (the sources of the picture) and
    // another in the middle of the turn and after the answer (a tool's `turnCompanion`):
    // none of them is a message to the worker, so none cuts the answer off, and none is
    // in it.
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    let chief = context.id(project.id, "chief");
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(project.id, 1).task.state, "working");
    context.adapter.attaches_images.set(true);
    context.adapter.busy_until_interrupted("zeus");
    context.pause_task(project.id, 1);
    context.pass().unwrap();
    context.pass().unwrap();
    context.resume_task(project.id, 1, WITH_A_PICTURE);
    for _ in 0..4 {
        context.pass().unwrap();
    }
    assert_eq!(context.task(project.id, 1).task.state, "working");
    let items = context.adapter.agent("zeus").items;
    let roles: Vec<Role> = items.iter().map(|item| item.role).collect();
    let at = items.len() - 2;
    assert!(
        items[at].text.starts_with("[Image #1][ConsensFlow m-"),
        "{items:?}"
    );
    assert_eq!((roles[at], roles[at + 1]), (Role::User, Role::Custom));

    // The turn the words began: a step toward the answer that stopped for a tool, the
    // tool's output, the harness's item for the picture the tool made, and the answer.
    let companion = |source: &str| {
        context
            .adapter
            .item(Role::Custom, &format!("[Image: source: {source}]"))
    };
    let mut step = context.adapter.item(Role::Assistant, "Reading the grammar");
    step.complete = false;
    let steps = [
        step,
        context.adapter.item(Role::Tool, "output of a tool"),
        companion("/work/images/made.png"),
    ];
    context
        .adapter
        .with("zeus", |agent| agent.items.extend(steps));
    context.adapter.answer("zeus", "Parser, finished");
    let after = companion("/work/images/last.png");
    context
        .adapter
        .with("zeus", |agent| agent.items.push(after));
    for _ in 0..3 {
        context.pass().unwrap();
    }

    assert_eq!(context.task(project.id, 1).task.state, "done");
    let results: Vec<String> = context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect();
    assert_eq!(results, ["Parser, finished"]);
}
