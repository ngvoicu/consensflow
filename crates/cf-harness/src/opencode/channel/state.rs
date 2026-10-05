//! Which conversation the window shows (`sessionState`,
//! `src/channels/opencode.js`). Only the launch-owned TUI can attest it: the
//! plugin inside answers for its launch, with the conversation (none on the
//! home screen or the session list) and what OpenCode says it is doing,
//! `{type: 'idle' | 'busy'}` or `{type: 'retry', message, next, …}` while it
//! waits to retry a refused request.

use cf_base::js;
use serde_json::Value;

use super::{is_session_id, json_of, succeeded, Channel, Wires};
use crate::seams::arm;
use crate::seams::loopback::{Method, Request};

/// How long the plugin has to answer, request and body together.
const ANSWER_MS: u64 = 1000;

/// What the plugin says the window shows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Shown {
    /// The conversation, or none where the TUI shows none.
    pub(crate) session: Option<String>,
    /// Whether the plugin sent its id as a list. The pattern an id is tested
    /// by reads a list of one text as that text too, but such an id equals
    /// no conversation's: Node names the conversation by the list, which
    /// is kept here as the text it reads as, and as none the window shows.
    pub(crate) listed: bool,
    /// What OpenCode says it is doing, null where the plugin said nothing.
    pub(crate) status: Value,
}

/// What the plugin of `channel`'s window says it shows: none where it does
/// not answer for this launch, in time, in JSON, or with a conversation's id.
pub(crate) async fn session_state(wires: Wires<'_>, channel: &Channel) -> Option<Shown> {
    let timeout = arm(wires.time, ANSWER_MS);
    let request = Request {
        method: Method::Get,
        url: format!("{}/session", channel.bridge.endpoint),
        headers: vec![(
            "authorization".to_owned(),
            format!("Bearer {}", channel.bridge.token),
        )],
        body: None,
    };
    let mut reply = timeout.bound(wires.loopback.send(request)).await?.ok()?;
    let status = reply.status();
    let body = timeout.bound(reply.body(usize::MAX)).await?.ok()?;
    let current = json_of(&body)?;
    let ours = current.get("launchId").and_then(Value::as_str) == Some(&channel.launch_id);
    if !succeeded(status) || !ours {
        return None;
    }
    let (session, listed) = match current.get("sessionId")? {
        Value::Null => (None, false),
        other => {
            // The pattern tests the text of whatever came, as `test` makes one.
            let text = js::string(Some(other)).ok()?;
            if !is_session_id(&text) {
                return None;
            }
            (Some(text.into_owned()), !other.is_string())
        }
    };
    let status = current
        .get("status")
        .filter(|status| !status.is_null())
        .cloned()
        .unwrap_or(Value::Null);
    Some(Shown {
        session,
        listed,
        status,
    })
}
