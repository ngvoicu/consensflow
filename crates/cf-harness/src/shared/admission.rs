//! What a channel's answer to a send says of it: only a refusal before the
//! channel's hand-over (`admitted: false`) says nothing reached the harness.
//! Any other failure may have reached it, so it is uncertain, and the harness's
//! own record decides rather than a blind second send.

use cf_base::js;
use serde_json::Value;

use crate::contract::Admission;

/// A channel's answer to a send, as an adapter reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    /// `ok: true`.
    pub ok: bool,
    /// `admitted: false`: refused before the hand-over.
    pub refused: bool,
    /// The channel's own words for what went wrong, then its code for it.
    pub cause: Option<String>,
    pub error: Option<String>,
}

impl Sent {
    /// A channel's answer as JSON reads (`sent?.ok === true`, ...).
    pub(crate) fn from_reply(reply: &Value) -> Self {
        let text = |name: &str| {
            reply
                .get(name)
                .filter(|value| !value.is_null())
                .map(|value| js::text(Some(value)).into_owned())
        };
        Self {
            ok: reply.get("ok") == Some(&Value::Bool(true)),
            refused: reply.get("admitted") == Some(&Value::Bool(false)),
            cause: text("cause"),
            error: text("error"),
        }
    }
}

/// How `sent` reads as a delivery's outcome, `refusal` the sentence of a
/// failure that says none, `queued` whether a success went into the
/// harness's own queue.
pub(crate) fn admission(sent: &Sent, refusal: &str, queued: bool) -> Admission {
    if sent.ok {
        return Admission::Admitted { queued };
    }
    let reason = sent
        .cause
        .clone()
        .or_else(|| sent.error.clone())
        .unwrap_or_else(|| refusal.to_owned());
    if sent.refused {
        Admission::Refused { reason }
    } else {
        Admission::Uncertain { reason }
    }
}

#[cfg(test)]
mod tests;
