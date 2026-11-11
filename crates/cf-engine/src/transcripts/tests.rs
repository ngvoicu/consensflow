//! The copy of a window's conversation, as Node's dispatcher made it.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{Observed, Over};
use cf_harness::records::{Detail, Item, Kind, Reading, Record, Role, Settlement, Status};
use cf_ledger::{
    HistoryAsk, HistoryFrom, NewProject, ParticipantView, HISTORY_SCAN_BYTES, PAGE_BYTES,
};
use serde_json::json;

use crate::testing::Context;

/// A test's engine with a project whose chief has a conversation, and how
/// many looks said they wrote something.
fn chief_talking() -> (Context, ParticipantView, Rc<Cell<u32>>) {
    let context = Context::new();
    let request = NewProject::from_json(&json!({
        "directory": "/work/app",
        "name": "app",
        "chief": { "harness": "claude-code", "model": "claude-opus-5" },
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
        synthetic: false,
        compaction: false,
        detail: None,
    }
}

/// A look that finds `items` in the window's record, and `beside` them the items only the
/// history shows, each with the place among the items it stands before.
fn look_beside(items: Vec<Item>, beside: Vec<(usize, Item)>) -> Observed {
    let mut observed = look(items);
    if let Some(reading) = observed.reading.take() {
        let Reading::Known(mut record) = Arc::unwrap_or_clone(reading) else {
            unreachable!("a look's reading is known");
        };
        record.beside = beside;
        observed.reading = Some(Arc::new(Reading::Known(record)));
    }
    observed
}

/// A look that finds `items` in the window's record.
fn look(items: Vec<Item>) -> Observed {
    Observed {
        reading: Some(Arc::new(Reading::Known(Record {
            items,
            in_flight: false,
            asking: false,
            failed: false,
            failure: None,
            failure_kind: None,
            last_call: None,
            took_back: false,
            model_moved: None,
            model_drift: None,
            quota: None,
            settlement: Settlement::Unknown,
            pending: Default::default(),
            monitors: Vec::new(),
            commands: Vec::new(),
            lines: Box::default(),
            beside: Vec::new(),
        }))),
        settled: true,
        over: Over::No,
        waiting: None,
        failed: false,
        quota: None,
        switched: None,
        unnamed: false,
        took_back: false,
        at_prompt: false,
        shared: false,
        begun: Vec::new(),
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
fn a_compaction_s_summary_is_copied_marked_as_one_where_its_reading_says_nothing_of_it() {
    let (context, chief, _) = chief_talking();
    let record = context.dispatcher.record_of(chief.id);
    let mut summary = item("s-1", Role::User, "This session is being continued.", true);
    summary.compaction = true;
    let items = vec![item("u-1", Role::User, "Ship it", true), summary];
    // The reading is the one Node wrote: the summary is a user's item there.
    let read = serde_json::to_value(&items[1]).expect("written");
    assert_eq!(read.get("compaction"), None);
    context
        .dispatcher
        .copy(&chief, &record, &look(items))
        .expect("copied");
    let ask = HistoryAsk {
        from: HistoryFrom::Newest,
        find: None,
        tools: false,
        room: PAGE_BYTES,
        scan: HISTORY_SCAN_BYTES,
    };
    let history = context
        .ledger
        .borrow()
        .read_history(chief.project_id, "chief", &ask)
        .expect("its history");
    let marks: Vec<(&str, &str, bool)> = history
        .entries
        .iter()
        .rev()
        .map(|row| {
            (
                row.item.id.as_str(),
                row.item.role.as_str(),
                row.item.compaction,
            )
        })
        .collect();
    assert_eq!(marks, [("u-1", "user", false), ("s-1", "user", true)]);
}

#[test]
fn a_users_item_with_no_human_in_it_is_copied_as_the_harness_s_own_and_the_rest_as_they_were() {
    let (context, chief, _) = chief_talking();
    let record = context.dispatcher.record_of(chief.id);
    let mut notice = item(
        "n-1",
        Role::User,
        "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>",
        true,
    );
    notice.synthetic = true;
    let items = vec![
        item("u-1", Role::User, "Ship it", true),
        notice,
        item("a-1", Role::Assistant, "Done", true),
    ];
    // The reading is the one Node wrote: the notice is a user's item there.
    assert_eq!(
        serde_json::to_value(&items[1]).expect("written")["role"],
        "user"
    );
    context
        .dispatcher
        .copy(&chief, &record, &look(items))
        .expect("copied");
    let ask = HistoryAsk {
        from: HistoryFrom::Newest,
        find: None,
        tools: false,
        room: PAGE_BYTES,
        scan: HISTORY_SCAN_BYTES,
    };
    let history = context
        .ledger
        .borrow()
        .read_history(chief.project_id, "chief", &ask)
        .expect("its history");
    let roles: Vec<(&str, &str)> = history
        .entries
        .iter()
        .rev()
        .map(|row| (row.item.id.as_str(), row.item.role.as_str()))
        .collect();
    assert_eq!(
        roles,
        [("u-1", "user"), ("n-1", "custom"), ("a-1", "assistant")]
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

/// The chief's history as the viewer reads it, oldest first: each item's id, its kind and
/// header where it has them.
fn drawn(
    context: &Context,
    chief: &ParticipantView,
) -> Vec<(String, Option<String>, Option<String>)> {
    let ask = HistoryAsk {
        from: HistoryFrom::Newest,
        find: None,
        tools: true,
        room: PAGE_BYTES,
        scan: HISTORY_SCAN_BYTES,
    };
    let history = context
        .ledger
        .borrow()
        .read_history(chief.project_id, "chief", &ask)
        .expect("its history");
    history
        .entries
        .iter()
        .rev()
        .map(|row| {
            let detail = row.item.detail.as_ref();
            (
                row.item.id.clone(),
                detail.map(|detail| detail.kind.clone()),
                detail.and_then(|detail| detail.header.clone()),
            )
        })
        .collect()
}

#[test]
fn a_look_copies_the_timeline_its_items_and_those_beside_them_each_where_it_stands() {
    let (context, chief, wrote) = chief_talking();
    let record = context.dispatcher.record_of(chief.id);
    let thought = |id: &str| {
        let mut thought = item(id, Role::Custom, "The file first.", true);
        thought.detail = Some(Arc::new(Detail::of(Kind::Thinking)));
        thought
    };
    let mut call = item("t-1", Role::Tool, "", true);
    call.detail = Some(Arc::new(Detail {
        header: Some(Arc::from("$ ls no-such-folder")),
        status: Some(Status::Running),
        ..Detail::of(Kind::Tool)
    }));
    let words = vec![
        item("u-1", Role::User, "go", true),
        item("a-1", Role::Assistant, "Looking", false),
    ];
    let copy = |observed: Observed| {
        context
            .dispatcher
            .copy(&chief, &record, &observed)
            .expect("copied");
    };
    copy(look_beside(
        words.clone(),
        vec![(1, thought("r-1:thinking:0")), (2, call.clone())],
    ));
    assert_eq!(wrote.get(), 1);
    // The same timeline again writes nothing: the copy counts it, not the items alone.
    copy(look_beside(
        words.clone(),
        vec![(1, thought("r-1:thinking:0")), (2, call.clone())],
    ));
    assert_eq!(wrote.get(), 1, "a look that wrote nothing says nothing");
    // The call's result comes: it is an item now, the call no longer beside them.
    let mut result = item("t-1", Role::Tool, "ls: no-such-folder: No such file", true);
    result.detail = Some(Arc::new(Detail {
        header: Some(Arc::from("$ ls no-such-folder")),
        status: Some(Status::Failed),
        ..Detail::of(Kind::Tool)
    }));
    let mut answered = words;
    answered.push(result);
    copy(look_beside(answered, vec![(1, thought("r-1:thinking:0"))]));
    assert_eq!(wrote.get(), 2);
    let tool = Some("tool".to_owned());
    assert_eq!(
        drawn(&context, &chief),
        [
            ("u-1".to_owned(), None, None),
            (
                "r-1:thinking:0".to_owned(),
                Some("thinking".to_owned()),
                None
            ),
            ("a-1".to_owned(), None, None),
            (
                "t-1".to_owned(),
                tool,
                Some("$ ls no-such-folder".to_owned())
            ),
        ]
    );
    // The words alone are what `cf history` reads of it: no thinking among them.
    assert_eq!(
        copied(&context, &chief),
        [
            ("u-1".to_owned(), "go".to_owned()),
            ("a-1".to_owned(), "Looking".to_owned()),
            (
                "t-1".to_owned(),
                "ls: no-such-folder: No such file".to_owned()
            ),
        ]
    );
}

#[test]
fn an_items_detail_is_copied_as_the_ledger_takes_it_and_a_reading_never_says_it() {
    let mut call = item("t-1", Role::Tool, "out", true);
    call.detail = Some(Arc::new(Detail {
        name: Some(Arc::from("Bash")),
        header: Some(Arc::from("$ ls")),
        status: Some(Status::Ok),
        took_ms: Some(1_200),
        diff: Some(Arc::from("-a\n+b")),
        tokens: None,
        kind: Kind::Tool,
    }));
    assert_eq!(
        super::to_copy(&call, false),
        json!({
            "id": "t-1", "role": "tool", "text": "out", "complete": true,
            "detail": { "kind": "tool", "name": "Bash", "header": "$ ls", "status": "ok",
                        "tookMs": 1_200, "diff": "-a\n+b" },
        })
    );
    assert_eq!(
        super::to_copy(&item("r-1", Role::Custom, "hm", true), true),
        json!({ "id": "r-1", "role": "custom", "text": "hm", "complete": true, "drawnOnly": true })
    );
    // The reading, as the recordings hold it, says none of it.
    assert_eq!(
        serde_json::to_value(&call).expect("written"),
        json!({ "id": "t-1", "role": "tool", "text": "out", "complete": true })
    );
}
