//! What the product is held to at each point of a step, said from what the
//! daemon showed: a message held at the gate has been delivered nowhere, one
//! the human passed on is typed into its window once and when that window's
//! turn is over, the answer to a question is handed to the door that asked,
//! and a declined brief is a cancelled task and a note to the chief. Each check
//! says every way what was seen falls short, a sentence each, and nothing when
//! it does not.

use cf_e2e::live::cut;
use serde_json::Value;

use crate::trace::Trace;

/// The id of a message of the daemon's views.
pub fn id(message: &Value) -> i64 {
    message["id"].as_i64().unwrap_or(-1)
}

/// What a message is called in a sentence: its id, kind and state.
fn named(message: &Value) -> String {
    format!(
        "m-{} ({} {}, {} attempts)",
        message["id"],
        message["kind"].as_str().unwrap_or("?"),
        message["state"].as_str().unwrap_or("?"),
        message["attempts"]
    )
}

/// A message held at the gate and delivered nowhere: the human's to pass on.
pub fn held(message: &Value, trace: &Trace) -> Vec<String> {
    let mut problems = Vec::new();
    if message["state"] != "gated" {
        problems.push(format!("{} is not held at the gate", named(message)));
    }
    if message["attempts"] != 0 {
        problems.push(format!(
            "{} was delivered before it was passed on",
            named(message)
        ));
    }
    let id = id(message);
    let begun = trace.count("delivery.begun", id) + trace.count("delivery.claimed", id);
    if begun > 0 {
        problems.push(format!(
            "m-{id} has {begun} delivery events in the daemon's trace while it is held"
        ));
    }
    problems
}

/// A message the human passed on that a window of `to` was typed it once, and
/// only once that window's turn was over: it is `delivered` on its first and
/// only attempt, the trace has one delivery begun and one confirmed for it and
/// no retry, and `to` was idle when the delivery began.
pub fn typed_once(message: &Value, trace: &Trace, to: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let id = id(message);
    if message["state"] != "delivered" {
        problems.push(format!("{} is not delivered", named(message)));
    }
    if message["attempts"] != 1 {
        problems.push(format!(
            "{} took {} attempts to deliver, not one",
            named(message),
            message["attempts"]
        ));
    }
    for (kind, wanted) in [("delivery.begun", 1), ("delivery.confirmed", 1)] {
        let seen = trace.count(kind, id);
        if seen != wanted {
            problems.push(format!(
                "the trace has {seen} {kind} for m-{id}, not {wanted}"
            ));
        }
    }
    for kind in ["delivery.retried", "delivery.failed", "delivery.cancelled"] {
        let seen = trace.count(kind, id);
        if seen > 0 {
            problems.push(format!("the trace has {seen} {kind} for m-{id}"));
        }
    }
    if let Some(begun) = trace.first("delivery.begun", id) {
        match trace.activity_before(to, begun) {
            Some("idle") => {}
            Some(other) => problems.push(format!(
                "m-{id} was typed into {to}'s window while it was {other}, not once its turn was over"
            )),
            None => problems.push(format!(
                "the daemon had said nothing of {to}'s window before it typed m-{id} there"
            )),
        }
    }
    if message["receipt"].is_null() || message["receipt"] == serde_json::json!({}) {
        problems.push(format!(
            "m-{id} is delivered with no receipt: nothing proved it arrived"
        ));
    }
    problems
}

/// A question's answer the human passed on, handed to the door of the
/// harness that asked: read by it (claimed, with the receipt a door leaves),
/// and never pasted into the window.
pub fn at_the_door(answer: &Value, trace: &Trace) -> Vec<String> {
    let mut problems = Vec::new();
    let id = id(answer);
    if answer["state"] != "read" {
        problems.push(format!("{} was not read by the door", named(answer)));
    }
    if answer["receipt"]["door"] != true {
        problems.push(format!(
            "m-{id} has no door's receipt: {}",
            cut(&answer["receipt"].to_string(), 120)
        ));
    }
    if trace.count("delivery.claimed", id) == 0 {
        problems.push(format!("the trace has no delivery.claimed for m-{id}"));
    }
    let pasted = trace.count("delivery.begun", id);
    if pasted > 0 || answer["attempts"] != 0 {
        problems.push(format!(
            "m-{id} was pasted into the window ({pasted} begun, {} attempts): a door's answer never is",
            answer["attempts"]
        ));
    }
    problems
}

/// A declined brief: cancelled with the human named as the one who declined
/// it, never delivered.
pub fn declined(brief: &Value, trace: &Trace) -> Vec<String> {
    let mut problems = Vec::new();
    let id = id(brief);
    if brief["state"] != "cancelled" {
        problems.push(format!("{} is not cancelled", named(brief)));
    }
    let reason = brief["reason"].as_str().unwrap_or_default();
    if !reason.contains("declined by @human") {
        problems.push(format!(
            "m-{id}'s reason does not say the human declined it: {}",
            cut(reason, 120)
        ));
    }
    if trace.count("delivery.begun", id) > 0 || brief["attempts"] != 0 {
        problems.push(format!("m-{id} was delivered after all"));
    }
    if trace.count("message.declined", id) != 1 {
        problems.push(format!(
            "the trace has {} message.declined for m-{id}, not one",
            trace.count("message.declined", id)
        ));
    }
    problems
}

/// What a result's text must hold to be the task's: `needles`, each once seen.
pub fn says(result: &str, needles: &[&str]) -> Vec<String> {
    needles
        .iter()
        .filter(|needle| !result.contains(**needle))
        .map(|needle| format!("the result does not say {needle}: {}", cut(result, 200)))
        .collect()
}

/// The window's record of a task has `says` as a user's message (what was
/// typed into it) and, as an assistant's, `answer`.
pub fn recorded(items: &[Value], typed: &str, answer: Option<&str>) -> Vec<String> {
    let said = |role: &str, needle: &str| {
        items.iter().any(|item| {
            item["role"] == role
                && item["text"]
                    .as_str()
                    .is_some_and(|text| text.contains(needle))
        })
    };
    let mut problems = Vec::new();
    if !said("user", typed) {
        problems.push(format!(
            "the window's record has no message of the user's that holds {typed:?}"
        ));
    }
    if let Some(answer) = answer {
        if !said("assistant", answer) {
            problems.push(format!(
                "the window's record has no answer of the assistant's that holds {answer:?}"
            ));
        }
    }
    problems
}

/// Whether the text carries the markup of Codex's own app directives
/// (`::codex-…{…}`), which a chief has no use for in a result.
pub fn codex_markup(text: &str) -> Option<String> {
    let at = text.find(":codex-")?;
    let from = text[..at].rfind('\n').map_or(0, |line| line + 1);
    let to = text[at..].find('\n').map_or(text.len(), |line| at + line);
    Some(cut(&text[from..to], 160))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn trace(lines: &[&str]) -> Trace {
        let text = lines.join("\n");
        Trace::of(&text)
    }

    const IDLE: &str = r#"{"kind":"window.activity","participant":"chief","state":"idle"}"#;
    const WORKING: &str = r#"{"kind":"window.activity","participant":"chief","state":"working"}"#;
    const BEGUN: &str = r#"{"kind":"delivery.begun","data":{"message":5,"attempt":1}}"#;
    const CONFIRMED: &str = r#"{"kind":"delivery.confirmed","data":{"message":5}}"#;

    fn delivered() -> Value {
        json!({ "id": 5, "kind": "result", "state": "delivered", "attempts": 1,
                "receipt": { "record": "m-5" } })
    }

    #[test]
    fn a_held_message_has_been_delivered_nowhere() {
        let quiet = trace(&[IDLE]);
        let gated = json!({ "id": 5, "kind": "result", "state": "gated", "attempts": 0 });
        assert_eq!(held(&gated, &quiet), Vec::<String>::new());
        let begun = trace(&[IDLE, BEGUN]);
        let said = held(
            &json!({ "id": 5, "kind": "result", "state": "queued", "attempts": 1 }),
            &begun,
        );
        assert_eq!(said.len(), 3, "{said:?}");
        assert!(said[0].contains("is not held at the gate"));
        assert!(said[1].contains("was delivered before it was passed on"));
        assert!(said[2].contains("1 delivery events"));
    }

    #[test]
    fn a_message_typed_once_after_the_turn_was_over_is_all_the_gate_promises() {
        let ok = trace(&[WORKING, IDLE, BEGUN, CONFIRMED]);
        assert_eq!(typed_once(&delivered(), &ok, "chief"), Vec::<String>::new());
    }

    #[test]
    fn a_message_typed_while_the_window_worked_or_twice_or_with_no_proof_is_said_so() {
        let mid_turn = trace(&[IDLE, WORKING, BEGUN, CONFIRMED]);
        let said = typed_once(&delivered(), &mid_turn, "chief");
        assert_eq!(said.len(), 1);
        assert!(said[0].contains("while it was working"), "{said:?}");

        let twice = trace(&[IDLE, BEGUN, BEGUN, CONFIRMED]);
        let again = json!({ "id": 5, "kind": "result", "state": "delivered", "attempts": 2,
                            "receipt": { "record": "m-5" } });
        let said = typed_once(&again, &twice, "chief");
        assert!(
            said.iter()
                .any(|problem| problem.contains("took 2 attempts")),
            "{said:?}"
        );
        assert!(
            said.iter()
                .any(|problem| problem.contains("2 delivery.begun")),
            "{said:?}"
        );

        let retried = trace(&[
            IDLE,
            BEGUN,
            r#"{"kind":"delivery.retried","data":{"message":5}}"#,
            CONFIRMED,
        ]);
        assert!(typed_once(&delivered(), &retried, "chief")
            .iter()
            .any(|problem| problem.contains("1 delivery.retried")));

        let unproved = json!({ "id": 5, "kind": "result", "state": "delivered", "attempts": 1,
                               "receipt": null });
        assert!(
            typed_once(&unproved, &trace(&[IDLE, BEGUN, CONFIRMED]), "chief")
                .iter()
                .any(|problem| problem.contains("no receipt"))
        );

        let unseen = trace(&[BEGUN, CONFIRMED]);
        assert!(typed_once(&delivered(), &unseen, "chief")
            .iter()
            .any(|problem| problem.contains("had said nothing of chief's window")));
    }

    #[test]
    fn an_answer_is_at_the_door_when_it_was_claimed_and_read_and_never_pasted() {
        let claimed = r#"{"kind":"delivery.claimed","data":{"message":5}}"#;
        let answer = json!({ "id": 5, "kind": "answer", "state": "read", "attempts": 0,
                             "receipt": { "door": true } });
        assert!(at_the_door(&answer, &trace(&[claimed])).is_empty());
        let pasted = json!({ "id": 5, "kind": "answer", "state": "delivered", "attempts": 1,
                             "receipt": { "record": "x" } });
        let said = at_the_door(&pasted, &trace(&[BEGUN]));
        assert_eq!(said.len(), 4, "{said:?}");
    }

    #[test]
    fn a_declined_brief_is_cancelled_by_the_human_and_never_delivered() {
        let declined_event = r#"{"kind":"message.declined","data":{"message":5}}"#;
        let brief = json!({ "id": 5, "kind": "task", "state": "cancelled", "attempts": 0,
                            "reason": "declined by @human" });
        assert!(declined(&brief, &trace(&[declined_event])).is_empty());
        let queued = json!({ "id": 5, "kind": "task", "state": "queued", "attempts": 0,
                             "reason": null });
        let said = declined(&queued, &trace(&[IDLE]));
        assert_eq!(said.len(), 3, "{said:?}");
    }

    #[test]
    fn a_result_says_what_the_task_asked_and_a_window_recorded_what_it_was_given() {
        assert!(says("MARK_OK and more", &["MARK_OK"]).is_empty());
        let said = says("something else", &["MARK_OK", "else"]);
        assert_eq!(said.len(), 1);
        assert!(said[0].starts_with("the result does not say MARK_OK"));

        let items = vec![
            json!({ "role": "user", "text": "Reply with exactly: MARK_OK" }),
            json!({ "role": "assistant", "text": "MARK_OK" }),
        ];
        assert!(recorded(&items, "Reply with exactly: MARK_OK", Some("MARK_OK")).is_empty());
        assert_eq!(
            recorded(&items[..1], "Reply with", Some("MARK_OK")).len(),
            1
        );
        assert_eq!(recorded(&[], "Reply", None).len(), 1);
    }

    #[test]
    fn codex_markup_is_found_by_its_prefix_and_shown_with_the_line_it_is_on() {
        assert_eq!(codex_markup("MARK_OK\nThe jar is drawn."), None);
        let result = "Done.\n::codex-git-stage{cwd=\"/x\"}\nMore.";
        assert_eq!(
            codex_markup(result).as_deref(),
            Some("::codex-git-stage{cwd=\"/x\"}")
        );
        assert_eq!(
            codex_markup("tail :codex-x").as_deref(),
            Some("tail :codex-x")
        );
    }
}
