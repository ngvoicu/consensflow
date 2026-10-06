//! `GET /api/inbox/<id>` (`api.js:208-221`): one message, whole, to its
//! recipient or its sender, unless it still waits for the human. The id is the
//! digits as the path had them: the 404 quotes them as typed. An answer read
//! here by the one it is for is received.

use cf_ledger::Read;
use serde_json::json;

use super::answers::received_whole;
use super::{Answer, Caller, Context, Failure, Request};
use crate::api::views::value;

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
    match visible {
        Some(message) => {
            let answer = Answer::ok(json!({ "message": value(&message)? }));
            received_whole(context, caller, &[&message], Read::Inbox)?;
            Ok(answer)
        }
        None => Err(Failure::refuse(
            404,
            "unknown-message",
            format!("no message m-{id} for you"),
        )),
    }
}

#[cfg(test)]
mod tests;
