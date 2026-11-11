//! A message pasted into Devin's window, only while the window still shows the
//! conversation the message is for.

use std::path::Path;

use super::wire::naming;
use crate::contract::{Pane, PaneHost};
use crate::shared::admission::Sent;
use crate::shared::pane::{write_paste, write_paste_untyped};

/// A refusal before the paste is made: nothing was written.
fn refused(error: &str) -> Sent {
    Sent {
        ok: false,
        refused: true,
        cause: None,
        error: Some(error.to_owned()),
    }
}

/// Why a paste the log does not allow is refused.
const ANOTHER: &str = "Devin is displaying another conversation";

/// What a paste needs the log to say of the conversation the window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Guard {
    /// That a configuration named it last: what every paste needs.
    Configured,
    /// That, or that the log configured none and names it alone, on every line that names one
    /// (`wire::Naming::unannounced`): a window opened again silent, whose delivery goes behind
    /// `/bypass` (the adapter's `DevinWindow::shows`), and then only into a window the human
    /// has not typed into since it opened.
    Unannounced,
}

/// Pastes `text` into `pane` when Devin's wire log says the window shows
/// `session`, the conversation it is known to be on, as `guard` asks it to say so. A window
/// that is known to be on none (no session, or an empty one) shows no conversation a message
/// is for. `read_empty` is the count of the human's keys its input box read
/// empty at, if it did ([`write_paste`]).
pub(super) async fn send(
    host: &dyn PaneHost,
    pane: &Pane,
    wire: &Path,
    session: Option<&str>,
    guard: Guard,
    text: &str,
    read_empty: Option<u64>,
) -> Sent {
    let Ok(naming) = naming(wire) else {
        return refused("Devin conversation is unavailable");
    };
    let Some(session) = session.filter(|session| !session.is_empty()) else {
        return refused(ANOTHER);
    };
    if naming.shown() == Some(session) {
        write_paste(host, pane, text, read_empty).await
    } else if guard == Guard::Unannounced && naming.unannounced() == Some(session) {
        // Only the human could have taken a window that says nothing to another conversation
        // (ConsensFlow types no command into it): the host pastes only while none of their keys
        // has reached it, checked as it pastes.
        write_paste_untyped(host, pane, text, read_empty).await
    } else {
        refused(ANOTHER)
    }
}

#[cfg(test)]
mod tests;
