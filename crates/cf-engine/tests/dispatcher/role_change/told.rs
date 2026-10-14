//! What the ledger records of what a conversation was told of its role, where
//! the note that told it did not go as plainly as a note pasted and seen to
//! arrive: one the window took before it closed, which the next launch finds in
//! the conversation's own record, and one too long to go whole, which tells the
//! conversation its role in part.

use std::rc::Rc;

use cf_base::text::utf16_len;
use cf_engine::testing::Context;
use cf_ledger::{LedgerError, MessageView};
use serde_json::Value;

use super::{
    chief_launches, codex, project_of, recorded, relaunch, replacements, said_to_the_chief, specs,
    REPLACES, WITH_SPECS,
};

/// The notes ConsensFlow wrote to the human, which say a role was told in part.
fn told_in_part(context: &Context, project: i64) -> Vec<MessageView> {
    let human = context.id(project, "human");
    context
        .inbox(human)
        .into_iter()
        .filter(|message| {
            message.sender.is_none() && message.body.contains("was told its new role in part")
        })
        .collect()
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

    // The window closes before the daemon saw it arrive: the note waits for the next.
    context.close_project(project.id).unwrap();
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
    // The window closes before the daemon saw the note arrive, and the project is
    // open again with no window for its chief (a Resume that a step has overtaken):
    // the head of the chief's queue is the note.
    context.close_project(project.id).unwrap();
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
fn a_role_too_long_for_one_message_goes_cut_is_not_recorded_as_told_and_the_human_is_told_so_at_every_resume(
) {
    let context = codex();
    let project = project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    // A staff too large for one message: the role is past what a paste is given whole.
    context
        .roles
        .rewritten
        .replace(format!(" {}", "x".repeat(17_000)));
    relaunch(&context, project.id);
    context.pass().unwrap();
    context.pass().unwrap();

    let notes = replacements(&context, chief);
    assert_eq!(notes.len(), 1);
    assert_eq!(
        context.message(notes[0].id).state,
        "delivered",
        "it arrived, in part"
    );
    let said = said_to_the_chief(&context);
    assert_eq!(said.len(), 1);
    assert!(
        said[0].contains(&format!(
            "read all of it with: cf inbox read m-{}",
            notes[0].id
        )),
        "the way to read the rest is said: {}",
        &said[0][said[0].len() - 200..]
    );
    assert_eq!(
        recorded(&context, chief).as_deref(),
        Some("instructions for chief"),
        "a role told in part is not recorded as told"
    );
    let board = told_in_part(&context, project.id);
    assert_eq!(board.len(), 1, "the human is told");
    assert!(
        board[0].body.starts_with(
            "@chief was told its new role in part: the staff is too large for one message"
        ),
        "{}",
        board[0].body
    );

    // The next Resume says it again, as nothing was recorded, and says so again.
    context.codex.answer("chief", "Understood.");
    context.pass().unwrap();
    relaunch(&context, project.id);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(replacements(&context, chief).len(), 2);
    assert_eq!(told_in_part(&context, project.id).len(), 2);
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
        told_in_part(&context, project.id).is_empty(),
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
/// says its role again, whether the role was recorded as told, and how many
/// times the human was told it went in part.
fn replacement_with(extra: usize) -> (usize, bool, usize) {
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
    let told = told_in_part(&context, project.id).len();
    context.close();
    (utf16_len(&note.body), recorded, told)
}

#[test]
fn a_role_whose_note_is_as_long_as_a_paste_is_given_whole_is_recorded_as_told_and_one_unit_more_is_told_in_part(
) {
    let (base, ..) = replacement_with(0);
    let room = 16_000 - base;
    assert_eq!(
        replacement_with(room),
        (16_000, true, 0),
        "the longest a paste goes whole: told, and nothing to say to the human"
    );
    assert_eq!(
        replacement_with(room + 1),
        (16_001, false, 1),
        "one unit more is cut to its opening: told in part"
    );
}
