//! `GET /api/inbox`: the messages waiting in the caller's inbox, newest first,
//! at most a hundred: what the ledger holds for it, less what still waits for
//! the human. It lists first lines, cut, and changes nothing: an answer is
//! received when `cf` says it wrote it whole (`POST /api/answers/read`), which
//! a list of previews never does.

use serde_json::json;

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
    let summaries = messages
        .iter()
        .map(|message| value(&MessageSummary::from(message)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Answer::ok(json!({ "messages": summaries })))
}

#[cfg(test)]
mod tests;
