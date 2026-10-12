//! The sequence the daemon wrote about an answer, as lines of the door's report: the
//! events that are about the answer and its question, the task's moves, the member's
//! window and the deliveries held, each with its number, its time and what it says.

use cf_e2e::live::members::clock;
use serde_json::{Map, Value};

/// Whether an event of the daemon's is about this check: the answer or the
/// question, the task's own moves, the member's window, or a delivery held.
pub fn is_ours(
    event: &Value,
    (answer, question): (Option<i64>, Option<i64>),
    task: Option<i64>,
    handle: &str,
) -> bool {
    let kind = event["kind"].as_str().unwrap_or_default();
    let message = event["data"]["message"].as_i64();
    message.is_some() && (message == answer || message == question)
        || (kind == "task.state" && task.is_some() && event["data"]["task"].as_i64() == task)
        || (kind.starts_with("window.") && event["participant"] == handle)
        || kind.starts_with("delivery.held")
}

/// An event as a line of the sequence: its number, its time, its kind and what it
/// says. A window's own lines (`window.activity`) carry their fields beside the
/// kind, a ledger event's are in `data`.
pub fn sequence_line(event: &Value) -> String {
    let said = match event.get("data") {
        Some(data) if !data.is_null() => data.clone(),
        _ => Value::Object(
            ["state", "reason", "message"]
                .into_iter()
                .filter(|field| !event[*field].is_null())
                .map(|field| (field.to_owned(), event[field].clone()))
                .collect::<Map<String, Value>>(),
        ),
    };
    format!(
        "       {} {} {} {said}",
        event["n"],
        clock(event["at"].as_str().unwrap_or_default()),
        event["kind"].as_str().unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_sequence_has_the_events_of_the_answer_the_question_the_task_and_the_window_and_the_held_ones(
    ) {
        let event =
            |kind: &str, data: Value| json!({ "n": 1, "at": "t", "kind": kind, "data": data });
        let (ids, task, handle) = ((Some(8), Some(7)), Some(3), "bench-claude-1");
        assert!(is_ours(
            &event("delivery.claimed", json!({ "message": 8 })),
            ids,
            task,
            handle
        ));
        assert!(is_ours(
            &event("message.sent", json!({ "message": 7 })),
            ids,
            task,
            handle
        ));
        assert!(is_ours(
            &event("task.state", json!({ "task": 3, "to": "waiting" })),
            ids,
            task,
            handle
        ));
        assert!(!is_ours(
            &event("task.state", json!({ "task": 4 })),
            ids,
            task,
            handle
        ));
        assert!(!is_ours(
            &event("message.sent", json!({ "message": 9 })),
            ids,
            task,
            handle
        ));
        let window =
            json!({ "kind": "window.activity", "participant": "bench-claude-1", "state": "idle" });
        assert!(is_ours(&window, ids, task, handle));
        assert!(!is_ours(&window, ids, task, "another"));
        assert!(is_ours(
            &json!({ "kind": "delivery.held", "reason": "busy" }),
            ids,
            task,
            handle
        ));
        // An event that names no message is not about the answer, whoever has no message either.
        assert!(!is_ours(
            &json!({ "kind": "project.opened" }),
            (None, None),
            None,
            handle
        ));
    }

    #[test]
    fn an_event_is_a_line_of_its_number_its_time_its_kind_and_what_it_says() {
        let ledger = json!({ "n": 12, "at": "2026-10-11T09:30:00.123Z", "kind": "delivery.claimed", "data": { "message": 8 } });
        assert_eq!(
            sequence_line(&ledger),
            "       12 09:30:00.123 delivery.claimed {\"message\":8}"
        );
        // A window's own line has its fields beside its kind.
        let window = json!({ "n": 13, "at": "2026-10-11T09:30:01.000Z", "kind": "window.activity",
            "participant": "chief", "state": "idle" });
        assert_eq!(
            sequence_line(&window),
            "       13 09:30:01.000 window.activity {\"state\":\"idle\"}"
        );
        let held = json!({ "n": 14, "at": "2026-10-11T09:30:02.000Z", "kind": "delivery.held",
            "reason": "the chief is busy", "message": 9 });
        // In the order the JavaScript check wrote them: state, reason, message.
        assert_eq!(
            sequence_line(&held),
            "       14 09:30:02.000 delivery.held {\"reason\":\"the chief is busy\",\"message\":9}"
        );
    }
}
