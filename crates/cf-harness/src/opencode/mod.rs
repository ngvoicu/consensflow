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
pub use install::{prepare_extension, Extension};
