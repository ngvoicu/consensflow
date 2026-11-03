//! A header quoted in a window's words is no proof that its message arrived: only
//! an item the message's header begins, as ConsensFlow pastes it, is. The human asks
//! about a message in its recipient's window, quoting its header, and the paste of
//! the message never shows: a look, the window's end and the start each took the
//! quote for the paste, and the message was confirmed though it never arrived (a
//! review of 2026-10-16). It is waited for, and goes again, as one the window never
//! showed does. Held to no Node trace: Node's dispatcher took any item that held the
//! header.

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_harness::records::Role;

/// The human asks about message `id` in the chief's window, quoting its header.
fn the_human_quotes(context: &Context, id: i64) {
    let words = format!(
        "Why is \"{}· note from @zeus]\" still queued?",
        marker_of(id)
    );
    let said = context.adapter.item(Role::User, &words);
    context
        .adapter
        .with("chief", |agent| agent.items.push(said));
}

/// A note to the chief whose paste its record will never show.
fn a_note_that_never_shows(context: &Context) -> (i64, i64) {
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "Lost");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    (project.id, note.id)
}

#[test]
fn a_look_takes_no_header_quoted_in_the_humans_words_for_the_paste() {
    let context = Context::new();
    let (_, note) = a_note_that_never_shows(&context);
    the_human_quotes(&context, note);
    context.pass().unwrap();
    assert_eq!(
        context.message(note).state,
        "delivering",
        "the record has no paste of it"
    );
}

#[test]
fn a_look_takes_no_header_a_message_quotes_after_a_blank_line_for_the_paste() {
    let context = Context::new();
    let (_, note) = a_note_that_never_shows(&context);
    // A paste the chief was given quotes it (Sol's words; Astra's is a message the
    // human passed on first that quotes one held at the gate).
    let quoting = format!(
        "[ConsensFlow m-{} · note from @zeus]\nInvestigate this:\n\n{}· note from @zeus]",
        note + 1,
        marker_of(note)
    );
    let said = context.adapter.item(Role::User, &quoting);
    context
        .adapter
        .with("chief", |agent| agent.items.push(said));
    context.pass().unwrap();
    assert_eq!(
        context.message(note).state,
        "delivering",
        "the record has no paste of it"
    );
}

/// A quote only the record holds, read as the window ends, says nothing of when it came: no
/// copy of it was taken after the hand-over, so it proves nothing, and the message goes
/// again.
#[test]
fn a_window_that_ends_takes_no_header_quoted_in_its_record_for_the_paste() {
    let context = Context::new();
    let (_, note) = a_note_that_never_shows(&context);
    // No look has read it: only the record, read as the window ends, has it.
    the_human_quotes(&context, note);
    context.exit("chief");
    let settled = context.message(note);
    assert_eq!(
        (settled.state.as_str(), settled.reason.as_deref()),
        ("queued", Some("@chief's window closed")),
        "it goes again"
    );
}

/// Since 3.0.7 (chief-meeting, CMT-23, and T-842's P3 for the window's end): an item the
/// window was given after the hand-over that holds the header, which the copy has, takes
/// the message for arrived, unproven, as a look past the arrival window does. It is never
/// pasted again, and the chief is told once that it can read it.
#[test]
fn a_window_that_ends_takes_a_header_quoted_in_its_copy_after_the_hand_over_for_arrived_unproven() {
    let context = Context::new();
    let (_, note) = a_note_that_never_shows(&context);
    // A look copies the quote; then the record cannot be read with no window open.
    the_human_quotes(&context, note);
    context.pass().unwrap();
    context
        .adapter
        .with("chief", |agent| agent.native = "unreadable".to_owned());
    context.exit("chief");
    let settled = context.message(note);
    assert_eq!(
        (settled.state.as_str(), settled.receipt["unproven"].clone()),
        ("delivered", serde_json::json!(true)),
        "arrived, unproven: never pasted again"
    );
}
