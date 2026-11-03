//! `GET /api/inbox/<id>`: one message, whole, to its recipient or its sender,
//! unless it still waits for the human. The id is the digits as the path had
//! them: the 404 quotes them as typed. It changes nothing: a message read here
//! is received when `cf` says it wrote it whole (`POST /api/answers/read`). A
//! message no window was handed yet, read while the project is closed, waits for
//! the Resume: the reader is told so (409 `waits`), and is given none of its text
//! ([`Waits`]); and so does one a meeting with the chief holds, for its end. A
//! note that tells its reader a role is said to be one (`"role": true`, to its
//! recipient alone): once it was delivered, in part, `cf` says it read it whole by
//! a bound of its own (`role_read_whole`), and the board then counts the role told.

use serde_json::json;

use super::{Answer, Caller, Context, Failure, Request};
use crate::api::views::value;
use crate::api::waits::Waits;

pub(super) async fn handle(
    context: &Context,
    caller: &Caller,
    _request: Request,
    id: &str,
) -> Result<Answer, Failure> {
    // Digits past what a message's number holds name no message.
    let found = match id.parse::<i64>() {
        Ok(number) => context.ledger.borrow().message(number)?,
        Err(_) => None,
    };
    let handle = Some(caller.participant.handle.as_str());
    // What still waits for the human is not yet the recipient's to read.
    let visible = found.filter(|message| {
        message.project_id == caller.project.id
            && (Some(message.recipient.as_str()) == handle || message.sender.as_deref() == handle)
            && !(message.state == "gated" && Some(message.recipient.as_str()) == handle)
    });
    let until = match &visible {
        Some(message) => Waits::now(context, caller)?.until(message),
        None => None,
    };
    match (visible, until) {
        (Some(message), Some(until)) => {
            Err(Failure::refuse(409, "waits", until.refusal(message.id)))
        }
        (Some(message), None) => {
            let role = Some(message.recipient.as_str()) == handle
                && context
                    .ledger
                    .borrow()
                    .role_notes_to(message.recipient_id)?
                    .contains(&message.id);
            let mut answer = json!({ "message": value(&message)? });
            if role {
                answer["role"] = json!(true);
            }
            Ok(Answer::ok(answer))
        }
        (None, _) => Err(Failure::refuse(
            404,
            "unknown-message",
            format!("no message m-{id} for you"),
        )),
    }
}

#[cfg(test)]
mod tests;
