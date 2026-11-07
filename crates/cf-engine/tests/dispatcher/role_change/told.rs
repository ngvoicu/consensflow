//! What the ledger records of what a conversation was told of its role, where
//! the note that told it did not go as plainly as a note pasted and seen to
//! arrive: one the window took before it closed, which the next launch finds in
//! the conversation's own record, and one too long to go whole, which tells the
//! conversation its role in part, and tells it whole once the window has read it
//! in its inbox (`cf inbox read`).

use std::rc::Rc;

use cf_base::text::utf16_len;
use cf_engine::testing::Context;
use cf_ledger::{LedgerError, MessageView, Read};
use serde_json::Value;

use super::{
    chief_launches, codex, project_of, recorded, relaunch, replacements, said_to_the_chief, specs,
    REPLACES, WITH_SPECS,
};

/// The notes ConsensFlow wrote to the human, which say a role told in part was not
/// read by the next Resume.
fn not_read(context: &Context, project: i64) -> Vec<MessageView> {
    let human = context.id(project, "human");
    context
        .inbox(human)
        .into_iter()
        .filter(|message| {
            message.sender.is_none() && message.body.contains("did not read its new role")
        })
        .collect()
}

/// The chief's window reads the note `id` whole: `cf inbox read m-N`, which said
/// so to the board. How many the ledger received.
fn read_in_the_inbox(context: &Context, project: i64, id: i64) -> usize {
    let chief = context.id(project, "chief");
    context
        .ledger
        .borrow_mut()
        .receive_read(chief, &[id], Read::Inbox)
        .unwrap()
}

/// What a release that rewrote the chief's text by 17,000 characters makes of a
/// Resume: a role too long for one message, told in part, its note delivered.
fn a_long_role_told_in_part(context: &Context, project: i64) -> MessageView {
    let chief = context.id(project, "chief");
    context
        .roles
        .rewritten
        .replace(format!(" {}", "x".repeat(17_000)));
    relaunch(context, project);
    context.pass().unwrap();
    context.pass().unwrap();
    let notes = replacements(context, chief);
    assert_eq!(notes.len(), 1);
    assert_eq!(context.message(notes[0].id).state, "delivered");
    notes[0].clone()
}

/// The human closes the project as the chief's harness takes what it was given, which
/// its record shows only once the window is gone: the window's end, which reads the
/// record, finds nothing of it, and what the next launch reads, with no window open,
/// holds it.
fn closes_before_its_record_shows_it(context: &Context, project: i64) {
    let written = context
        .codex
        .with("chief", |agent| std::mem::take(&mut agent.items));
    context.close_project(project).unwrap();
    context.codex.with("chief", |agent| agent.items = written);
}

#[test]
fn a_replacement_the_window_took_before_it_closed_is_what_the_conversation_was_told_whatever_the_ledger_says_and_the_role_the_human_went_back_to_is_said_again(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    // The human turns specs on, and the window is resumed: the role goes in by
    // paste, and the window's own record shows it.
    specs(&context, project.id, true);
    relaunch(&context, project.id);
    context.pass().unwrap();
    let paste = replacements(&context, chief).remove(0);
    assert_eq!(context.message(paste.id).state, "delivering");
    assert_eq!(said_to_the_chief(&context).len(), 1);

    // The window closes before the daemon saw it arrive, and its record shows the note
    // only once the window is gone (its harness wrote it late): the note waits for the
    // next.
    closes_before_its_record_shows_it(&context, project.id);
    assert_eq!(context.message(paste.id).state, "queued");
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief"),
        "as far as the ledger knows, the conversation was not told"
    );

    // The human takes the specs off again and resumes: the role is the one the
    // conversation began with, which is why the note that waits would be withdrawn.
    specs(&context, project.id, false);
    context.resume_project(project.id).unwrap();
    assert_eq!(
        context.message(paste.id).state,
        "delivered",
        "it is in the conversation's record: the conversation was told it"
    );
    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 2, "and the role it is now is said again");
    assert!(
        notes[1].body.ends_with("\n\ninstructions for chief"),
        "{}",
        notes[1].body
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(notes[1].id).state, "delivered");
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief")
    );
    let texts = said_to_the_chief(&context);
    assert_eq!(texts.len(), 1, "{texts:?}");
    assert!(texts[0].contains(REPLACES));
    let launches = chief_launches(&context.codex);
    assert_eq!(launches.len(), 3);
    assert_eq!(launches[2]["message"], Value::Null);
    context.close();
}

#[test]
fn a_replacement_the_record_shows_is_not_made_the_first_message_of_the_launch_that_finds_it_the_head_of_the_queue(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    specs(&context, project.id, true);
    relaunch(&context, project.id);
    context.pass().unwrap();
    let paste = replacements(&context, chief).remove(0);
    // The window closes before the daemon saw the note arrive, or its record showed
    // it, and the project is open again with no window for its chief (a Resume that a
    // step has overtaken): the head of the chief's queue is the note.
    closes_before_its_record_shows_it(&context, project.id);
    assert_eq!(context.message(paste.id).state, "queued");
    context
        .ledger
        .borrow_mut()
        .set_project_state(project.id, "open")
        .unwrap();
    context.pass().unwrap();

    assert_eq!(
        context.message(paste.id).state,
        "delivered",
        "the record shows it: it was told, and is not told again as the launch's first message"
    );
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some(WITH_SPECS),
        "and what it said is the role it was told"
    );
    let launches = chief_launches(&context.codex);
    assert_eq!(launches.len(), 3, "the window opened");
    assert_eq!(launches[2]["message"], Value::Null);
    assert_eq!(
        replacements(&context, chief).len(),
        1,
        "no other was written"
    );
    context.close();
}

#[test]
fn a_replacement_is_confirmed_and_recorded_by_the_one_write_that_does_both_and_no_call_of_its_own_records_a_role(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    specs(&context, project.id, true);
    // The ledger refuses a call that records a conversation's role by itself, as a
    // ledger that failed between the two writes of a confirmation and a record would.
    context
        .ledger
        .borrow_mut()
        .watch(Box::new(|call, _| match call {
            "set_conversation_role_text" => Err(LedgerError::refused(
                "ledger-failed",
                "the ledger could not record the role",
            )),
            _ => Ok(()),
        }));
    relaunch(&context, project.id);
    context.pass().unwrap();
    context.pass().unwrap();

    let note = replacements(&context, chief).remove(0);
    assert_eq!(context.message(note.id).state, "delivered");
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some(WITH_SPECS),
        "confirmed and recorded together: no call of its own was needed"
    );
    assert_eq!(
        context.log.failures.borrow().len(),
        0,
        "nothing failed unseen"
    );
    context.close();
}

#[test]
fn a_replacement_the_window_never_took_is_still_withdrawn_when_the_record_does_not_show_it() {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    // The record holds the header of another message, which proves nothing of the note.
    context.note(project.id, "human", "chief", "Plan the work.");
    context.pass().unwrap();
    context.pass().unwrap();
    context.codex.answer("chief", "On it.");
    context.pass().unwrap();
    // A conversation resumed keeps what it held.
    let held = context.codex.agent("chief").items;
    let codex = Rc::downgrade(&context.codex);
    *context.codex.after_prepare.borrow_mut() = Some(Rc::new(move |handle| {
        if let Some(codex) = codex.upgrade() {
            let held = held.clone();
            codex.with(handle, |agent| {
                let after = std::mem::replace(&mut agent.items, held);
                agent.items.extend(after);
            });
        }
    }));
    specs(&context, project.id, true);
    relaunch(&context, project.id);
    context.pass().unwrap();
    let paste = replacements(&context, chief).remove(0);
    // What the harness's record holds does not show it, though it was handed over.
    context.codex.with("chief", |agent| {
        agent.items.retain(|item| !item.text.contains(REPLACES));
    });
    assert!(
        context
            .codex
            .agent("chief")
            .items
            .iter()
            .any(|item| item.text.contains("[ConsensFlow m-")),
        "the record still has a header of another message"
    );
    context.close_project(project.id).unwrap();
    assert_eq!(context.message(paste.id).state, "queued");

    specs(&context, project.id, false);
    context.resume_project(project.id).unwrap();
    assert_eq!(context.message(paste.id).state, "cancelled");
    assert_eq!(
        replacements(&context, chief).len(),
        1,
        "the role is what the conversation was told: nothing is said"
    );
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief")
    );
    context.close();
}

#[test]
fn a_role_too_long_for_one_message_goes_cut_with_the_word_to_read_the_rest_now_and_is_told_only_once_its_note_is_read(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    let note = a_long_role_told_in_part(&context, project.id);
    let said = said_to_the_chief(&context);
    assert_eq!(said.len(), 1);
    assert!(
        said[0]
            .contains("Run that command now, before anything else, and read everything it prints."),
        "its opening says to read the rest now: {}",
        &said[0][..600]
    );
    assert!(
        said[0].contains(&format!("read all of it with: cf inbox read m-{}", note.id)),
        "the way to read the rest is said: {}",
        &said[0][said[0].len() - 200..]
    );
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief"),
        "a role told in part is not recorded as told until it is read"
    );
    assert!(
        not_read(&context, project.id).is_empty(),
        "nothing is said to the human while the window may still read it"
    );

    // The window reads it whole in its inbox: the role is told.
    assert_eq!(read_in_the_inbox(&context, project.id, note.id), 1);
    assert_eq!(context.message(note.id).state, "read");
    assert!(
        recorded(&context, chief).is_some_and(|told| told.ends_with('x')),
        "the role it read is what it was told"
    );
    assert_eq!(
        read_in_the_inbox(&context, project.id, note.id),
        0,
        "read once"
    );
    context.close();
}

#[test]
fn a_role_told_in_part_and_read_is_said_nothing_of_at_the_next_resume_and_the_human_is_told_nothing(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    let note = a_long_role_told_in_part(&context, project.id);
    read_in_the_inbox(&context, project.id, note.id);
    context.codex.answer("chief", "Understood.");
    context.pass().unwrap();

    relaunch(&context, project.id);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        replacements(&context, chief).len(),
        1,
        "the role is what it read: nothing is said again"
    );
    assert!(not_read(&context, project.id).is_empty());
    context.close();
}

#[test]
fn a_role_told_in_part_and_not_read_by_the_next_resume_is_said_again_and_the_human_is_told_so() {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    let note = a_long_role_told_in_part(&context, project.id);
    context.codex.answer("chief", "Understood.");
    context.pass().unwrap();

    relaunch(&context, project.id);
    context.pass().unwrap();
    context.pass().unwrap();
    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 2, "it is said again");
    let board = not_read(&context, project.id);
    assert_eq!(board.len(), 1, "the human is told");
    assert_eq!(
        board[0].body,
        format!(
            "@chief did not read its new role before this Resume: it is too long for one message, and m-{} went in cut at about 16,000 characters with `cf inbox read m-{}` for the rest. It is told again now, in m-{}.",
            note.id, note.id, notes[1].id
        )
    );

    // Read this time: the Resume after says nothing, and the human is told nothing more.
    assert_eq!(context.message(notes[1].id).state, "delivered");
    read_in_the_inbox(&context, project.id, notes[1].id);
    context.codex.answer("chief", "Understood.");
    context.pass().unwrap();
    relaunch(&context, project.id);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(replacements(&context, chief).len(), 2);
    assert_eq!(not_read(&context, project.id).len(), 1);
    context.close();
}

#[test]
fn a_role_told_in_part_whose_text_changed_again_before_it_was_read_is_said_anew_with_no_line_to_the_human(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    a_long_role_told_in_part(&context, project.id);
    context.codex.answer("chief", "Understood.");
    context.pass().unwrap();

    // The human turns specs on: another role, which the window is told; the one it
    // did not read is no longer its role, and nothing is said of it.
    specs(&context, project.id, true);
    relaunch(&context, project.id);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(replacements(&context, chief).len(), 2);
    assert!(not_read(&context, project.id).is_empty());
    context.close();
}

/// A release rewrote the chief's text by 17,000 characters, the window is resumed, and the
/// note that tells the role in part is on its way: pasted, not yet seen by the daemon.
fn a_long_role_on_its_way(context: &Context, project: i64) -> MessageView {
    let chief = context.id(project, "chief");
    context
        .roles
        .rewritten
        .replace(format!(" {}", "x".repeat(17_000)));
    relaunch(context, project);
    context.pass().unwrap();
    let note = replacements(context, chief).remove(0);
    assert_eq!(context.message(note.id).state, "delivering");
    note
}

#[test]
fn a_role_told_in_part_read_before_its_delivery_was_confirmed_is_kept_and_told_once_it_is_confirmed(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    let note = a_long_role_on_its_way(&context, project.id);

    // The window runs `cf inbox read` the moment it has the note, as the note tells it to,
    // before the daemon has seen the paste arrive.
    assert_eq!(
        read_in_the_inbox(&context, project.id, note.id),
        0,
        "kept, and not received yet: the paste is still the window's to settle"
    );
    assert_eq!(context.message(note.id).state, "delivering");
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief"),
        "nothing is told before the paste is proved"
    );

    // The window's record shows the note: the delivery is confirmed, and the read with it.
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.message(note.id).state,
        "read",
        "it was read: the human is not to be told it was not"
    );
    assert!(
        recorded(&context, chief).is_some_and(|told| told.ends_with('x')),
        "the role it read is what it was told"
    );

    // The Resume after says nothing of it, and the human is told nothing.
    context.codex.answer("chief", "Understood.");
    context.pass().unwrap();
    relaunch(&context, project.id);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        replacements(&context, chief).len(),
        1,
        "the role is what it read: nothing is said again"
    );
    assert!(not_read(&context, project.id).is_empty());
    context.close();
}

#[test]
fn a_role_read_before_its_window_closed_is_told_when_the_next_launch_finds_the_note_in_the_record()
{
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    let note = a_long_role_on_its_way(&context, project.id);
    assert_eq!(read_in_the_inbox(&context, project.id, note.id), 0);

    // The window closes before the daemon saw the note arrive, and its record shows the
    // note only once the window is gone: the paste is given back to the queue.
    closes_before_its_record_shows_it(&context, project.id);
    assert_eq!(context.message(note.id).state, "queued");
    assert!(
        recorded(&context, chief).is_some_and(|told| !told.ends_with('x')),
        "as far as the ledger knows, the conversation was not told"
    );

    // The next launch finds the note in the conversation's own record: it went in, and the
    // window read it whole before it closed.
    context.resume_project(project.id).unwrap();
    assert_eq!(
        context.message(note.id).state,
        "read",
        "the read was kept with the note, which a read does not receive while it waits"
    );
    assert!(
        recorded(&context, chief).is_some_and(|told| told.ends_with('x')),
        "the role it read is what it was told"
    );
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        replacements(&context, chief).len(),
        1,
        "the role is what it read: nothing is said again"
    );
    assert!(not_read(&context, project.id).is_empty());
    context.close();
}

#[test]
fn a_chief_window_that_goes_by_itself_after_reading_its_role_note_came_up_and_the_human_is_told_nothing_of_a_launch_that_did_not(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let note = a_long_role_on_its_way(&context, project.id);
    assert_eq!(read_in_the_inbox(&context, project.id, note.id), 0);

    // The window goes on its own as its record shows the note: confirmed as the exit is read.
    // The host says what its screen showed, as it does of a launch that did not come up.
    context
        .host
        .exits_showing(Some(1), None, Some(&["codex: the session ended"]));
    context.exit("chief");
    assert_eq!(context.message(note.id).state, "read");
    let human = context.id(project.id, "human");
    let closed: Vec<String> = context
        .inbox(human)
        .into_iter()
        .filter(|message| message.sender.is_none() && message.body.contains("window closed"))
        .map(|message| message.body)
        .collect();
    assert!(
        closed.is_empty(),
        "its window came up and read its role: {closed:?}"
    );
    context.close();
}

#[test]
fn a_long_message_that_says_no_role_goes_cut_as_any_is_and_is_no_role_told_in_part() {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    // The human's words are longer than a paste is given whole.
    let long = context.note(project.id, "human", "chief", &"x".repeat(17_000));
    context.pass().unwrap();
    context.pass().unwrap();

    assert_eq!(context.message(long.id).state, "delivered");
    let said = said_to_the_chief(&context);
    assert!(
        said.last().is_some_and(
            |text| text.contains(&format!("read all of it with: cf inbox read m-{}", long.id))
        ),
        "it went in cut, as any message that long does"
    );
    assert!(
        not_read(&context, project.id).is_empty(),
        "it said no role, in whole or in part: the human is told nothing of one"
    );
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief")
    );
    context.close();
}

/// What the Resume of a chief whose texts a release rewrote by `extra`
/// characters and a space comes to: the length in UTF-16 units of the note that
/// says its role again, whether the role was recorded as told, and whether the
/// note's opening says to read the rest now.
fn replacement_with(extra: usize) -> (usize, bool, bool) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    context
        .roles
        .rewritten
        .replace(format!(" {}", "x".repeat(extra)));
    relaunch(&context, project.id);
    context.pass().unwrap();
    context.pass().unwrap();
    let note = replacements(&context, chief).remove(0);
    let recorded = recorded(&context, chief).is_some_and(|told| told.ends_with('x'));
    let in_part = note.body.contains("Run that command now");
    context.close();
    (utf16_len(&note.body), recorded, in_part)
}

#[test]
fn a_role_whose_note_is_as_long_as_a_paste_is_given_whole_is_recorded_as_told_and_one_unit_more_is_told_in_part(
) {
    let (base, ..) = replacement_with(0);
    let room = 16_000 - base;
    assert_eq!(
        replacement_with(room),
        (16_000, true, false),
        "the longest a paste goes whole: told, with the opening of a whole role"
    );
    let (length, recorded, in_part) = replacement_with(room + 1);
    assert!(length > 16_000, "{length}");
    assert!(
        !recorded && in_part,
        "one unit more is cut to its opening, which says to read the rest now: told in part until read"
    );
}
