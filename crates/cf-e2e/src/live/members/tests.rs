//! What a check stands on that needs no harness and no daemon: how it waits,
//! and how it reads what the daemon printed and opened. (What the rig does with a
//! project, with the stand-in agent for its members, is in `tests/rig/live_members.rs`.)

use std::cell::Cell;

use super::*;

#[test]
fn a_task_is_over_when_it_is_done_accepted_failed_or_cancelled_and_not_before() {
    for state in ["done", "accepted", "failed", "cancelled"] {
        assert!(is_over(&json!({ "state": state })), "{state}");
    }
    for state in ["queued", "working", "waiting", "paused"] {
        assert!(!is_over(&json!({ "state": state })), "{state}");
    }
    assert!(!is_over(&json!({})));
}

#[test]
fn the_notes_of_work_are_the_chiefs_notes_that_say_the_window_of_the_task_closed_with_some_and_no_others(
) {
    let closed = "T-1's window closed with background work still running: sleep 4711";
    let inbox = vec![
        json!({ "kind": "result", "body": "ZEEWIN", "state": "delivered" }),
        json!({ "kind": "note", "body": closed }),
        // 3.0.4's note of what a turn's end ended, which 3.0.6 never writes.
        json!({ "kind": "note", "body": "T-1's turn left background work running. Ended: sleep 9." }),
        json!({ "kind": "note", "body": "T-2's window closed with background work still running: sleep 9" }),
        json!({ "kind": "question", "body": "T-1's window closed with background work" }),
    ];
    assert_eq!(notes_of_work(&inbox, 1), [closed]);
    assert_eq!(
        notes_of_work(&inbox, 2),
        ["T-2's window closed with background work still running: sleep 9"]
    );
    assert_eq!(notes_of_work(&inbox, 3), Vec::<String>::new());
    assert_eq!(notes_of_work(&[], 1), Vec::<String>::new());
}

#[test]
fn the_brief_of_a_task_has_reached_its_window_once_it_is_delivered_or_read() {
    let thread = |state: &str| {
        json!({ "messages": [
            { "id": 1, "kind": "note", "state": "delivered" },
            { "id": 2, "kind": "task", "state": state },
            { "id": 3, "kind": "task", "state": "delivered" },
        ] })
    };
    for state in ["delivered", "read"] {
        assert!(brief_delivered(&thread(state)), "{state}");
    }
    // Only the first message of the task is its brief, and a window that has not
    // shown it has it queued, or on its way in.
    for state in ["queued", "delivering", "gated", "failed", "cancelled"] {
        assert!(!brief_delivered(&thread(state)), "{state}");
    }
    assert!(!brief_delivered(&json!({ "messages": [] })));
    assert!(!brief_delivered(&json!({})));
    assert!(!brief_delivered(&Value::Null));
}

#[test]
fn a_thread_is_read_for_its_first_question_its_last_result_and_the_answer_to_a_question() {
    let thread = json!({ "messages": [
        { "id": 1, "kind": "task" },
        { "id": 2, "kind": "question" },
        { "id": 3, "kind": "question" },
        { "id": 4, "kind": "answer", "replyTo": 9 },
        { "id": 5, "kind": "answer", "replyTo": 2, "state": "read" },
        { "id": 6, "kind": "result", "body": "first" },
        { "id": 7, "kind": "result", "body": "second" },
    ] });
    let question = first_question(&thread).unwrap();
    assert_eq!(question["id"], 2);
    assert_eq!(last_result(&thread).unwrap()["id"], 7);
    assert_eq!(answer_to(&thread, &question).unwrap()["id"], 5);
    // The first answer is the first of all, whichever question it replied to.
    assert_eq!(first_answer(&thread).unwrap()["id"], 4);
    // The answer of another question is not this one's, and a thread with none has none.
    assert!(answer_to(&thread, &json!({ "id": 3 })).is_none());
    let empty = json!({ "messages": [] });
    assert!(first_question(&empty).is_none() && last_result(&empty).is_none());
    assert!(first_question(&json!({})).is_none() && last_result(&json!({})).is_none());
    assert!(first_answer(&empty).is_none());
}

#[test]
fn the_chief_was_given_the_first_result_of_its_inbox_that_was_delivered_and_none_before() {
    let inbox = [
        json!({ "id": 1, "kind": "note", "state": "delivered", "body": "a note" }),
        json!({ "id": 2, "kind": "result", "state": "queued", "body": "on its way" }),
        json!({ "id": 3, "kind": "result", "state": "delivered", "body": "ZEEWIN" }),
        json!({ "id": 4, "kind": "result", "state": "delivered", "body": "a later one" }),
    ];
    // A result still on its way is not the chief's yet, and a note is no result.
    assert_eq!(delivered_result(&inbox).as_deref(), Some("ZEEWIN"));
    assert_eq!(delivered_result(&inbox[..2]), None);
    assert_eq!(delivered_result(&[]), None);
    // A body that is no text is said as JSON says it.
    let counted = [json!({ "kind": "result", "state": "delivered", "body": 7 })];
    assert_eq!(delivered_result(&counted).as_deref(), Some("7"));
}

#[test]
fn a_wait_ends_with_what_it_found_and_says_none_when_the_time_ran_out() {
    let log = Log::new("members test");
    let looks = Cell::new(0);
    let found = find(
        &log,
        "the third look",
        Duration::from_secs(10),
        Duration::from_millis(1),
        || {
            looks.set(looks.get() + 1);
            Ok((looks.get() == 3).then_some("there"))
        },
    )
    .unwrap();
    assert_eq!((found, looks.get()), (Some("there"), 3));
    // Nothing found in time is a check that did not hold, not an error.
    let nothing: Option<()> = find(
        &log,
        "nothing",
        Duration::from_millis(30),
        Duration::from_millis(5),
        || Ok(None),
    )
    .unwrap();
    assert_eq!(nothing, None);
}

#[test]
fn a_look_that_fails_ends_the_wait_with_its_error_at_once() {
    let log = Log::new("members test");
    let looks = Cell::new(0);
    let failed = find(
        &log,
        "a daemon that answers",
        Duration::from_secs(60),
        Duration::from_millis(1),
        || -> Result<Option<()>> {
            looks.set(looks.get() + 1);
            Err(Error::Daemon("the daemon is gone".to_owned()))
        },
    )
    .unwrap_err();
    assert_eq!(failed.to_string(), "the daemon is gone");
    assert_eq!(looks.get(), 1, "it did not look again for a minute");
}

#[test]
fn a_window_names_its_conversation_after_the_flag_it_was_opened_on() {
    let open = |words: &[&str]| json!({ "id": "p1-claude", "argv": words });
    assert_eq!(
        session_of(&open(&[
            "claude",
            "--settings",
            "x",
            "--session-id",
            "s-1",
            "--model",
            "m"
        ])),
        Some("s-1".to_owned())
    );
    // A window on a conversation that is kept is opened on `--resume`.
    assert_eq!(
        session_of(&open(&["claude", "--resume", "s-2"])),
        Some("s-2".to_owned())
    );
    // The others name theirs in their own store.
    assert_eq!(session_of(&open(&["codex", "-c", "x"])), None);
    assert_eq!(session_of(&open(&["claude", "--resume"])), None);
    assert_eq!(session_of(&json!({ "id": "p1-claude" })), None);
}

#[test]
fn the_number_of_a_task_is_the_one_cf_printed_whichever_way_it_says_it() {
    assert_eq!(
        task_number(r#"{"task":{"number":7,"state":"queued"}}"#),
        Some(7)
    );
    assert_eq!(task_number(r#"{"number":3}"#), Some(3));
    assert_eq!(task_number("T-3 added"), None);
    assert_eq!(task_number(""), None);
    assert_eq!(task_number(r#"{"error":"no member"}"#), None);
}

#[test]
fn a_member_window_is_its_pane_its_handle_and_the_conversation_it_opened_on() {
    let window = MemberWindow {
        handle: "bench-claude-1".to_owned(),
        pane: json!({ "id": "p1-bench-claude-1", "generation": 1 }),
        open: Some(json!({ "argv": ["claude", "--session-id", "s-9"] })),
        activity: json!({ "state": "working" }),
        lane: Value::Null,
    };
    assert_eq!(window.id(), "p1-bench-claude-1");
    assert_eq!(window.session().as_deref(), Some("s-9"));
    let unopened = MemberWindow {
        open: None,
        ..window
    };
    assert_eq!(unopened.session(), None);
}
