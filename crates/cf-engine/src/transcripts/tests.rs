//! The copy of a window's conversation, as `src/core/transcripts.js` makes it.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::Observed;
use cf_harness::records::{Item, Reading, Record, Role, Settlement};
use cf_ledger::{NewProject, ParticipantView};
use serde_json::json;

use crate::testing::Context;

/// A test's engine with a project whose chief has a conversation, and how
/// many looks said they wrote something.
fn chief_talking() -> (Context, ParticipantView, Rc<Cell<u32>>) {
    let context = Context::new();
    let request = NewProject::from_json(&json!({
        "directory": "/work/app",
        "name": "app",
        "chief": { "harness": "claude-code", "agent": "apollo" },
        "staff": [],
    }))
    .expect("a project");
    let project = context
        .ledger
        .borrow_mut()
        .create_project(&request)
        .expect("created");
    let chief = project
        .participants
        .into_iter()
        .find(|participant| participant.handle == "chief")
        .expect("its chief");
    context
        .ledger
        .borrow_mut()
        .start_conversation(chief.id, "claude-code")
        .expect("a conversation");
    let wrote = Rc::new(Cell::new(0));
    let heard = Rc::clone(&wrote);
    context
        .dispatcher
        .on_transcript(Rc::new(move || heard.set(heard.get() + 1)));
    (context, chief, wrote)
}

fn item(id: &str, role: Role, text: &str, complete: bool) -> Item {
    Item {
        id: Arc::from(id),
        role,
        text: Arc::from(text),
        complete,
        at: None,
        commentary: false,
    }
}

/// A look that finds `items` in the window's record.
fn look(items: Vec<Item>) -> Observed {
    Observed {
        reading: Some(Arc::new(Reading::Known(Record {
            items,
            in_flight: false,
            asking: false,
            failed: false,
            quota: None,
            settlement: Settlement::Unknown,
        }))),
        settled: true,
        waiting: None,
        failed: false,
        quota: None,
        switched: None,
        unnamed: false,
        took_back: false,
    }
}

/// What the chief's conversation holds once it ends: each item's id and text, in order.
fn copied(context: &Context, chief: &ParticipantView) -> Vec<(String, String)> {
    let mut ledger = context.ledger.borrow_mut();
    let conversation = ledger
        .current_conversation(chief.id)
        .expect("read")
        .expect("one");
    ledger.end_conversation(conversation.id).expect("ended");
    let history = ledger.chief_history(chief.project_id).expect("its history");
    history
        .last()
        .expect("the conversation")
        .items
        .iter()
        .map(|item| (item.id.clone(), item.text.clone()))
        .collect()
}

#[test]
fn a_look_copies_what_is_new_and_the_last_item_again_and_says_so_when_it_wrote() {
    let (context, chief, wrote) = chief_talking();
    let record = context.dispatcher.record_of(chief.id);
    let hello = item("i-1", Role::User, "hello", true);
    let copy = |items: Vec<Item>| {
        context
            .dispatcher
            .copy(&chief, &record, &look(items))
            .expect("copied");
    };
    copy(vec![
        hello.clone(),
        item("i-2", Role::Assistant, "Look", false),
    ]);
    assert_eq!(wrote.get(), 1);
    // Nothing new: the last item, copied again, is as it was.
    copy(vec![
        hello.clone(),
        item("i-2", Role::Assistant, "Look", false),
    ]);
    assert_eq!(wrote.get(), 1, "a look that wrote nothing says nothing");
    copy(vec![
        hello,
        item("i-2", Role::Assistant, "Looking at it", true),
        item("i-3", Role::Tool, "ls", true),
    ]);
    assert_eq!(wrote.get(), 2);
    assert_eq!(
        copied(&context, &chief),
        [
            ("i-1".to_owned(), "hello".to_owned()),
            ("i-2".to_owned(), "Looking at it".to_owned()),
            ("i-3".to_owned(), "ls".to_owned()),
        ]
    );
}

#[test]
fn a_record_that_shrank_is_copied_over_from_the_start() {
    let (context, chief, _) = chief_talking();
    let record = context.dispatcher.record_of(chief.id);
    let hello = item("i-1", Role::User, "hello", true);
    let copy = |items: Vec<Item>| {
        context
            .dispatcher
            .copy(&chief, &record, &look(items))
            .expect("copied");
    };
    copy(vec![
        hello.clone(),
        item("i-2", Role::Assistant, "one", true),
        item("i-3", Role::Assistant, "two", true),
    ]);
    // A resumed window rewrote its record: shorter, a new item where the old ones were.
    copy(vec![hello, item("i-9", Role::Assistant, "again", true)]);
    let ids: Vec<String> = copied(&context, &chief)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert!(ids.contains(&"i-9".to_owned()), "copied: {ids:?}");
}

#[test]
fn a_conversation_followed_is_the_participants_and_copied_from_its_start() {
    let (context, chief, _) = chief_talking();
    let record = context.dispatcher.record_of(chief.id);
    context
        .dispatcher
        .copy(
            &chief,
            &record,
            &look(vec![item("i-1", Role::User, "hello", true)]),
        )
        .expect("copied");
    context
        .dispatcher
        .follow(&chief, &record, "native-cleared")
        .expect("followed");
    let now = context
        .ledger
        .borrow()
        .current_conversation(chief.id)
        .expect("read")
        .expect("one");
    assert_eq!(now.native_session.as_deref(), Some("native-cleared"));
    assert_eq!(*record.copied.borrow(), None, "nothing of it copied yet");
    context
        .dispatcher
        .copy(
            &chief,
            &record,
            &look(vec![item("i-7", Role::User, "fresh", true)]),
        )
        .expect("copied");
    assert_eq!(
        copied(&context, &chief),
        [("i-7".to_owned(), "fresh".to_owned())]
    );
}
