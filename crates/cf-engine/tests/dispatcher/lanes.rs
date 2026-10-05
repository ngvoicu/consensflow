//! Which window each of a trace's effects belongs to, and whether two traces
//! differ only by how independent windows' effects interleave.
//!
//! JavaScript reached a fake's answer through one microtask hop per async
//! function the call passed through; the kit's executor answers a turn
//! later, whatever wraps the call. So where the chief's look passes through
//! more async functions than a worker's launch, the launch overtook the look
//! in Node and does not in Rust. A test whose trace differs only so is held
//! with [`first_difference`] instead of in strict order.
//!
//! An effect's lane is its window, `p<project>-<handle>`: a pane host's
//! call by the pane it names, an adapter's call by its launch, a token by
//! whom it is for, a trace line by its participant, and of the ledger's
//! events, a conversation's by its participant and a delivery's by its
//! message's recipient. What is no window's (an operation, any other event
//! of the ledger, a forgotten project, the log) is in every lane: it keeps
//! its place among all the others.

use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

/// Where `node` and `rust` differ other than by the interleaving of
/// independent windows: none when each window's effects, with what is no
/// window's, come in the same order on both.
pub fn first_difference(node: &[Value], rust: &[Value]) -> Option<String> {
    let (node_lanes, rust_lanes) = (lanes(node), lanes(rust));
    let names: BTreeSet<&str> = node_lanes
        .iter()
        .chain(&rust_lanes)
        .flatten()
        .map(String::as_str)
        .collect();
    let mut wanted: Vec<Option<&str>> = names.into_iter().map(Some).collect();
    if wanted.is_empty() {
        wanted.push(None);
    }
    for lane in wanted {
        let pick = |events: &[Value], of: &[Option<String>]| -> Vec<Value> {
            events
                .iter()
                .zip(of)
                .filter(|(_, owner)| owner.is_none() || owner.as_deref() == lane)
                .map(|(event, _)| event.clone())
                .collect()
        };
        let (left, right) = (pick(node, &node_lanes), pick(rust, &rust_lanes));
        if let Some(at) = (0..left.len().max(right.len())).find(|&at| left.get(at) != right.get(at))
        {
            let show = |events: &[Value]| {
                events[at.saturating_sub(3)..(at + 3).min(events.len())]
                    .iter()
                    .map(|event| format!("    {event}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            return Some(format!(
                "in {}'s lane at {at}\n  node:\n{}\n  rust:\n{}",
                lane.unwrap_or("no window"),
                show(&left),
                show(&right)
            ));
        }
    }
    None
}

/// Each effect's window, none for what is no window's.
fn lanes(events: &[Value]) -> Vec<Option<String>> {
    let mut launches: HashMap<String, String> = HashMap::new();
    let mut tokens: HashMap<String, String> = HashMap::new();
    let mut conversations: HashMap<i64, String> = HashMap::new();
    let mut recipients: HashMap<i64, String> = HashMap::new();
    let pane = |project: &Value, handle: &Value| {
        format!("p{project}-{}", handle.as_str().unwrap_or_default())
    };
    events
        .iter()
        .map(|event| {
            if let Some(host) = event.get("host") {
                let args = &event["args"];
                let named = if host == "request" {
                    &args[1]["id"]
                } else {
                    &args[0]["id"]
                };
                return named.as_str().map(str::to_owned);
            }
            if event.get("adapter").is_some() {
                let launch = event["launch"].as_str();
                if event["method"] == "prepare" {
                    let lane = pane(&event["project"], &event["handle"]);
                    if let Some(launch) = launch {
                        launches.insert(launch.to_owned(), lane.clone());
                    }
                    return Some(lane);
                }
                return launch.and_then(|launch| launches.get(launch).cloned());
            }
            if let Some(token) = event.get("issue").and_then(Value::as_str) {
                let lane = pane(&event["project"], &event["handle"]);
                tokens.insert(token.to_owned(), lane.clone());
                return Some(lane);
            }
            if let Some(token) = event.get("revoke").and_then(Value::as_str) {
                return tokens.get(token).cloned();
            }
            if let Some(launch) = event.get("forget").and_then(Value::as_str) {
                return launches.get(launch).cloned();
            }
            if let Some(logged) = event.get("event") {
                return ledger_lane(logged, &mut conversations, &mut recipients);
            }
            if let Some(line) = event.get("trace") {
                if let (Some(project), Some(participant)) =
                    (line["project"].as_i64(), line["participant"].as_str())
                {
                    return Some(format!("p{project}-{participant}"));
                }
            }
            None
        })
        .collect()
}

/// A ledger event's window: a conversation's participant's, a delivery's
/// recipient's; none for any other. What names whom is learnt as it comes:
/// a conversation by the event that started it, a message by the one that
/// sent it.
fn ledger_lane(
    event: &Value,
    conversations: &mut HashMap<i64, String>,
    recipients: &mut HashMap<i64, String>,
) -> Option<String> {
    let (project, kind, data) = (&event["project"], event["kind"].as_str()?, &event["data"]);
    let lane_of = |handle: &Value| handle.as_str().map(|handle| format!("p{project}-{handle}"));
    let conversation = data["conversation"].as_i64();
    let message = data["message"].as_i64();
    if let ("message.sent" | "task.created", Some(message), Some(lane)) =
        (kind, message, lane_of(&data["to"]))
    {
        recipients.insert(message, lane);
        return None;
    }
    if kind.starts_with("conversation.") {
        if let (Some(conversation), Some(lane)) = (conversation, lane_of(&data["participant"])) {
            conversations.insert(conversation, lane);
        }
        return conversation.and_then(|conversation| conversations.get(&conversation).cloned());
    }
    if kind.starts_with("delivery.") {
        return message.and_then(|message| recipients.get(&message).cloned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn host(pane: &str, op: &str) -> Value {
        json!({ "host": "request", "args": [op, { "id": pane, "generation": 1 }], "answer": { "ok": true } })
    }

    #[test]
    fn two_windows_effects_may_interleave_one_windows_may_not() {
        let (a, b) = (
            host("p1-chief", "pane.snapshot"),
            host("p1-zeus", "pane.input"),
        );
        let a2 = host("p1-chief", "pane.input");
        assert_eq!(
            first_difference(&[a.clone(), b.clone()], &[b.clone(), a.clone()]),
            None
        );
        assert!(first_difference(&[a.clone(), a2.clone()], &[a2, a]).is_some());
    }

    #[test]
    fn what_is_no_windows_keeps_its_place_among_them_all() {
        let pass = json!({ "op": "pass" });
        let a = host("p1-chief", "pane.snapshot");
        assert!(first_difference(&[pass.clone(), a.clone()], &[a.clone(), pass.clone()]).is_some());
        assert!(first_difference(std::slice::from_ref(&pass), &[]).is_some());
        assert_eq!(
            first_difference(&[pass.clone(), a.clone()], &[pass, a]),
            None
        );
    }

    #[test]
    fn a_launchs_calls_are_its_windows_and_a_token_is_whom_it_is_for() {
        let prepare = json!({ "adapter": "adapter:claude-code", "method": "prepare", "launch": "l-1", "project": 1, "handle": "zeus" });
        let issue =
            json!({ "issue": "token-zeus", "participant": 3, "handle": "zeus", "project": 1 });
        let observe =
            json!({ "adapter": "adapter:claude-code", "method": "observe", "launch": "l-1" });
        let revoke = json!({ "revoke": "token-zeus" });
        let chief = host("p1-chief", "pane.snapshot");
        let node = [
            prepare.clone(),
            issue.clone(),
            observe.clone(),
            revoke.clone(),
            chief.clone(),
        ];
        let rust = [
            prepare.clone(),
            chief.clone(),
            issue.clone(),
            observe.clone(),
            revoke.clone(),
        ];
        assert_eq!(first_difference(&node, &rust), None);
        let swapped = [prepare, observe, issue, revoke, chief];
        assert!(first_difference(&node, &swapped).is_some());
    }

    #[test]
    fn a_conversations_and_a_deliverys_events_are_their_windows_and_a_tasks_are_none() {
        let event = |kind: &str, data: Value| json!({ "event": { "project": 1, "kind": kind, "data": data } });
        let sent = event(
            "task.created",
            json!({ "task": 1, "from": "chief", "to": "zeus", "message": 1 }),
        );
        let begun = event("delivery.begun", json!({ "message": 1, "attempt": 1 }));
        let started = event(
            "conversation.started",
            json!({ "participant": "zeus", "conversation": 2 }),
        );
        let bound = event(
            "conversation.bound",
            json!({ "conversation": 2, "nativeSession": "n" }),
        );
        let chief = host("p1-chief", "pane.snapshot");
        let node = [
            sent.clone(),
            begun.clone(),
            started.clone(),
            bound.clone(),
            chief.clone(),
        ];
        let rust = [
            sent.clone(),
            chief.clone(),
            begun.clone(),
            started.clone(),
            bound.clone(),
        ];
        assert_eq!(first_difference(&node, &rust), None);
        // A task's event is no window's: it keeps its place.
        assert!(first_difference(&[sent.clone(), chief.clone()], &[chief, sent]).is_some());
    }
}
