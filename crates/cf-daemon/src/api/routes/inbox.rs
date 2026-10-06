//! `GET /api/inbox` (`api.js:205-207`): the messages waiting in the caller's
//! inbox, newest first, at most a hundred: what the ledger holds for it, less
//! what still waits for the human. An answer whose summary shows all of it
//! (what the list says of a message is its first line, cut) has been read
//! whole by whoever it is for, and is received.

use cf_ledger::{MessageView, Read};
use serde_json::json;

use super::answers::received_whole;
use super::{Answer, Caller, Context, Failure, Request};
use crate::api::views::{value, MessageSummary};

/// How many messages the ledger gives an inbox when it is not told: Node's
/// default.
const LIMIT: i64 = 100;

pub(super) async fn handle(
    context: &Context,
    caller: &Caller,
    _request: Request,
) -> Result<Answer, Failure> {
    let messages = context
        .ledger
        .borrow()
        .inbox(caller.participant.id, LIMIT)?;
    let shown: Vec<MessageSummary> = messages.iter().map(MessageSummary::from).collect();
    let summaries = shown.iter().map(value).collect::<Result<Vec<_>, _>>()?;
    let answer = Answer::ok(json!({ "messages": summaries }));
    let whole: Vec<&MessageView> = messages
        .iter()
        .zip(&shown)
        .filter(|(message, summary)| summary.preview == message.body)
        .map(|(message, _)| message)
        .collect();
    received_whole(context, caller, &whole, Read::Inbox)?;
    Ok(answer)
}

#[cfg(test)]
mod tests;
