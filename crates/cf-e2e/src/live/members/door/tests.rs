//! What the judgement of an answer at a door holds to: each of its five checks, and
//! the notes of a claim that was given up, on the events the daemon wrote.

use serde_json::json;

use super::*;

/// An event of the daemon's, the `n`th, at the second `second` of a minute.
fn event(n: u32, second: u32, kind: &str, data: Value) -> Value {
    json!({ "n": n, "at": format!("2026-10-11T09:30:{second:02}.000Z"), "kind": kind, "data": data })
}

/// The events of an answer the door received: asked at 1, answered at 10,
/// claimed at 11, read at 12; the task waiting from 2 and working from 12.
fn received() -> Vec<Value> {
    vec![
        event(1, 0, "task.state", json!({ "task": 3, "to": "working" })),
        event(2, 2, "task.state", json!({ "task": 3, "to": "waiting" })),
        event(3, 11, "delivery.claimed", json!({ "message": 8 })),
        event(
            4,
            12,
            "message.read",
            json!({ "message": 8, "receipt": { "door": true } }),
        ),
        event(5, 12, "task.state", json!({ "task": 3, "to": "working" })),
    ]
}

fn thread() -> Value {
    json!({ "number": 3, "messages": [
            { "id": 7, "kind": "question", "createdAt": "2026-10-11T09:30:02.000Z" },
            { "id": 8, "kind": "answer", "replyTo": 7, "state": "read", "attempts": 0,
              "receipt": { "door": true }, "createdAt": "2026-10-11T09:30:10.000Z" },
            { "id": 9, "kind": "result", "body": "COLOUR=Blue" },
        ] })
}

fn question() -> Value {
    thread()["messages"][0].clone()
}

fn judged(events: &[Value], thread: &Value) -> Judged {
    judge(events, thread, &question(), &answer_of(thread), "blue")
}

fn answer_of(thread: &Value) -> Value {
    thread["messages"][1].clone()
}

fn held(judged: &Judged) -> Vec<(&'static str, bool)> {
    judged
        .findings
        .iter()
        .map(|finding| (finding.name, finding.ok))
        .collect()
}

#[test]
fn an_answer_the_door_claimed_and_acknowledged_and_never_pasted_is_received_once() {
    let judged = judged(&received(), &thread());
    assert_eq!(
        held(&judged),
        [
            ("claimed", true),
            ("acknowledged received: true", true),
            ("never pasted", true),
            ("waiting to working", true),
            ("result carries the answer", true),
        ]
    );
    assert_eq!(
        judged.findings[0].detail,
        "m-8 claimed by the door 1.0 s after it was given (event 3)"
    );
    assert_eq!(
            judged.findings[1].detail,
            "m-8 is read, receipt {\"door\":true}, 1 message.read event (event 4, 1.0 s after the claim)"
        );
    assert_eq!(
            judged.findings[2].detail,
            "m-8: 0 delivery.begun, 0 delivery.confirmed, attempts 0; 1 answer to m-7; 1 question on T-3"
        );
    assert_eq!(
        judged.findings[3].detail,
        "T-3: working → waiting → working"
    );
    assert_eq!(judged.findings[4].detail, "\"COLOUR=Blue\"");
    assert!(judged.notes.is_empty());
    assert_eq!(judged.received_at, Some(1_791_711_012_000));
}

#[test]
fn an_answer_that_was_pasted_is_not_received_at_the_door_whoever_read_it() {
    // The door never acknowledged: the daemon pasted the answer into the window.
    let mut events = received();
    events.retain(|event| event["kind"] != "message.read" && event["kind"] != "delivery.claimed");
    events.push(event(6, 14, "delivery.begun", json!({ "message": 8 })));
    events.push(event(7, 15, "delivery.confirmed", json!({ "message": 8 })));
    let mut pasted = thread();
    pasted["messages"][1]["attempts"] = json!(1);
    pasted["messages"][1]["receipt"] = Value::Null;
    let judged = judged(&events, &pasted);
    assert_eq!(
        held(&judged),
        [
            ("claimed", false),
            ("acknowledged received: true", false),
            ("never pasted", false),
            ("waiting to working", true),
            ("result carries the answer", true),
        ]
    );
    assert_eq!(
        judged.findings[0].detail,
        "no delivery.claimed event for m-8"
    );
    assert_eq!(judged.received_at, None);
}

#[test]
fn a_receipt_is_the_doors_only_when_exactly_one_read_says_so() {
    // Two reads of the answer, or one whose receipt is not the door's, are not one receipt.
    let mut twice = received();
    twice.push(event(
        6,
        13,
        "message.read",
        json!({ "message": 8, "receipt": { "door": true } }),
    ));
    assert!(!judged(&twice, &thread()).findings[1].ok);
    let mut by_paste = received();
    by_paste[3]["data"]["receipt"] = json!({ "paste": true });
    assert!(!judged(&by_paste, &thread()).findings[1].ok);
    // The answer is read, with the door's receipt, in the thread as well.
    let mut unread = thread();
    unread["messages"][1]["state"] = json!("queued");
    assert!(!judged(&received(), &unread).findings[1].ok);
}

#[test]
fn a_second_answer_to_the_question_is_a_paste_in_all_but_name() {
    let mut two = thread();
    two["messages"].as_array_mut().unwrap().push(json!(
        { "id": 10, "kind": "answer", "replyTo": 7, "state": "queued", "attempts": 0 }
    ));
    let judged = judged(&received(), &two);
    assert!(!judged.findings[2].ok);
    assert!(
        judged.findings[2].detail.contains("2 answers to m-7"),
        "{}",
        judged.findings[2].detail
    );
}

#[test]
fn a_task_that_never_waited_or_never_went_on_did_not_use_its_door() {
    let mut never_waited = received();
    never_waited.remove(1);
    assert!(!judged(&never_waited, &thread()).findings[3].ok);
    let mut never_resumed = received();
    never_resumed.remove(4);
    let judged = judged(&never_resumed, &thread());
    assert!(!judged.findings[3].ok);
    assert_eq!(judged.findings[3].detail, "T-3: working → waiting");
    // The order is the daemon's: working before waiting is not working after it.
    let backwards: Vec<String> = ["waiting", "working"].map(str::to_owned).into();
    assert!(waited_then_worked(&backwards));
    let wrong: Vec<String> = ["working", "waiting"].map(str::to_owned).into();
    assert!(!waited_then_worked(&wrong));
    assert!(!waited_then_worked(&[]));
}

#[test]
fn a_result_that_does_not_carry_the_answer_or_is_none_fails_the_last_check() {
    let mut other = thread();
    other["messages"][2]["body"] = json!("COLOUR=red");
    assert!(!judged(&received(), &other).findings[4].ok);
    let mut none = thread();
    none["messages"].as_array_mut().unwrap().pop();
    let judged = judged(&received(), &none);
    assert!(!judged.findings[4].ok);
    assert_eq!(judged.findings[4].detail, "no result on T-3");
    // The last result of the thread is the one that is read, and a long one is cut.
    let mut long = thread();
    long["messages"][2]["body"] = json!(format!("blue{}", "x".repeat(200)));
    let detail = judged_detail(&long);
    assert_eq!(detail.chars().count(), 102, "{detail}");
}

fn judged_detail(thread: &Value) -> String {
    judged(&received(), thread).findings[4].detail.clone()
}

#[test]
fn a_claim_that_was_given_up_is_noted_and_does_not_fail_the_answer() {
    // Claimed at 11, given up at 13 for the door's silence, read at 14 all the same.
    let events = vec![
        event(1, 2, "task.state", json!({ "task": 3, "to": "waiting" })),
        event(2, 11, "delivery.claimed", json!({ "message": 8 })),
        event(
            3,
            13,
            "delivery.unclaimed",
            json!({ "message": 8, "because": "the door did not answer" }),
        ),
        event(
            4,
            14,
            "message.read",
            json!({ "message": 8, "receipt": { "door": true } }),
        ),
        event(5, 14, "task.state", json!({ "task": 3, "to": "working" })),
    ];
    let judged = judged(&events, &thread());
    assert!(
        judged.findings.iter().all(|finding| finding.ok),
        "{judged:?}"
    );
    assert_eq!(
            judged.notes,
            [
                "the claim on m-8 was given up 2.0 s after it was made (event 3: \"the door did not answer\"), and the door's receipt still made it read 1.0 s later (event 4)"
            ]
        );
}

#[test]
fn a_claim_made_twice_is_said_and_the_second_is_the_one_given_up_where_it_was() {
    let events = vec![
        event(1, 2, "task.state", json!({ "task": 3, "to": "waiting" })),
        event(2, 10, "delivery.claimed", json!({ "message": 8 })),
        event(
            3,
            11,
            "delivery.unclaimed",
            json!({ "message": 8, "because": "first" }),
        ),
        event(4, 12, "delivery.claimed", json!({ "message": 8 })),
        event(
            5,
            15,
            "delivery.unclaimed",
            json!({ "message": 8, "because": "second" }),
        ),
    ];
    let judged = judged(&events, &thread());
    assert!(
        judged.findings[0]
            .detail
            .ends_with("(event 2; claimed 2 times)"),
        "{}",
        judged.findings[0].detail
    );
    assert!(judged.notes[0].contains("1.0 s after it was made (event 3: \"first\")"));
    // The claim that is given up is the last one before it: the second, three seconds on.
    assert!(judged.notes[1].contains("3.0 s after it was made (event 5: \"second\")"));
    assert!(
        !judged.notes[1].contains("receipt still"),
        "none was written"
    );
}

#[test]
fn the_events_of_an_answer_are_the_ones_that_name_it_and_the_moves_of_a_task_are_its_own() {
    let mut events = received();
    events.push(event(6, 13, "delivery.claimed", json!({ "message": 9 })));
    events.push(event(
        7,
        13,
        "task.state",
        json!({ "task": 4, "to": "done" }),
    ));
    let seen = Seen::of(&events, 8);
    assert_eq!(
        [
            seen.claimed.len(),
            seen.read.len(),
            seen.begun.len(),
            seen.confirmed.len(),
            seen.unclaimed.len()
        ],
        [1, 1, 0, 0, 0]
    );
    assert_eq!(moves(&events, 3), ["working", "waiting", "working"]);
    assert_eq!(moves(&events, 4), ["done"]);
    assert!(moves(&events, 5).is_empty());
}

#[test]
fn a_time_of_the_daemon_is_read_in_milliseconds_and_one_that_is_none_is_none() {
    assert_eq!(
        ms_of(&json!("2026-10-11T09:30:00.123Z")),
        Some(1_791_711_000_123)
    );
    assert_eq!(ms_of(&json!("later")), None);
    assert_eq!(ms_of(&Value::Null), None);
    assert_eq!(
        between(&json!("x"), &json!("2026-10-11T09:30:00.123Z")),
        "?"
    );
}
