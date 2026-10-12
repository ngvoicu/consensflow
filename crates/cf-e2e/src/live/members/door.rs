//! What the daemon wrote of an answer and of its task, judged: was the answer to
//! a member's question received at the door of the harness that asked it, once
//! (the receipt and stop design, §1.2, §1.6, §2.5, §2.7). The answer is
//!
//! - claimed by the door (`delivery.claimed`),
//! - acknowledged `received: true` (`message.read` with the receipt
//!   `{"door": true}`: only an acknowledged claim leaves that proof),
//! - never pasted (no `delivery.begun` or `delivery.confirmed` for it, no
//!   attempts, no second answer),
//!
//! and its task went from waiting back to working, and the result carries the
//! answer. An answer the door fails to acknowledge comes back as text: the paste
//! is the failure this tells apart from a receipt, whichever harness's door it is.

use serde_json::Value;

use super::super::seconds;
use super::account::plural;

/// What one check of the judgement found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

impl Finding {
    fn new(name: &'static str, ok: bool, detail: String) -> Self {
        Self { name, ok, detail }
    }
}

/// What the daemon wrote of an answer: its events, by what they say happened to
/// the answer's delivery, oldest first.
#[derive(Debug, Clone)]
pub struct Seen<'e> {
    pub claimed: Vec<&'e Value>,
    pub read: Vec<&'e Value>,
    pub begun: Vec<&'e Value>,
    pub confirmed: Vec<&'e Value>,
    pub unclaimed: Vec<&'e Value>,
}

impl<'e> Seen<'e> {
    /// The events of `events` about the message `answer`.
    pub fn of(events: &'e [Value], answer: i64) -> Self {
        let about = |kind: &str| -> Vec<&'e Value> {
            events
                .iter()
                .filter(|event| event["kind"] == kind && event["data"]["message"] == answer)
                .collect()
        };
        Self {
            claimed: about("delivery.claimed"),
            read: about("message.read"),
            begun: about("delivery.begun"),
            confirmed: about("delivery.confirmed"),
            unclaimed: about("delivery.unclaimed"),
        }
    }
}

/// The states the task `task` went to, in order, as the daemon wrote them
/// (`task.state` events).
pub fn moves(events: &[Value], task: i64) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["kind"] == "task.state" && event["data"]["task"] == task)
        .filter_map(|event| event["data"]["to"].as_str().map(str::to_owned))
        .collect()
}

/// Whether a task `moves` made went from waiting back to working: the member
/// waited for its answer and went on with it.
pub fn waited_then_worked(moves: &[String]) -> bool {
    moves
        .iter()
        .position(|state| state == "waiting")
        .is_some_and(|waited| moves[waited..].iter().any(|state| state == "working"))
}

/// A time of the daemon's (an ISO 8601 text), in milliseconds since the epoch.
pub fn ms_of(time: &Value) -> Option<i64> {
    time.as_str()?
        .parse::<jiff::Timestamp>()
        .ok()
        .map(|time| time.as_millisecond())
}

/// The seconds from `earlier` to `later`, two times of the daemon's.
fn between(later: &Value, earlier: &Value) -> String {
    match (ms_of(later), ms_of(earlier)) {
        (Some(later), Some(earlier)) => seconds(later - earlier),
        _ => "?".to_owned(),
    }
}

/// The judgement of one answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judged {
    /// When the door's receipt was written, in milliseconds since the epoch.
    pub received_at: Option<i64>,
    /// What happened to the claim before the receipt: seen, not a failure, for the
    /// answer was not pasted.
    pub notes: Vec<String>,
    /// The five checks of it.
    pub findings: Vec<Finding>,
}

/// What the daemon wrote of the `answer` to the `question` on the task `thread`
/// (`events` are all it wrote): whether it was received at the door, once, and
/// whether the result carries the `said` the answer was.
pub fn judge(
    events: &[Value],
    thread: &Value,
    question: &Value,
    answer: &Value,
    said: &str,
) -> Judged {
    let id = answer["id"].as_i64().unwrap_or(-1);
    let question_id = question["id"].as_i64().unwrap_or(-1);
    let seen = Seen::of(events, id);
    let number = thread["number"].as_i64().unwrap_or(-1);
    let moved = moves(events, number);
    let messages: &[Value] = thread["messages"].as_array().map_or(&[], Vec::as_slice);
    let answers = messages
        .iter()
        .filter(|message| message["kind"] == "answer" && message["replyTo"] == question_id)
        .count();
    let asked = messages
        .iter()
        .filter(|message| message["kind"] == "question")
        .count();
    let result = messages
        .iter()
        .rev()
        .find(|message| message["kind"] == "result");
    let (claim, receipt) = (seen.claimed.first(), seen.read.first());
    let notes = seen
        .unclaimed
        .iter()
        .map(|event| unclaimed_note(id, event, &seen, claim.copied(), receipt.copied()))
        .collect();
    let findings = vec![
        Finding::new(
            "claimed",
            !seen.claimed.is_empty(),
            claim.map_or_else(
                || format!("no delivery.claimed event for m-{id}"),
                |claim| {
                    let more = if seen.claimed.len() > 1 {
                        format!("; claimed {} times", seen.claimed.len())
                    } else {
                        String::new()
                    };
                    format!(
                        "m-{id} claimed by the door {} s after it was given (event {}{more})",
                        between(&claim["at"], &answer["createdAt"]),
                        claim["n"]
                    )
                },
            ),
        ),
        Finding::new(
            "acknowledged received: true",
            answer["state"] == "read"
                && answer["receipt"]["door"] == true
                && seen.read.len() == 1
                && receipt.is_some_and(|receipt| receipt["data"]["receipt"]["door"] == true),
            format!(
                "m-{id} is {}, receipt {}, {}{}",
                answer["state"].as_str().unwrap_or("undefined"),
                answer["receipt"],
                plural(seen.read.len(), "message.read event"),
                match (receipt, claim) {
                    (Some(receipt), Some(claim)) => format!(
                        " (event {}, {} s after the claim)",
                        receipt["n"],
                        between(&receipt["at"], &claim["at"])
                    ),
                    _ => String::new(),
                }
            ),
        ),
        Finding::new(
            "never pasted",
            seen.begun.is_empty()
                && seen.confirmed.is_empty()
                && answer["attempts"] == 0
                && answers == 1,
            format!(
                "m-{id}: {} delivery.begun, {} delivery.confirmed, attempts {}; {} to m-{question_id}; {} on T-{number}",
                seen.begun.len(),
                seen.confirmed.len(),
                answer["attempts"],
                plural(answers, "answer"),
                plural(asked, "question"),
            ),
        ),
        Finding::new(
            "waiting to working",
            waited_then_worked(&moved),
            format!("T-{number}: {}", moved.join(" → ")),
        ),
        Finding::new(
            "result carries the answer",
            result.is_some_and(|result| {
                result["body"]
                    .as_str()
                    .is_some_and(|body| body.to_lowercase().contains(&said.to_lowercase()))
            }),
            result.map_or_else(
                || format!("no result on T-{number}"),
                |result| {
                    let body: String = result["body"]
                        .as_str()
                        .unwrap_or_default()
                        .chars()
                        .take(100)
                        .collect();
                    serde_json::Value::String(body).to_string()
                },
            ),
        ),
    ];
    Judged {
        received_at: receipt.and_then(|receipt| ms_of(&receipt["at"])),
        notes,
        findings,
    }
}

/// What a claim that was given up came to, as a line: when, why, and that the
/// door's receipt still made the answer read where there is one.
fn unclaimed_note(
    id: i64,
    event: &Value,
    seen: &Seen,
    first_claim: Option<&Value>,
    receipt: Option<&Value>,
) -> String {
    // The claim it gave up is the last one made before it, or the first.
    let made = seen
        .claimed
        .iter()
        .rev()
        .find(|found| found["n"].as_i64() < event["n"].as_i64())
        .copied()
        .or(first_claim);
    let after = made.map_or_else(
        || "?".to_owned(),
        |claim| between(&event["at"], &claim["at"]),
    );
    let because = event["data"]["because"].as_str().unwrap_or("undefined");
    let still = receipt.map_or_else(String::new, |receipt| {
        format!(
            ", and the door's receipt still made it read {} s later (event {})",
            between(&receipt["at"], &event["at"]),
            receipt["n"]
        )
    });
    format!(
        "the claim on m-{id} was given up {after} s after it was made (event {}: \"{because}\"){still}",
        event["n"]
    )
}

#[cfg(test)]
mod tests;
