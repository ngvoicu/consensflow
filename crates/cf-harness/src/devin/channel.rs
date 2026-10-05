//! A message pasted into Devin's window (`src/channels/devin.js`), only while
//! the window still shows the conversation the message is for.

use std::path::Path;

use super::wire::selected_session;
use crate::contract::{Pane, PaneHost};
use crate::shared::admission::Sent;
use crate::shared::pane::write_paste;

/// A refusal before the paste is made: nothing was written.
fn refused(error: &str) -> Sent {
    Sent {
        ok: false,
        refused: true,
        cause: None,
        error: Some(error.to_owned()),
    }
}

/// Pastes `text` into `pane` when Devin's wire log says the window shows
/// `session`, the conversation it is known to be on. A window that is known
/// to be on none (no session, or an empty one) shows no conversation a message
/// is for.
pub(super) async fn send(
    host: &dyn PaneHost,
    pane: &Pane,
    wire: &Path,
    session: Option<&str>,
    text: &str,
) -> Sent {
    let Ok(current) = selected_session(wire) else {
        return refused("Devin conversation is unavailable");
    };
    match session.filter(|session| !session.is_empty()) {
        Some(session) if current.as_deref() == Some(session) => write_paste(host, pane, text).await,
        _ => refused("Devin is displaying another conversation"),
    }
}

#[cfg(test)]
mod tests;
