//! Claude Code: its windows, what they run with, its hooks, where it keeps
//! a session's transcript, what the transcript says of the conversation,
//! the turn the daemon stopped before Claude wrote a word of it, and the
//! hooks of ours an older version left in its settings.

mod adapter;
pub(crate) mod install;
pub(crate) mod paths;
mod question_hook;
pub mod record;
mod stale_hooks;
mod status;
mod stopped;

pub use adapter::ClaudeAdapter;
pub use question_hook::question_hook;
pub use stale_hooks::{stale_hooks, StaleHooks};
