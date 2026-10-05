//! `GET /api/questions/<id>` (`api.js:267-291`): a door waiting for the answer
//! to the question it put on the board. It answers the question and the answer
//! so far, `null` while there is none, after at most `wait` milliseconds
//! (0 to 25,000, asked as `?wait=`, and a door polls again): the ledger is
//! asked every 250 ms, and the wait ends at once when the daemon stops.
//!
//! The question is read once, and answered as it was then.

use std::time::Duration;

use cf_base::js;
use serde_json::json;
use tokio::time::{sleep, Instant};

use super::{Answer, Caller, Context, Failure, Request};
use crate::api::views::{value, Answered, MessageSummary};

/// The longest one poll for an answer may hold, in milliseconds.
const MAX_WAIT_MS: f64 = 25_000.0;

/// How often the ledger is asked for the answer while one waits.
const POLL: Duration = Duration::from_millis(250);

pub(super) async fn handle(
    context: &Context,
    caller: &Caller,
    request: Request,
    id: &str,
) -> Result<Answer, Failure> {
    // Digits past what a message's number holds name no message.
    let asked = match id.parse::<i64>() {
        Ok(number) => context.ledger.borrow().message(number)?,
        Err(_) => None,
    };
    let Some(asked) = asked
        .filter(|message| message.kind == "question" && message.project_id == caller.project.id)
    else {
        return Err(Failure::refuse(
            404,
            "unknown-message",
            format!("no question m-{id}"),
        ));
    };
    if asked.sender.as_deref() != Some(caller.participant.handle.as_str()) {
        return Err(Failure::refuse(
            403,
            "not-your-question",
            format!(
                "m-{} was asked by @{}",
                asked.id,
                asked.sender.as_deref().unwrap_or("null")
            ),
        ));
    }
    let until = Instant::now() + wait_of(request.param("wait"));
    let mut answer = context.ledger.borrow().answer_to(asked.id)?;
    while answer.is_none() && Instant::now() < until && !context.closing.is_set() {
        let step = POLL.min(until.saturating_duration_since(Instant::now()));
        tokio::select! {
            () = sleep(step) => {}
            () = context.closing.wait() => {}
        }
        answer = context.ledger.borrow().answer_to(asked.id)?;
    }
    Ok(Answer::ok(json!({
        "question": value(&MessageSummary::from(&asked))?,
        "answer": match &answer {
            Some(answer) => value(&Answered::from(answer))?,
            None => json!(null),
        },
    })))
}

/// How long the door asked to wait, as `Math.min(25000, Math.max(0,
/// Number(wait) || 0))` reads it: none for a word that is no number, at most
/// 25 s whatever it asks.
fn wait_of(asked: Option<&str>) -> Duration {
    let number = asked.map_or(0.0, js::number);
    let millis = if number.is_nan() { 0.0 } else { number };
    Duration::from_secs_f64(millis.clamp(0.0, MAX_WAIT_MS) / 1000.0)
}

#[cfg(test)]
mod tests;
