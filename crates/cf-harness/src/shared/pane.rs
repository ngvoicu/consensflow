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

/// What the pane host says of a window now (`pane.snapshot`).
pub(crate) async fn snapshot(host: &dyn PaneHost, pane: &Pane) -> Result<Value, HostError> {
    host.request("pane.snapshot", named(pane)).await
}
