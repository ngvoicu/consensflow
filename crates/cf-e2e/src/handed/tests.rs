use serde_json::{json, Value};

use super::{pastes, retried_after_a_paste};

fn event(n: i64, kind: &str, message: i64, reason: Option<&str>) -> Value {
    let mut data = json!({ "message": message });
    if let Some(reason) = reason {
        data["reason"] = json!(reason);
    }
    json!({ "n": n, "kind": kind, "project": 1, "data": data })
}

/// CI's run 46057354251 (hosted Windows, `devin_notes_twice` on Devin 3000.12): m-4's first
/// hand-over refused for its box before anything was written, then pasted; m-5 pasted once. The
/// chief's screen took two pastes, one each.
#[test]
fn a_hand_over_refused_for_its_box_is_no_paste_and_takes_nothing_for_lost() {
    let events = [
        event(38, "delivery.begun", 4, None),
        event(
            39,
            "delivery.retried",
            4,
            Some("the window changed as the message was handed over: box-changed"),
        ),
        event(40, "delivery.begun", 4, None),
        event(41, "delivery.begun", 5, None),
        event(42, "delivery.confirmed", 4, None),
        event(43, "delivery.confirmed", 5, None),
    ];
    let pasted = pastes(&events, 4);
    assert_eq!(pasted.len(), 1, "{pasted:?}");
    assert_eq!(pasted[0]["n"], 40, "the paste is the one that went in");
    assert!(retried_after_a_paste(&events, 4).is_empty());
    assert_eq!(pastes(&events, 5).len(), 1);
}

/// A paste taken for lost is still two pastes, and its retry is told: what every case is there to
/// catch.
#[test]
fn a_paste_taken_for_lost_is_two_pastes() {
    let events = [
        event(1, "delivery.begun", 4, None),
        event(
            2,
            "delivery.retried",
            4,
            Some("the harness record never showed it"),
        ),
        event(3, "delivery.begun", 4, None),
    ];
    assert_eq!(pastes(&events, 4).len(), 2);
    assert_eq!(retried_after_a_paste(&events, 4).len(), 1);
}

/// The refusal speaks only for the begun it answers: a refusal of a later hand-over leaves an
/// earlier paste a paste, and a refusal of another message speaks for none of this one's.
#[test]
fn a_refusal_speaks_only_for_the_begun_it_answers() {
    let refused = Some("the window changed as the message was handed over: output-pending");
    let events = [
        event(1, "delivery.begun", 4, None),
        event(
            2,
            "delivery.retried",
            4,
            Some("the harness record never showed it"),
        ),
        event(3, "delivery.begun", 4, None),
        event(4, "delivery.retried", 4, refused),
        event(5, "delivery.begun", 4, None),
        event(6, "delivery.begun", 5, None),
        event(7, "delivery.retried", 6, refused),
    ];
    let pasted: Vec<&Value> = pastes(&events, 4).into_iter().map(|e| &e["n"]).collect();
    assert_eq!(pasted, [&json!(1), &json!(5)]);
    assert_eq!(retried_after_a_paste(&events, 4).len(), 1);
    assert_eq!(pastes(&events, 5).len(), 1);
}
