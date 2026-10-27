//! No re-paste over a record that holds the header (3.0.7 chief-meeting, CMT-23,
//! T-760's F3): a message whose window's record was given, after its hand-over, a
//! user item that carries its header in a shape the proof cannot match is taken for
//! arrived, unproven, once its paste would go again. It is never pasted again and
//! never said not to have reached its window: `delivery.unproven` keeps the item's
//! shape for filing, and whoever can act on it is told once, in a note. A header in
//! an item the record had before the hand-over proves nothing of it, and a retry says
//! what its looks read (CMT-08, T-760's F5).

use cf_engine::delivery_text::marker_of;
use cf_engine::testing::Context;
use cf_harness::records::Role;
use cf_ledger::NewNote;
use serde_json::{json, Value};

/// The kit's arrival window, and a moment past it.
const PAST_ARRIVAL: i64 = 31_000;

/// A note to the chief whose paste its record will never show as a paste.
fn a_note_that_never_shows(context: &Context) -> (i64, i64) {
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    let note = context.note(project.id, "zeus", "chief", "Lost");
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    (project.id, note.id)
}

/// The chief's record is given an item that carries message `id`'s header behind
/// words: the human quoting it, or a shape of a paste the proof does not know.
fn header_behind_words(context: &Context, id: i64) {
    let words = format!("Why is \"{}· note from @zeus]\" still queued?", marker_of(id));
    let said = context.adapter.item(Role::User, &words);
    context
        .adapter
        .with("chief", |agent| agent.items.push(said));
}

/// The ledger's events of `kind`, as it logged their data.
fn logged(context: &Context, kind: &str) -> Vec<Value> {
    context
        .recorder
        .events()
        .into_iter()
        .filter(|event| event["seam"] == "event" && event["event"]["kind"] == kind)
        .map(|event| event["event"]["data"].clone())
        .collect()
}

/// How many times the chief's window was handed message `id`.
fn handed(context: &Context, id: i64) -> usize {
    logged(context, "delivery.begun")
        .iter()
        .filter(|begun| begun["message"] == json!(id))
        .count()
}

#[test]
fn a_record_that_holds_the_header_after_the_hand_over_takes_it_for_arrived_and_never_pastes_it_again(
) {
    let context = Context::new();
    let (project, note) = a_note_that_never_shows(&context);
    header_behind_words(&context, note);
    context.pass().unwrap();
    assert_eq!(context.message(note).state, "delivering", "not before its time");
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    let settled = context.message(note);
    assert_eq!(settled.state, "delivered");
    assert_eq!(settled.receipt["unproven"], json!(true));
    assert_eq!(handed(&context, note), 1, "never pasted again");
    assert_eq!(logged(&context, "delivery.retried"), Vec::<Value>::new());
    let unproven = logged(&context, "delivery.unproven");
    assert_eq!(unproven.len(), 1);
    assert_eq!(unproven[0]["message"], json!(note));
    assert_eq!(unproven[0]["shape"]["before"], json!("words"));
    assert_eq!(unproven[0]["shape"]["markers"], json!(1));
    let chief = context.id(project, "chief");
    let told: Vec<String> = context
        .inbox(chief)
        .into_iter()
        .filter(|message| message.sender.is_none())
        .map(|message| message.body)
        .collect();
    assert_eq!(
        told,
        [format!(
            "m-{note}'s arrival couldn't be proven from the record; it was not pasted again. @chief can read it with `cf inbox read m-{note}`."
        )],
        "told once"
    );
    for _ in 0..3 {
        context.advance(PAST_ARRIVAL);
        context.pass().unwrap();
    }
    assert_eq!(handed(&context, note), 1);
}

#[test]
fn a_header_the_record_held_before_the_hand_over_proves_nothing_and_the_paste_goes_again() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.adapter.with("chief", |agent| agent.arrive = false);
    // The human quotes the header of the message to come (its id is the next), and a
    // look copies it, before the message is handed over.
    header_behind_words(&context, 1);
    context.pass().unwrap();
    let note = context.note(project.id, "zeus", "chief", "Lost");
    assert_eq!(note.id, 1);
    context.advance(1_000);
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(logged(&context, "delivery.unproven"), Vec::<Value>::new());
    let retried = logged(&context, "delivery.retried");
    assert_eq!(retried.len(), 1, "it goes again");
    // What its looks read is in its retry (CMT-08).
    let look = &retried[0]["look"];
    assert_eq!(look["markers"], json!(1), "{look}");
    assert_eq!(look["first"]["shape"]["before"], json!("words"));
    assert!(look["lastItem"].is_string(), "{look}");
    assert!(look["restMs"].as_i64().unwrap() > 30_000, "{look}");
    assert!(look["longestPassMs"].as_i64().unwrap() >= PAST_ARRIVAL, "{look}");
}

#[test]
fn a_member_s_message_arrived_unproven_is_told_to_its_task_s_requester() {
    let context = Context::new();
    let project = context.with_staff(&["zeus"]);
    context.pass().unwrap();
    context.give(project.id, "zeus", "Parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = context.task(project.id, 1).messages[0].recipient.clone();
    context.adapter.with("zeus", |agent| agent.settled = true);
    context.adapter.with("zeus", |agent| agent.arrive = false);
    let note = context
        .ledger
        .borrow_mut()
        .note(
            project.id,
            &NewNote {
                from: Some("chief".to_owned()),
                to: session.clone(),
                body: "Mind the tests".to_owned(),
                task: Some(1),
            },
        )
        .unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).state, "delivering");
    let words = format!("Did {}· note from @chief] come?", marker_of(note.id));
    let said = context.adapter.item(Role::User, &words);
    context.adapter.with("zeus", |agent| agent.items.push(said));
    context.pass().unwrap();
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(note.id).receipt["unproven"], json!(true));
    let chief = context.id(project.id, "chief");
    assert!(
        context.inbox(chief).iter().any(|message| message.body.starts_with(&format!(
            "m-{}'s arrival couldn't be proven from the record; it was not pasted again. @{session} can read it",
            note.id
        ))),
        "the chief gave T-1: it hears"
    );
}

/// Devin glued the paste behind words the human had left in its box, in one item, and
/// the Enter pressed once more ten seconds after the hand-over admitted the paste again:
/// the item came before that Enter, after the hand-over. It is the message's all the same
/// (the live pacing run on Devin 3000.11.205, 2026-10-27: m-8 pasted twice).
#[test]
fn an_item_that_came_between_the_hand_over_and_the_enter_pressed_again_holds_the_message() {
    let context = Context::new();
    let (_, note) = a_note_that_never_shows(&context);
    context.host.set_snapshot(json!({ "outputQuietMs": 60_000 }));
    context.advance(1_000);
    header_behind_words(&context, note);
    context.pass().unwrap();
    // Its Enter, once more, ten seconds on.
    context.advance(10_000);
    context.pass().unwrap();
    assert!(
        context
            .host
            .inputs()
            .iter()
            .any(|body| body["bytes"][0] == 13),
        "the Enter went once more"
    );
    context.advance(PAST_ARRIVAL);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.message(note).receipt["unproven"], json!(true));
    assert_eq!(handed(&context, note), 1, "never pasted again");
}
