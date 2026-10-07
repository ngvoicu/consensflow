//! What an adapter asks of a window's pane (`src/channels/pty.js`): a
//! paste, written as the human would type it, and a snapshot of what the
//! window shows and holds.

use serde_json::{json, Value};

use crate::contract::{HostError, Pane, PaneHost};
use crate::shared::admission::Sent;

/// The pane's id and generation, as a request names it.
fn named(pane: &Pane) -> Value {
    json!({ "id": pane.id, "generation": pane.generation })
}

/// Pastes `body` into the pane (`writePaste`). The host says whether a paste
/// it did not finish wrote nothing (`admitted: false`) or may have written
/// some; an answer that never came (the bridge ended) or did not say (its
/// deadline passed) may have been written: uncertain, never a refusal.
pub(crate) async fn write_paste(host: &dyn PaneHost, pane: &Pane, body: &str) -> Sent {
    let mut request = named(pane);
    request["body"] = json!(body);
    match host.request("pane.write_paste", request).await {
        Ok(answer) => Sent::from_reply(&answer),
        Err(HostError { error, message }) => Sent {
            ok: false,
            refused: false,
            cause: Some(message),
            error: Some(error.unwrap_or_else(|| "transport".to_owned())),
        },
    }
}

/// Presses `keys` into the pane as the daemon presses its own (`pane.input`),
/// which the host does not count as the human's: what they leave in the input
/// box holds no paste back. The host's word for why it did not, or the
/// bridge's when it never answered.
pub(crate) async fn write_keys(
    host: &dyn PaneHost,
    pane: &Pane,
    keys: &[u8],
) -> Result<(), String> {
    let mut request = named(pane);
    request["bytes"] = json!(keys);
    match host.request("pane.input", request).await {
        Ok(answer) => {
            let sent = Sent::from_reply(&answer);
            if sent.ok {
                return Ok(());
            }
            Err(sent
                .cause
                .or(sent.error)
                .unwrap_or_else(|| "the window refused the keys".to_owned()))
        }
        Err(HostError { message, .. }) => Err(message),
    }
}

/// What the pane host says of a window now (`pane.snapshot`).
pub(crate) async fn snapshot(host: &dyn PaneHost, pane: &Pane) -> Result<Value, HostError> {
    host.request("pane.snapshot", named(pane)).await
}

/// Asks the pane host to admit a native send (`claim`): the pane is current,
/// its input works and no paste is going in. It names its pane `pane`, not
/// `id`. The host's answer is returned as it came, for each channel to read in
/// its own words; a request it never answered is the answer `{ok: false,
/// error: "transport", cause}`, the cause the host's word for it, else its
/// message: a claim never fails.
pub(crate) async fn claim(host: &dyn PaneHost, pane: &Pane) -> Value {
    let request = json!({ "pane": pane.id, "generation": pane.generation });
    match host.request("pane.claim", request).await {
        Ok(answer) => answer,
        Err(HostError { error, message }) => {
            json!({ "ok": false, "error": "transport", "cause": error.unwrap_or(message) })
        }
    }
}

#[cfg(test)]
mod tests;
