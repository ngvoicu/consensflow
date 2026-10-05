//! One message sent to a Codex window's broker (`send`,
//! `src/channels/codex.js`): the pane is claimed, then the text is handed to
//! the broker for the thread the message is for, which admits it only while
//! the window's TUI still shows that thread. The broker is the hand-over: a
//! message refused before it, by the pane host or at the deadline, is known to
//! have reached nothing, and from it on the broker may have taken the message,
//! so any failure is uncertain and Codex's own record decides; the message is
//! never retried automatically.

use cf_base::js;
use serde_json::{json, Value};

use super::{is_thread, parse, Channel, BODY_LIMIT};
use crate::contract::{Pane, PaneHost};
use crate::seams::loopback::Loopback;
use crate::seams::{arm, Time};
use crate::shared::admission::Sent;
use crate::shared::pane::claim;

/// How long a message has to be taken, from the moment it is sent: the
/// broker's own expiry for it, and the time the request has.
const DEADLINE_MS: i64 = 3_000;

/// The largest whole number JavaScript holds exactly.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// Why a message is uncertain when nothing came of the request.
const TRANSPORT: &str = "native-queue-transport";

/// Why a message is uncertain when the reply to it said nothing the channel
/// can read.
const ADMISSION: &str = "native-queue-admission";

/// Where a message goes and what it is for.
pub(crate) struct Target<'a> {
    pub(crate) channel: &'a Channel,
    /// The thread the message is for: none until the broker has named it.
    pub(crate) thread: Option<&'a str>,
    /// The pane the send is claimed through.
    pub(crate) pane: &'a Pane,
    pub(crate) host: &'a dyn PaneHost,
}

/// What a send answered (the object `send` returned): what the window's
/// broker did with the message, as far as the channel can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Answer {
    pub(crate) ok: bool,
    /// Whether the broker took the message: its own word, or a refusal before
    /// the hand-over said so; none where it may have.
    pub(crate) admitted: Option<bool>,
    pub(crate) error: Option<String>,
    /// `bytesWritten: 0`: nothing reached Codex.
    pub(crate) zero_bytes: bool,
    pub(crate) cause: Option<String>,
}

impl Answer {
    fn admitted() -> Self {
        Self {
            ok: true,
            admitted: Some(true),
            error: None,
            zero_bytes: false,
            cause: None,
        }
    }

    /// Refused with nothing written (`zeroByteRefusal`).
    fn zero_bytes(error: &str, cause: Option<String>) -> Self {
        Self {
            ok: false,
            admitted: Some(false),
            error: Some(error.to_owned()),
            zero_bytes: true,
            cause,
        }
    }

    /// The deadline came before the message could be handed over.
    fn expired() -> Self {
        Self::zero_bytes("expired", None)
    }

    /// A claim the host refused, in the host's own words: its cause, else its
    /// error, else the claim itself where it is no more than a word.
    fn claim_refused(claimed: &Value) -> Self {
        let word = |name: &str| {
            claimed
                .get(name)
                .filter(|value| !value.is_null())
                .map(|value| js::text(Some(value)).into_owned())
        };
        let cause = word("cause")
            .or_else(|| word("error"))
            .or_else(|| claimed.as_str().map(str::to_owned))
            .unwrap_or_else(|| "claim-refused".to_owned());
        Self::zero_bytes("failed-with-zero-bytes", Some(cause))
    }

    /// The broker may or may not have taken the message.
    fn uncertain(cause: &str) -> Self {
        Self {
            ok: false,
            admitted: None,
            error: Some("uncertain".to_owned()),
            zero_bytes: false,
            cause: Some(cause.to_owned()),
        }
    }

    /// What the broker's reply says of the message: it took it, it refused it
    /// with nothing written, or it says something no verdict says. A refusal
    /// counts whatever the reply's status.
    fn from_reply(status: u16, reply: &Value) -> Self {
        let says = |name: &str, flag: bool| reply.get(name) == Some(&Value::Bool(flag));
        if (200..300).contains(&status) && says("ok", true) && says("admitted", true) {
            return Self::admitted();
        }
        let nothing_written = reply.get("bytesWritten").and_then(Value::as_f64) == Some(0.0);
        match reply.get("error") {
            Some(Value::String(error)) if says("admitted", false) && nothing_written => {
                Self::zero_bytes(error, None)
            }
            _ => Self::uncertain(ADMISSION),
        }
    }

    /// The answer as an adapter reads a send's (`admission`).
    pub(crate) fn reading(&self) -> Sent {
        Sent {
            ok: self.ok,
            refused: self.admitted == Some(false),
            cause: self.cause.clone(),
            error: self.error.clone(),
        }
    }
}

/// The pane a send can name: an id, and a generation that is a whole number
/// from 1, one JavaScript holds exactly.
fn pane_named(pane: &Pane) -> Result<(), String> {
    if !pane.id.is_empty() && (1..=MAX_SAFE_INTEGER).contains(&pane.generation) {
        Ok(())
    } else {
        Err("codex-queue delivery needs pane {id, generation}".to_owned())
    }
}

/// The thread a message is for, which must be a canonical id.
fn thread_named(thread: Option<&str>) -> Result<&str, String> {
    thread
        .filter(|thread| is_thread(thread))
        .ok_or_else(|| "codex-queue delivery needs a canonical native session UUID".to_owned())
}

/// Sends `text` to the window `target` names: what became of it, or why it
/// could not even be addressed (before the hand-over, nothing was asked of
/// anyone).
pub(crate) async fn send(
    time: &dyn Time,
    loopback: &dyn Loopback,
    target: &Target<'_>,
    text: &str,
) -> Result<Answer, String> {
    pane_named(target.pane)?;
    let thread = thread_named(target.thread)?;
    let deadline = time.wall_ms().saturating_add(DEADLINE_MS);
    if deadline <= time.wall_ms() {
        return Ok(Answer::expired());
    }
    let claimed = claim(target.host, target.pane).await;
    if claimed.get("ok") != Some(&Value::Bool(true)) {
        return Ok(Answer::claim_refused(&claimed));
    }
    if deadline <= time.wall_ms() {
        return Ok(Answer::expired());
    }
    Ok(hand_over(time, loopback, target.channel, thread, text, deadline).await)
}

/// The text handed to the broker, with what is left of the deadline to be
/// taken in. From here on the broker may be taking the message, so no failure
/// is an error, and what is not the broker's own word is uncertain.
async fn hand_over(
    time: &dyn Time,
    loopback: &dyn Loopback,
    channel: &Channel,
    thread: &str,
    text: &str,
    deadline: i64,
) -> Answer {
    if time.wall_ms() >= deadline {
        return Answer::expired();
    }
    let body = js::stringify(&json!({
        "launchId": channel.launch_id,
        "sessionId": thread,
        "text": text,
        "expiresAt": deadline,
    }));
    let Some(request) = channel.request("/deliver", Some(body)) else {
        return Answer::uncertain(TRANSPORT);
    };
    let left = u64::try_from((deadline - time.wall_ms()).max(1)).unwrap_or(1);
    let attempt = arm(time, left);
    let Some(Ok(mut reply)) = attempt.bound(loopback.send(request)).await else {
        return Answer::uncertain(TRANSPORT);
    };
    let status = reply.status();
    let Some(Ok(bytes)) = attempt.bound(reply.body(BODY_LIMIT)).await else {
        return Answer::uncertain(TRANSPORT);
    };
    // Reading a field of a reply of `null` threw: that is a failure of the
    // transport, where any other reply is the broker's.
    match parse(&bytes) {
        Some(Value::Null) | None => Answer::uncertain(TRANSPORT),
        Some(reply) => Answer::from_reply(status, &reply),
    }
}

#[cfg(test)]
mod tests;
