//! A message pasted into Devin's window, only while the window still shows the
//! conversation the message is for.

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
/// is for. `read_empty` is the count of the human's keys its input box read
/// empty at, if it did ([`write_paste`]).
pub(super) async fn send(
    host: &dyn PaneHost,
    pane: &Pane,
    wire: &Path,
    session: Option<&str>,
    text: &str,
    read_empty: Option<u64>,
) -> Sent {
    let Ok(current) = selected_session(wire) else {
        return refused("Devin conversation is unavailable");
    };
    match session.filter(|session| !session.is_empty()) {
        Some(session) if current.as_deref() == Some(session) => {
            write_paste(host, pane, text, read_empty).await
        }
        _ => refused("Devin is displaying another conversation"),
    }
}

#[cfg(test)]
mod tests;
