//! What an adapter asks of a window's pane: a paste, written as the human would
//! type it, and a snapshot of what the window shows and holds.

use serde_json::{json, Value};

use cf_proto::panes::{ReadEmpty, BOTTOM_LINES};

use crate::contract::{HostError, Pane, PaneHost, NOT_ADMITTED};
use crate::shared::admission::Sent;
use crate::shared::look::{At, Look, NoLook, PasteLicence};

/// The pane's id and generation, as a request names it.
fn named(pane: &Pane) -> Value {
    json!({ "id": pane.id, "generation": pane.generation })
}

/// Pastes `body` into the pane (`writePaste`), as the look that licensed it says: its `at` (the
/// look's `seen` and its count of the human's keys, which the host checks again where it writes),
/// `readEmpty` (that count again and the evidence) where the look's reader read the box empty
/// over the human's unsent keys, and the key that empties the box (`clearFirst`), written with the body. Only a
/// licence of a look makes a paste: a wait writes nothing. The host says whether a paste it did
/// not finish wrote nothing (`admitted: false`) or may have written some; an answer that never
/// came (the bridge ended) or did not say (its deadline passed: the bridge's thirty seconds,
/// or, for a window whose harness declared how it reads a paste, the cap of that harness for
/// this paste and eight seconds more) may have been written: uncertain, never a refusal. A
/// paste the host does not admit as it is asked for, after every wait of the adapter's before
/// it, is refused unwritten ([`PaneHost::admits`]).
pub(crate) async fn write_paste(
    host: &dyn PaneHost,
    pane: &Pane,
    body: &str,
    licence: &PasteLicence,
) -> Sent {
    let PasteLicence::Go {
        at,
        read_empty,
        clear_first,
    } = licence
    else {
        return not_admitted();
    };
    if !host.admits() {
        return not_admitted();
    }
    let mut request = named(pane);
    request["body"] = json!(body);
    request["at"] = json!(at.seen);
    request["typed"] = json!(at.typed);
    if let Some(by) = *read_empty {
        request["readEmpty"] = json!(ReadEmpty {
            typed: at.typed,
            by,
        });
    }
    if let Some(keys) = clear_first {
        // The key is one control byte; the body of a request is text.
        request["clearFirst"] = json!(String::from_utf8_lossy(keys));
    }
    answer(host, "pane.write_paste", request).await
}

/// Presses an Enter into the pane that is not a paste's first, on the look that licensed it
/// (`pane.enter`): the host checks the look again with the pane's writer held, and writes it
/// only if the screen still shows the box the look read, nothing is pending and the human has
/// typed nothing since.
pub async fn enter(host: &dyn PaneHost, pane: &Pane, at: &At) -> Sent {
    let mut request = named(pane);
    request["at"] = json!(at.seen);
    request["typed"] = json!(at.typed);
    answer(host, "pane.enter", request).await
}

async fn answer(host: &dyn PaneHost, operation: &str, request: Value) -> Sent {
    match host.request(operation, request).await {
        Ok(answer) => Sent::from_reply(&answer),
        Err(HostError { error, message }) => Sent {
            ok: false,
            refused: false,
            cause: Some(message),
            error: Some(error.unwrap_or_else(|| "transport".to_owned())),
        },
    }
}

/// One observation of the window now ([`Look`]): the host's snapshot with the screen's last
/// lines. None where the host gave no look, and why.
pub async fn look(host: &dyn PaneHost, pane: &Pane) -> Result<Look, NoLook> {
    let snapshot = snapshot_drawn(host, pane)
        .await
        .map_err(|_| NoLook::NoAnswer)?;
    Look::of(&snapshot)
}

/// A `pane.snapshot` with the screen's last lines as drawn (`bottom`), where a look is read.
pub(crate) async fn snapshot_drawn(host: &dyn PaneHost, pane: &Pane) -> Result<Value, HostError> {
    let mut request = named(pane);
    request["bottom"] = json!(BOTTOM_LINES);
    host.request("pane.snapshot", request).await
}

/// Asks the pane host to admit a native send (`claim`): the pane is current,
/// its input works and no paste is going in. It names its pane `pane`, not
/// `id`. The host's answer is returned as it came, for each channel to read in
/// its own words; a request it never answered is the answer `{ok: false,
/// error: "transport", cause}`, the cause the host's word for it, else its
/// message: a claim never fails. A claim reserves nothing at the host, and
/// whether the host still admits the send ([`PaneHost::admits`]) is asked at
/// the send's own last wait: Pi's, the claim; a send over loopback's, its
/// connection (`Loopback::hand_over`).
pub(crate) async fn claim(host: &dyn PaneHost, pane: &Pane) -> Value {
    let request = json!({ "pane": pane.id, "generation": pane.generation });
    match host.request("pane.claim", request).await {
        Ok(answer) => answer,
        Err(HostError { error, message }) => {
            json!({ "ok": false, "error": "transport", "cause": error.unwrap_or(message) })
        }
    }
}

/// A paste the host did not admit: refused, and nothing of it written.
fn not_admitted() -> Sent {
    Sent {
        ok: false,
        refused: true,
        cause: Some(NOT_ADMITTED.to_owned()),
        error: Some("not-admitted".to_owned()),
    }
}

#[cfg(test)]
mod tests;
