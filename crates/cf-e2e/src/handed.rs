//! What the daemon's events say of a message's pastes: which of its `delivery.begun` put it
//! into a window, and which of its `delivery.retried` took a paste for lost.
//!
//! A delivery is begun before it is handed over, and the pane host may refuse the hand-over
//! for a look that is no longer true of the window (N17: its box changed, output is pending, or
//! the person typed since). Such a refusal is made before any byte is written (`paste_gate`),
//! and the daemon says so in a `delivery.retried` whose reason begins with
//! [`CHANGED_AS_HANDED_OVER`]: nothing went in, no attempt is spent, and the next look pastes
//! it. That begun is no paste, and that retry takes nothing for lost. A slow machine meets it
//! wherever a window draws between the daemon's look and its hand-over (T-2414: hosted
//! Windows, a Devin chief that was still drawing its last turn's end).

use serde_json::Value;

/// How the daemon's reason for a hand-over refused before anything was written begins: the
/// engine's own words (`cf_engine::CHANGED_AS_HANDED_OVER`, which a test holds equal).
pub const CHANGED_AS_HANDED_OVER: &str = "the window changed as the message was handed over";

/// Whether `event` is a `delivery.retried` of a hand-over refused before anything was written.
pub fn refused_before_written(event: &Value) -> bool {
    event["kind"] == "delivery.retried"
        && event["data"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.starts_with(CHANGED_AS_HANDED_OVER))
}

fn of(event: &Value, id: i64) -> bool {
    event["data"]["message"] == id
}

/// The `delivery.begun` of the message `id` that went into its window, in order: each but those
/// whose next word of the message's delivery is a refusal before anything was written.
pub fn pastes(events: &[Value], id: i64) -> Vec<&Value> {
    let ours: Vec<&Value> = events
        .iter()
        .filter(|event| of(event, id))
        .filter(|event| {
            event["kind"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("delivery."))
        })
        .collect();
    ours.iter()
        .enumerate()
        .filter(|(_, event)| event["kind"] == "delivery.begun")
        .filter(|(at, _)| {
            !ours[at + 1..]
                .iter()
                .find(|later| {
                    later["kind"] == "delivery.begun" || later["kind"] == "delivery.retried"
                })
                .is_some_and(|next| refused_before_written(next))
        })
        .map(|(_, event)| *event)
        .collect()
}

/// The `delivery.retried` of the message `id` that took a paste for lost, or gave it back for
/// any cause but a hand-over refused before anything was written.
pub fn retried_after_a_paste(events: &[Value], id: i64) -> Vec<&Value> {
    events
        .iter()
        .filter(|event| event["kind"] == "delivery.retried" && of(event, id))
        .filter(|event| !refused_before_written(event))
        .collect()
}

/// The daemon's events of `kind` about the message `id`, in order: its `delivery.begun` that
/// went into the window ([`pastes`]), its `delivery.retried` that came after a paste
/// ([`retried_after_a_paste`]), and every event of any other kind.
pub fn of_message<'a>(events: &'a [Value], kind: &str, id: i64) -> Vec<&'a Value> {
    match kind {
        "delivery.begun" => pastes(events, id),
        "delivery.retried" => retried_after_a_paste(events, id),
        _ => events
            .iter()
            .filter(|event| event["kind"] == kind && of(event, id))
            .collect(),
    }
}

#[cfg(test)]
mod tests;
