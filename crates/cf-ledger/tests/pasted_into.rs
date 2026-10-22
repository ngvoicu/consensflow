//! The conversation a message on its way was pasted into (migration 0021): marked as
//! a paste goes into an open window, read by a start while the message is on its way,
//! and cleared by the ledger itself whatever takes it off its way. A start reads that
//! conversation's record for it, though the window was switched to another since (the
//! review of the Devin arrival branch, 2026-10-22).

#![allow(clippy::expect_used)]

use std::cell::Cell;
use std::rc::Rc;

use cf_base::time::Clock;
use cf_ledger::{open_ledger, Ledger, NewChief, NewMember, NewNote, NewProject, Options};
use serde_json::json;

/// A clock that says what the test set it to.
struct At(Rc<Cell<i64>>);

impl Clock for At {
    fn now_ms(&mut self) -> i64 {
        self.0.get()
    }
}

/// A project with apollo its chief and zeus its worker; the chief's window on
/// `native-a`, and a note from zeus to the chief begun: the ledger, the note's id and
/// the conversation.
fn begun(dir: &std::path::Path) -> (Ledger, i64, i64, i64) {
    begun_at(dir, Rc::new(Cell::new(1_792_058_400_000)))
}

/// [`begun`], on a clock that says what `at` holds.
fn begun_at(dir: &std::path::Path, at: Rc<Cell<i64>>) -> (Ledger, i64, i64, i64) {
    let options = Options {
        clock: Box::new(At(at)),
        ..Options::default()
    };
    let mut ledger = open_ledger(&dir.join("consensflow.db"), options).expect("a ledger");
    let project = ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "devin".into(),
                agent: Some("apollo".into()),
            },
            staff: vec![NewMember {
                agent: "zeus".into(),
                harness: "devin".into(),
                designer: false,
                roles: vec!["worker".into()],
                tier: "standard".into(),
            }],
            gate: false,
            wake_up: None,
            specs: None,
        })
        .expect("a project");
    let chief = project
        .participants
        .iter()
        .find(|participant| participant.handle == "chief")
        .expect("the chief")
        .id;
    let conversation = ledger
        .start_conversation(chief, "devin")
        .expect("a conversation");
    ledger
        .bind_conversation(conversation.id, "native-a")
        .expect("bound");
    let note = ledger
        .note(
            project.id,
            &NewNote {
                from: Some("zeus".into()),
                to: "chief".into(),
                body: "hello".into(),
                task: None,
            },
        )
        .expect("a note");
    ledger.begin_delivery(note.id).expect("begun");
    (ledger, note.id, chief, conversation.id)
}

#[test]
fn a_paste_into_an_open_window_keeps_its_conversation_while_on_its_way_after_a_switch() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, note, chief, conversation) = begun(dir.path());
    assert_eq!(
        ledger.pasted_into(note).expect("read"),
        None,
        "begun alone, as a launch's first message is: no conversation of its own"
    );
    ledger.paste_into(note, conversation).expect("marked");
    // The human runs /new: the window is followed to another conversation.
    ledger
        .follow_conversation(chief, "devin", "native-b")
        .expect("followed");
    assert_eq!(
        ledger.pasted_into(note).expect("read").as_deref(),
        Some("native-a"),
        "the one it went into"
    );
    ledger
        .confirm_delivery(note, Some(&json!({ "item": "u-1" })))
        .expect("confirmed");
    assert_eq!(ledger.pasted_into(note).expect("read"), None);
}

#[test]
fn whatever_takes_a_message_off_its_way_clears_the_conversation_it_went_into() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, note, _, conversation) = begun(dir.path());
    ledger.paste_into(note, conversation).expect("marked");
    ledger
        .retry_delivery(note, "given back", true)
        .expect("given back");
    let column = |ledger: &Ledger| -> Option<i64> {
        ledger
            .sqlite()
            .query_row(
                "SELECT pasted_into FROM message WHERE id = ?",
                [note],
                |row| row.get(0),
            )
            .expect("the row")
    };
    assert_eq!(column(&ledger), None, "given back: cleared");
    ledger.begin_delivery(note).expect("begun again");
    ledger.paste_into(note, conversation).expect("marked again");
    assert_eq!(column(&ledger), Some(conversation));
    ledger.fail_delivery(note, "given up").expect("failed");
    assert_eq!(column(&ledger), None, "failed: cleared");
    // A message that is not on its way is marked by nothing.
    ledger.paste_into(note, conversation).expect("asked");
    assert_eq!(column(&ledger), None);
}

/// A start asks a harness's store for its word of a paste on its way written since its
/// attempt began (T-280): that attempt's second, never an earlier attempt's, and none for
/// a message not on its way.
#[test]
fn a_message_on_its_way_says_the_second_its_attempt_began() {
    let dir = tempfile::tempdir().expect("a folder");
    let at = Rc::new(Cell::new(1_792_058_400_250));
    let (mut ledger, note, _, _) = begun_at(dir.path(), Rc::clone(&at));
    assert_eq!(
        ledger.delivery_begun_ms(note).expect("read"),
        Some(1_792_058_400_000)
    );
    ledger
        .retry_delivery(note, "given back", true)
        .expect("given back");
    assert_eq!(ledger.delivery_begun_ms(note).expect("read"), None);
    at.set(1_792_058_461_900);
    ledger.begin_delivery(note).expect("begun again");
    assert_eq!(
        ledger.delivery_begun_ms(note).expect("read"),
        Some(1_792_058_461_000),
        "the second attempt's"
    );
}

/// The engine words a paste by why it was last given back (T-280: a start that gave back
/// a paste its window took sends it again saying so): the latest give-back's reason,
/// none for a message never given back.
#[test]
fn a_message_says_why_it_was_last_given_back() {
    let dir = tempfile::tempdir().expect("a folder");
    let (mut ledger, note, _, _) = begun(dir.path());
    assert_eq!(ledger.last_retry_reason(note).expect("read"), None);
    ledger
        .retry_delivery(note, "first reason", true)
        .expect("given back");
    ledger.begin_delivery(note).expect("begun again");
    ledger
        .retry_delivery(note, "second reason", true)
        .expect("given back again");
    assert_eq!(
        ledger.last_retry_reason(note).expect("read").as_deref(),
        Some("second reason")
    );
}
