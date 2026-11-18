//! `GET /api/questions/<id>`: a door waiting for the answer to the question it
//! put on the board. It answers the question and the answer so far, `null`
//! while there is none, after at most `wait` milliseconds (0 to 25,000, asked
//! as `?wait=`, and a door polls again).
//!
//! A poll is a write: the answer it finds is *claimed* for the door, and the
//! paste skips it from then on, until the door says it handed it to its
//! harness (`POST /api/answers/<id>/receipt`) or the claim is voided. The
//! ledger is asked every 250 ms, one claim at a time, and never holds
//! anything across the wait. A door of a closed project waits as one with no
//! answer yet does, whatever came: its answer is kept for the Resume (the
//! ledger's `claim_answer`). A door that was shut by a pause, or by the
//! move, completion, cancellation, failure or acceptance of its task, is told
//! so at once (409 `door-closed`), in words that say whether an answer may still
//! come; a
//! daemon that is stopping, a window that exited (its token revoked), and a
//! client that has gone (the server runs a handler on after its client left)
//! end the wait with no answer and claim nothing: an answer claimed for
//! nobody would be held from the paste, with nobody to hand it over. A claim
//! whose reply still did not get through is asked again by the door, which is
//! given the same answer (`Claim::Answered` for an answer it holds), and
//! otherwise given up by the window's next look at rest or at a dialog of
//! its own, its exit, a pause or the daemon's start.

use std::time::Duration;

use cf_base::js;
use cf_ledger::{Claim, MessageView, TaskView, MOVED_QUESTION};
use serde_json::{json, Value};
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
    loop {
        // Nothing is claimed for a daemon that is stopping, nor for a window
        // that is gone (its token was revoked when it exited), nor for a
        // client that has left: nobody would read what it was given.
        if context.closing.is_set()
            || request.consumer().has_left()
            || context.credentials.resolve(request.bearer()).is_none()
        {
            return unanswered(&asked);
        }
        let claimed = context
            .ledger
            .borrow_mut()
            .claim_answer(asked.id, caller.participant.id)?;
        match claimed {
            Claim::Answered(answer) => {
                (context.kick)();
                return answered(&asked, &answer);
            }
            Claim::Closed => {
                // The task may have moved while this poll waited. Read the question
                // again as well: its withdrawal reason changed in that same step.
                let (current, task) = {
                    let ledger = context.ledger.borrow();
                    let current = ledger.message(asked.id)?.unwrap_or_else(|| asked.clone());
                    let task = match asked.task_number {
                        Some(number) => ledger
                            .task(caller.project.id, number)?
                            .map(|thread| thread.task),
                        None => None,
                    };
                    (current, task)
                };
                return Err(shut(&current, task.as_ref()));
            }
            Claim::Waiting => {}
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return unanswered(&asked);
        }
        tokio::select! {
            () = sleep(POLL.min(left)) => {}
            () = context.closing.wait() => {}
            () = request.consumer().left() => {}
        }
    }
}

/// The question, and no answer yet.
fn unanswered(asked: &MessageView) -> Result<Answer, Failure> {
    Ok(Answer::ok(json!({
        "question": value(&MessageSummary::from(asked))?,
        "answer": Value::Null,
    })))
}

/// The question, and the answer the door now holds.
fn answered(asked: &MessageView, answer: &MessageView) -> Result<Answer, Failure> {
    Ok(Answer::ok(json!({
        "question": value(&MessageSummary::from(asked))?,
        "answer": value(&Answered::from(answer))?,
    })))
}

/// A shut door says whether its question can still be answered on a later turn.
fn shut(asked: &MessageView, task: Option<&TaskView>) -> Failure {
    let id = asked.id;
    let state = task.map(|task| task.state.as_str());
    let moved = asked.reason.as_deref() == Some(MOVED_QUESTION);
    let finished = asked
        .task_number
        .is_some_and(|number| asked.reason.as_deref() == Some(&format!("T-{number} was finished")));
    let words = match (asked.task_number, state) {
        (Some(task), Some("accepted")) => format!(
            "T-{task} is accepted, so nothing waits on m-{id}: it is not answered here and no answer will come. Do not ask it again; end your turn now."
        ),
        (Some(task), Some("cancelled")) => format!(
            "T-{task} is cancelled, so nothing waits on m-{id}: it is not answered here and no answer will come. Do not ask it again; end your turn now."
        ),
        (Some(task), _) if moved => format!(
            "T-{task} moved to a new window that starts over, so nothing waits on m-{id}: it is not answered here and no answer will come. Do not ask it again; end your turn now."
        ),
        (Some(task), Some("done")) if finished => format!(
            "T-{task} is done, so nothing waits on m-{id}: it is not answered here and no answer will come. Do not ask it again; end your turn now."
        ),
        (Some(task), _) if finished => format!(
            "T-{task} was finished, so nothing waits on m-{id}: it is not answered here and no answer will come. Do not ask it again; end your turn now."
        ),
        (Some(task), Some("failed")) => format!(
            "T-{task} failed, so m-{id} is not answered here. If it is reopened in this window, the chief can decide what to send. Do not ask it again; end your turn now."
        ),
        (Some(task), _) => format!(
            "T-{task} was stopped, so m-{id} is not answered here: its answer comes to you as a message when the task goes on. Do not ask it again; end your turn now."
        ),
        (None, _) => format!(
            "m-{id} is not answered here: its answer comes to you as a message. Do not ask it again; end your turn now."
        ),
    };
    Failure::refuse(409, "door-closed", words)
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
