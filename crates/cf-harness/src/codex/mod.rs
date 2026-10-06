//! Codex: its windows, the channel to the broker of the supervisor they run
//! under, where it keeps a thread's rollout, what the rollout says of the
//! thread, and the quota it reports ahead of time.

mod adapter;
mod channel;
#[cfg(test)]
mod fakes;
mod launch;
pub(crate) mod paths;
mod quota;
pub mod record;
mod role;

pub use adapter::CodexAdapter;
// What `codex-send` plays the channel's cases through (`src/bin/codex_send.rs`).
#[cfg(feature = "test-support")]
pub use channel::{send, Answer, Channel, Session, Shown, Target};
