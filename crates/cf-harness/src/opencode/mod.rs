//! OpenCode: its windows, the channel to the server and the plugin inside
//! them, where it keeps its store, and what a session's rows and events in it
//! say of the conversation.

mod adapter;
mod channel;
mod child_env;
mod install;
pub(crate) mod paths;
mod quota;
pub mod record;
mod role;
mod version;

// What the admin judges an OpenCode by: the rule a launch holds to.
pub(crate) use version::{not_supported, unsupported as unsupported_version};

pub use adapter::OpenCodeAdapter;
// What the channel's cases against the system's own clock, loopback and
// processes call (`tests/channels`).
#[cfg(feature = "test-support")]
pub use channel::{
    create_session, real_path, seed_session, send, Bridge, Channel, Launched, Seed, Sent, Serve,
    Target, Wires, LIFETIME_MS, TIMEOUT_MS,
};
pub use install::{prepare_extension, Extension};
