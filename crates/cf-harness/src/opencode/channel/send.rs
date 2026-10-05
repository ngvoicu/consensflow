//! A message to the window through ConsensFlow's plugin inside its TUI,
//! which posts it to the conversation the TUI shows (`send`,
//! `src/channels/opencode.js`). The pane's claim comes first: a failed claim
//! before the request is known to have sent zero bytes, and anything after
//! the request started is uncertain, for OpenCode's own record to decide.

use cf_base::js;
use serde_json::{json, Value};

use super::{json_of, succeeded, Channel, Wires};
use crate::contract::{Pane, PaneHost};
use crate::seams::arm;
use crate::seams::loopback::{Method, Request};
use crate::shared::admission::Sent;
use crate::shared::pane::claim;

/// How long a message has to be handed over, from the moment it is sent, and
/// how long its request has at most.
const DEADLINE_MS: i64 = 3_000;

/// The largest whole number JavaScript holds exactly.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// Where a message goes: the conversation it is for, and the pane it is
/// claimed through.
pub struct Target<'a> {
    pub session: &'a str,
    pub pane: &'a Pane,
    pub host: &'a dyn PaneHost,
}

/// Sends `text` to the window `channel` leads to, once the pane is claimed:
/// what became of it as an adapter reads a send, or why it could not be
/// asked of the pane at all.
pub async fn send(
    wires: Wires<'_>,
    channel: &Channel,
    target: &Target<'_>,
    text: &str,
) -> Result<Sent, String> {
    if !(1..=MAX_SAFE_INTEGER).contains(&target.pane.generation) {
        return Err("native delivery needs pane {id, generation}".to_owned());
    }
    let expires_at = wires.time.wall_ms().saturating_add(DEADLINE_MS);
    let claimed = claim(target.host, target.pane).await;
    if claimed.get("ok") != Some(&Value::Bool(true)) {
        return Ok(claim_refused(&claimed));
    }
    let left = expires_at - wires.time.wall_ms();
    if left <= 0 {
        return Ok(refused("expired".to_owned()));
    }
    let body = json!({
        "launchId": channel.launch_id,
        "sessionId": target.session,
        "text": text,
        "expiresAt": expires_at,
    });
    let request = Request {
        method: Method::Post,
        url: format!("{}/deliver", channel.bridge.endpoint),
        headers: vec![
            (
                "authorization".to_owned(),
                format!("Bearer {}", channel.bridge.token),
            ),
            ("content-type".to_owned(), "application/json".to_owned()),
        ],
        body: Some(js::stringify(&body).into_bytes()),
    };
    // Whether the plugin took it is its own word, in a reply the request's
    // time bounds; any failure on the way leaves it uncertain.
    let timeout = arm(wires.time, DEADLINE_MS.min(left).unsigned_abs());
    let Some(Ok(mut reply)) = timeout.bound(wires.loopback.send(request)).await else {
        return Ok(uncertain());
    };
    let status = reply.status();
    let Some(Ok(bytes)) = timeout.bound(reply.body(usize::MAX)).await else {
        return Ok(uncertain());
    };
    let Some(result) = json_of(&bytes) else {
        return Ok(uncertain());
    };
    let says = |field: &str, value: Value| result.get(field) == Some(&value);
    if succeeded(status) && says("ok", json!(true)) && says("admitted", json!(true)) {
        return Ok(Sent {
            ok: true,
            refused: false,
            cause: None,
            error: None,
        });
    }
    let nothing_written = result.get("bytesWritten").and_then(Value::as_f64) == Some(0.0);
    if says("admitted", json!(false)) && nothing_written {
        if let Some(error) = result.get("error").and_then(Value::as_str) {
            return Ok(refused(error.to_owned()));
        }
    }
    Ok(uncertain())
}

/// Refused before the request was sent: nothing reached OpenCode.
fn refused(error: String) -> Sent {
    Sent {
        ok: false,
        refused: true,
        cause: None,
        error: Some(error),
    }
}

/// A claim the pane host refused, in the host's own words
/// (`zeroByteClaimRefusal`).
fn claim_refused(claimed: &Value) -> Sent {
    let word = |name: &str| {
        claimed
            .get(name)
            .filter(|value| !value.is_null())
            .map(|value| js::text(Some(value)).into_owned())
    };
    let cause = word("cause")
        .or_else(|| word("error"))
        .unwrap_or_else(|| "claim-refused".to_owned());
    Sent {
        ok: false,
        refused: true,
        cause: Some(cause),
        error: Some("failed-with-zero-bytes".to_owned()),
    }
}

/// OpenCode may or may not have taken the message.
fn uncertain() -> Sent {
    Sent {
        ok: false,
        refused: false,
        cause: None,
        error: Some("uncertain".to_owned()),
    }
}
