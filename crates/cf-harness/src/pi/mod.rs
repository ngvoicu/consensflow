//! Pi: its windows, the channel to the extension inside them, where it keeps
//! a session's file, and what the session and the extension's evidence
//! beside it say of the conversation.

mod adapter;
mod channel;
mod install;
pub(crate) mod paths;
pub mod record;

pub use adapter::PiAdapter;
// What the channel's cases against the system's own clock call
// (`tests/channels/contract.rs`), and what `pi-send` plays the Pi extension's
// tests through (`src/bin/pi_send.rs`).
#[cfg(feature = "test-support")]
pub use channel::{send, Answer, Target};
pub use install::{prepare_extension, Extension};
