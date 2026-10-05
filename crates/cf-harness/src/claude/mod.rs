//! Claude Code: its windows, what they run with, its hooks, where it keeps
//! a session's transcript, and what the transcript says of the
//! conversation.

mod adapter;
mod install;
pub(crate) mod paths;
mod question_hook;
pub mod record;
mod status;

pub use adapter::ClaudeAdapter;
pub use question_hook::question_hook;
