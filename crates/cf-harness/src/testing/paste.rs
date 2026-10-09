//! The ways a pane host can answer a paste into a window, for the channels that
//! are pasted (Claude Code and Devin). The other channels have their rows of the
//! contract in `tests/channels/contract.rs`, against servers of their own; these
//! have the rows of the same contract here.

use serde_json::{json, Value};

use crate::contract::{Admission, HostError};

/// One for each way the delivery contract has: the way, the host's answer or
/// the failure of its bridge, and how the adapters must read the channel's
/// answer to it. A paste is handed over at its first byte. Refused before it,
/// nothing reached the window; an error after it, or no word at all, may have
/// reached it; written, accepted.
pub fn paste_answers() -> Vec<(&'static str, Result<Value, HostError>, Admission)> {
    let failed = |error: &str, message: &str| HostError {
        error: Some(error.to_owned()),
        message: message.to_owned(),
    };
    vec![
        (
            "the pane host refuses the paste before writing a byte",
            Ok(json!({
                "ok": false,
                "admitted": false,
                "bytesWritten": 0,
                "error": "stale-generation",
                "cause": "p1-zeus is at generation 4 now",
            })),
            Admission::Refused {
                reason: "p1-zeus is at generation 4 now".to_owned(),
            },
        ),
        (
            "the pane host fails the paste after writing bytes",
            Ok(json!({
                "ok": false,
                "admitted": null,
                "error": "uncertain",
                "cause": "the pane input failed partway",
            })),
            Admission::Uncertain {
                reason: "the pane input failed partway".to_owned(),
            },
        ),
        (
            "the bridge's own deadline passes before the host answers",
            Err(failed("deadline", "pane.write_paste passed its deadline")),
            Admission::Uncertain {
                reason: "pane.write_paste passed its deadline".to_owned(),
            },
        ),
        (
            "the pane host goes away before it answers",
            Err(failed("eof", "the bridge ended")),
            Admission::Uncertain {
                reason: "the bridge ended".to_owned(),
            },
        ),
        (
            "the pane host writes the paste",
            Ok(json!({ "ok": true })),
            Admission::Admitted { queued: false },
        ),
    ]
}
