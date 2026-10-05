//! OpenCode: its windows, the channel to the server and the plugin inside
//! them, where it keeps its store, and what a session's rows and events in it
//! say of the conversation.

mod adapter;
mod channel;
mod child_env;
mod install;
mod paths;
mod quota;
pub mod record;
mod role;

pub use adapter::OpenCodeAdapter;
// What `opencode-channel` plays the channel's cases through
// (`src/bin/opencode_channel.rs`).
#[cfg(feature = "test-support")]
pub use channel::{
    create_session, seed_session, send, Bridge, Channel, Launched, Seed, Sent, Serve, Target,
    Wires, LIFETIME_MS, TIMEOUT_MS,
};
pub use install::{prepare_extension, Extension};
