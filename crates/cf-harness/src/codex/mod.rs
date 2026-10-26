//! Codex: its windows, the channel to the broker of the supervisor they run
//! under, where it keeps a thread's rollout, what the rollout says of the
//! thread, the quota it reports ahead of time, and whether its computer use is
//! set up.

mod adapter;
mod channel;
pub mod computer_use;
#[cfg(test)]
mod fakes;
mod launch;
pub(crate) mod paths;
mod quota;
pub mod record;
mod role;

pub use adapter::CodexAdapter;
// What the channel's cases against the system's own clock and loopback call
// (`tests/channels/codex.rs`).
#[cfg(feature = "test-support")]
pub use channel::{send, Answer, Channel, Session, Shown, Target};
