//! The channel to an OpenCode window (`src/channels/opencode.js`, and the
//! `opencode` branch of `launchConfiguration`, `src/channels.js`). A window
//! runs a server of its own, on a private port with a password, and ConsensFlow's
//! plugin inside it runs another, which says which conversation the TUI shows
//! (`state`) and takes every message after the first (`send`). The first goes
//! through the window's own server once it answers (`seed`), and the
//! conversation a fresh window opens on is made before it, on a throwaway
//! server of its own (`create`).

mod create;
mod directory;
mod launch;
mod seed;
mod send;
mod state;
mod url;

use std::time::Duration;

use serde_json::Value;

use crate::seams::{Loopback, Processes, Time};

pub use create::{create_session, Serve, TIMEOUT_MS};
pub(super) use launch::launch_configuration;
pub use launch::Launched;
pub use seed::{seed_session, Seed, LIFETIME_MS};
pub use send::{send, Target};
pub(super) use state::{session_state, Shown};

// For the binary that plays the channel's cases (`test-support`).
#[cfg(feature = "test-support")]
pub use crate::shared::admission::Sent;

/// A launch's channel: its window's server, and the plugin's.
#[derive(Debug, Clone)]
pub struct Channel {
    pub launch_id: String,
    /// The window's own server: `http://127.0.0.1:<port>`.
    pub endpoint: String,
    pub password: String,
    pub bridge: Bridge,
}

/// The plugin's server, which the launch's token is the key to.
#[derive(Debug, Clone)]
pub struct Bridge {
    pub endpoint: String,
    pub token: String,
}

/// What the engine gives the channel to wait and ask with.
#[derive(Clone, Copy)]
pub struct Wires<'a> {
    pub time: &'a dyn Time,
    pub loopback: &'a dyn Loopback,
    pub processes: &'a dyn Processes,
}

impl Wires<'_> {
    /// A wait of `millis`.
    async fn sleep(&self, millis: u64) {
        self.time.sleep(Duration::from_millis(millis)).await;
    }
}

/// What a body that went past its size is refused in, whether it is a
/// readiness poll's or a new session's (`readBoundedText`).
const OVERSIZED: &str = "opencode session returned an oversized response";

/// Whether a reply is a success to `fetch` (`response.ok`): a status from
/// 200 to 299.
fn succeeded(status: u16) -> bool {
    (200..300).contains(&status)
}

/// Whether `text` is the id of a conversation, `/^ses_[A-Za-z0-9]+$/`.
fn is_session_id(text: &str) -> bool {
    text.strip_prefix("ses_").is_some_and(|rest| {
        !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

/// `response.json()`: the JSON in a body, a leading byte order mark taken
/// off as the body mixin does, or none where there is none.
fn json_of(body: &[u8]) -> Option<Value> {
    let whole = body.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(body);
    cf_base::json::from_slice_lossy(whole).ok()
}

#[cfg(test)]
mod tests;
