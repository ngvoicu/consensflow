//! What the page is told of the daemon (`daemon-status`).

use serde::Serialize;

use super::{
    DAEMON_CLOSING_WINDOWS, DAEMON_NOT_CONFIRMED, DAEMON_STARTED_AGAIN, DAEMON_WINDOWS_CLOSED,
};

/// What the page is told about the daemon (its `daemon-status`): up, or down,
/// why, and whether the app is starting it again.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct DaemonStatus {
    pub(super) available: bool,
    pub(super) cause: Option<String>,
    pub(super) retrying: bool,
    /// The cause says what the app is doing and nothing follows it: the page does not add
    /// that the app is starting the daemon again, which it is not yet.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(super) closing: bool,
    /// Said once, in the page's status line, when the daemon is up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) notice: Option<String>,
}

impl DaemonStatus {
    pub(super) fn up() -> Self {
        Self {
            available: true,
            cause: None,
            retrying: false,
            closing: false,
            notice: None,
        }
    }

    pub(super) fn down(cause: &str, retrying: bool) -> Self {
        Self {
            available: false,
            cause: Some(cause.to_string()),
            retrying,
            closing: false,
            notice: None,
        }
    }

    /// The daemon stopped and its windows are being closed.
    pub(super) fn closing_windows() -> Self {
        Self {
            closing: true,
            ..Self::down(DAEMON_CLOSING_WINDOWS, true)
        }
    }

    /// Every window of the daemon that stopped is proven gone: the daemon is started again.
    pub(super) fn windows_closed() -> Self {
        Self::down(DAEMON_WINDOWS_CLOSED, true)
    }

    /// The daemon is up again after it stopped.
    pub(super) fn started_again() -> Self {
        Self {
            notice: Some(DAEMON_STARTED_AGAIN.to_string()),
            ..Self::up()
        }
    }

    /// The end of the old daemon's windows could not be confirmed: nothing is started.
    pub(super) fn not_confirmed() -> Self {
        Self::down(DAEMON_NOT_CONFIRMED, false)
    }
}
