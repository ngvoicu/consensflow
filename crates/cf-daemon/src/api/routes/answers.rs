//! `POST /api/answers` (`api.js:292-308`): the answer to a question, in words
//! or by choices. It reads its body first. Message numbers run across every
//! project, and every project's chief is `@chief`, so a question is answered
//! only in the caller's own project; the ledger decides who may answer and
//! how, and the dispatcher is woken once the answer is written.

use cf_base::js;
use serde_json::{json, Value};

use super::{Answer, Caller, Context, Failure, Request};
use crate::api::views::{value, MessageSummary};

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
