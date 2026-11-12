//! `POST /api/answers`: the answer to a question, in words or by choices. It
//! reads its body first. Message numbers run across every project, and every
//! project's chief is `@chief`, so a question is answered only in the caller's
//! own project; the ledger decides who may answer and how, and the dispatcher
//! is woken once the answer is written.
//!
//! `POST /api/answers/<id>/receipt`: a door that claimed an answer for its
//! harness says whether it handed it over (`{"received": true}`) or did not.
//! Only the answer's own recipient says so, and only handing it over makes it
//! read; a door that was shut meanwhile is refused (409 `door-closed`), and
//! the answer comes as a message.
//!
//! `POST /api/answers/read`: `cf` says which messages it wrote whole to its
//! output (`{"answers": [ids], "via": "inbox" | "task"}`), once that output is
//! complete: the answers of a thread `cf task get` printed, or the message
//! `cf inbox read` printed, whatever its kind. The ones among them that are
//! for the caller, still queued, and received by the way they were read (the
//! ledger's `Read`) are received: a message read in the inbox is never typed
//! in after it, nor is a result, which the board then shows read. A message
//! whose paste is still on its way (`delivering`) is read all the same, as a
//! window reads a message the moment it has it: the read is kept with it, and told
//! when the delivery is confirmed (a note that says a role) or given back
//! (the ledger's `keep_read`); it is not received yet, and counts for none. A read
//! never receives anything by itself: a response that did not reach its
//! reader, or an output that was cut off, says nothing.

use cf_base::js;
use cf_ledger::Read;
use serde_json::{json, Value};

use super::{Answer, Caller, Context, Failure, Request};
use crate::api::views::{value, MessageSummary};
use crate::api::waits::Waits;

pub(super) async fn handle(
    context: &Context,
    caller: &Caller,
    mut request: Request,
) -> Result<Answer, Failure> {
    let body = request.json().await?;
    let id = body.get("question");
    let asked = match whole_and_positive(id) {
        Some(number) => context.ledger.borrow().message(number)?,
        None => None,
    };
    let Some(asked) = asked
        .filter(|message| message.kind == "question" && message.project_id == caller.project.id)
    else {
        return Err(Failure::refuse(
            404,
            "unknown-message",
            format!("no question m-{} in this project", js::text(id)),
        ));
    };
    let answer = context.ledger.borrow_mut().answer(
        asked.id,
        caller.participant.id,
        body.get("body"),
        body.get("choices"),
    )?;
    (context.kick)();
    Ok(Answer::created(
        json!({ "message": value(&MessageSummary::from(&answer))? }),
    ))
}

/// `cf` wrote messages whole to its output and says so: those among them that
/// are for the caller and still wait in the queue are `read`, each once, as
/// the ledger receives them, and the dispatcher is woken, for the task they
/// were what waited for may go on. A message for anyone else, or one that is
/// not queued, is left as it is, and saying so again wakes nothing. A role `cf`
/// printed in parts it says read by the part and how many there are (`part`, `parts`),
/// of that one note: the ledger reads the role once the last follows every other.
pub(super) async fn read(
    context: &Context,
    caller: &Caller,
    mut request: Request,
) -> Result<Answer, Failure> {
    let body = request.json().await?;
    let invalid = || {
        Failure::refuse(
            400,
            "invalid-read",
            "answers is a list of message numbers and via is inbox or task; part and parts, of one message, are whole numbers from 1, part at most parts",
        )
    };
    let via = match body.get("via").and_then(Value::as_str) {
        Some("inbox") => Read::Inbox,
        Some("task") => Read::Task,
        _ => return Err(invalid()),
    };
    let named = body
        .get("answers")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().map(Value::as_i64).collect::<Option<Vec<_>>>())
        .ok_or_else(invalid)?;
    // A role `cf` printed in parts (T-1513) is said read by the part printed and how many
    // there are, the two together, of the one note.
    let number = |key| match body.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|number| usize::try_from(number).ok())
            .filter(|&number| number > 0)
            .map(Some)
            .ok_or_else(invalid),
    };
    let part = match (number("part")?, number("parts")?, named.as_slice()) {
        (None, None, _) => None,
        (Some(part), Some(parts), &[id]) if part <= parts => Some((id, part, parts)),
        _ => return Err(invalid()),
    };
    let mut ledger = context.ledger.borrow_mut();
    let received = match part {
        Some((id, part, parts)) => {
            ledger.receive_read_part(caller.participant.id, id, part, parts, via)?
        }
        None => ledger.receive_read(caller.participant.id, &named, via)?,
    };
    drop(ledger);
    if received > 0 {
        (context.kick)();
    }
    Ok(Answer::ok(json!({})))
}

/// What the door says of an answer it claimed: the ledger decides what that
/// makes of it, and the dispatcher is woken, for a received answer may
/// have been what its task waited for.
pub(super) async fn receipt(
    context: &Context,
    caller: &Caller,
    mut request: Request,
    id: &str,
) -> Result<Answer, Failure> {
    let body = request.json().await?;
    let received = body
        .get("received")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            Failure::refuse(
                400,
                "invalid-receipt",
                "received is true (handed over) or false (not)",
            )
        })?;
    // Digits past what a message's number holds name no answer.
    let number = id.parse::<i64>().unwrap_or(0);
    context
        .ledger
        .borrow_mut()
        .settle_claim(number, caller.participant.id, received)?;
    (context.kick)();
    let answer = context.ledger.borrow().message(number)?;
    // A claim given back leaves the answer queued: while the project is closed,
    // its summary says none of its text ([`Waits`]).
    let waits = Waits::now(context, caller)?;
    Ok(Answer::ok(json!({
        "message": match &answer {
            Some(answer) => value(&waits.summary(answer))?,
            None => Value::Null,
        },
    })))
}

/// `Number.isInteger(id) && id > 0`: a JSON number that is a whole number and
/// more than zero, as a message number; none for anything else, and for a
/// whole number too big for a message to have.
fn whole_and_positive(id: Option<&Value>) -> Option<i64> {
    let Some(Value::Number(number)) = id else {
        return None;
    };
    if let Some(whole) = number.as_i64() {
        return (whole > 0).then_some(whole);
    }
    // `3.0` is a whole number in JavaScript, as any double with no fraction is.
    let float = number.as_f64()?;
    #[allow(clippy::cast_precision_loss)]
    let most = i64::MAX as f64;
    if float.fract() != 0.0 || float <= 0.0 || float >= most {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some(float as i64)
}

#[cfg(test)]
mod tests;
