//! What a look reads of the store, where the goldens' scenarios never reach:
//! events out of order or of no JSON, rows no event names, null where a
//! field was read, reading on to ids that are no text, and a writer between
//! a whole read's messages and its parts.

use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

#[test]
fn an_event_out_of_its_order_or_of_no_json_and_rows_no_event_names_fail_the_look() {
    type Stage<'a> = &'a dyn Fn(&Staged);
    let failing: [(&str, Stage<'_>); 6] = [
        ("malformed OpenCode event sequence at 5.5", &|staged| {
            staged
                .store
                .execute(
                    "insert into event values ('e55', ?, 5.5, 'session.updated.1', '{}')",
                    [SESSION],
                )
                .unwrap();
        }),
        ("missing OpenCode event for message m3", &|staged| {
            staged.message(
                "m3",
                10,
                &json!({ "role": "assistant", "time": { "created": 10 } }),
            )
        }),
        ("malformed OpenCode message m3", &|staged| {
            staged.message("m3", 10, &json!("{bad"));
            staged.event(6, "message.updated.1", &info("m3", None));
        }),
        ("malformed OpenCode part p3", &|staged| {
            staged.part("p3", "m2", 11, &json!("{bad"));
            staged.event(6, "message.part.updated.1", &part_of("p3", "m2"));
        }),
        ("malformed OpenCode event e6", &|staged| {
            staged.event(6, "session.updated.1", &json!("{bad"))
        }),
        (
            "OpenCode data is null, where its field info was read",
            &|staged| staged.event(6, "message.updated.1", &json!("null")),
        ),
    ];
    for (reason, stage) in failing {
        let staged = Staged::new();
        staged.stopped();
        stage(&staged);
        assert_eq!(read_once(&staged), format!("unknown: unreadable: {reason}"));
    }
    // A seq read twice.
    let staged = Staged::new();
    staged.stopped();
    staged
        .store
        .execute(
            "insert into event values ('e5b', ?, 5, 'session.updated.1', '{}')",
            [SESSION],
        )
        .unwrap();
    assert_eq!(
        read_once(&staged),
        "unknown: unreadable: malformed OpenCode event sequence at 5"
    );
    // A seq written as text reads as its number; a null event of a type that
    // names nothing is read past.
    let staged = Staged::new();
    staged.stopped();
    staged
        .store
        .execute(
            "insert into event values ('e6', ?, '6', 'session.updated.1', '{}')",
            [SESSION],
        )
        .unwrap();
    staged.event(7, "session.updated.1", &json!("null"));
    assert_eq!(read_once(&staged), settled(&conversation_items(true, 9)));
    // No event at all.
    let staged = Staged::new();
    staged.message(
        "m1",
        1,
        &json!({ "role": "user", "time": { "created": 1 } }),
    );
    assert_eq!(
        read_once(&staged),
        "unknown: unreadable: missing OpenCode event sequence for ses_1"
    );
}

#[test]
fn an_event_naming_an_id_that_is_no_text_is_read_on_as_node_binds_it() {
    let opened = in_flight(&conversation_items(true, 9), false);
    for (id, read) in [
        // Bound as 1: the message whose id is "1" is found, and held twice.
        (json!(true), opened.clone()),
        // Bound as a double, which no text id equals.
        (json!(1), opened.clone()),
        // A named parameter object of no names: the session is bound first.
        (json!({}), opened.clone()),
        (json!([]), opened.clone()),
        (
            json!({ "x": 1 }),
            "unknown: unreadable: unknown named parameter 'x'".to_owned(),
        ),
        (
            json!(["m2"]),
            "unknown: unreadable: unknown named parameter '0'".to_owned(),
        ),
    ] {
        let staged = Staged::new();
        staged.stopped();
        staged.message(
            "1",
            20,
            &json!({ "role": "user", "time": { "created": 20 } }),
        );
        staged.event(6, "message.updated.1", &info("1", None));
        let mut reader = staged.reader();
        assert_eq!(show(&look(&mut reader)), opened);
        staged.event(
            7,
            "message.updated.1",
            &json!({ "info": { "id": id, "sessionID": SESSION } }),
        );
        assert_eq!(show(&look(&mut reader)), read, "{id}");
    }
}

#[test]
fn a_writer_between_a_whole_read_s_messages_and_its_parts_is_not_seen_until_the_next() {
    let staged = Staged::new();
    staged.stopped();
    let file = staged.file();
    let wrote = Arc::new(AtomicBool::new(false));
    let writing = Arc::clone(&wrote);
    let mut reader = Reader {
        session: SESSION.to_owned(),
        env: staged.env.clone(),
        local: local(),
        store: None,
        read: None,
        answer: None,
        keys: Keys::default(),
        between: Some(Box::new(move || {
            if writing.swap(true, Ordering::SeqCst) {
                return;
            }
            let writer = Connection::open(&file).unwrap();
            writer
                .execute_batch(
                    "begin immediate;
                     update part set data = '{\"type\":\"text\",\"text\":\"Written between.\"}' where id = 'p2';
                     insert into event values ('e6', 'ses_1', 6, 'message.part.updated.1',
                       '{\"part\":{\"id\":\"p2\",\"messageID\":\"m2\",\"sessionID\":\"ses_1\"}}');
                     commit;",
                )
                .unwrap();
        })),
    };
    let first = reader.look(&Options::default(), 0);
    assert!(
        wrote.load(Ordering::SeqCst),
        "the writer wrote between the reads"
    );
    assert_eq!(show(&first), settled(&conversation_items(true, 9)));
    let next = reader.look(&Options::default(), 0);
    assert_eq!(
        show(&next),
        settled(
            r#"[["m1","user","Review T-1",true,1],["m2","assistant","Written between.",true,9]]"#
        )
    );
}

#[test]
fn a_lookup_is_prepared_though_no_event_names_a_row() {
    let staged = Staged::new();
    staged.stopped();
    let mut reader = staged.reader();
    assert_eq!(
        show(&look(&mut reader)),
        settled(&conversation_items(true, 9))
    );
    // Node 26: "unreadable: no such column: id", the message lookup prepared
    // with no new event to name a row.
    staged
        .store
        .execute_batch("alter table message rename column id to retired_id")
        .unwrap();
    let read = show(&look(&mut reader));
    assert!(
        read.starts_with("unknown: unreadable: no such column: id"),
        "{read}"
    );
}
