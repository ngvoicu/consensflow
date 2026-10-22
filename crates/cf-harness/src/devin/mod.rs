//! Devin for Terminal: its windows, its hooks, where it keeps its sessions,
//! and what a session's store and its launches' wire logs say of the
//! conversation.

mod adapter;
mod channel;
mod history;
mod install;
pub(crate) mod paths;
mod question_hook;
pub mod record;
mod session_hook;
mod wire;
mod wire_log;

// For the live tests' `tooling` alone, which ships with nothing.
#[cfg(any(test, feature = "test-support"))]
pub(crate) use adapter::{store_took, window_on, wire_readiness};
#[cfg(any(test, feature = "test-support"))]
pub(crate) use install::added_hooks;
pub(crate) use install::{minimum_version, supported_version};

pub use adapter::DevinAdapter;
pub use question_hook::question_hook;
pub use session_hook::session_hook;
