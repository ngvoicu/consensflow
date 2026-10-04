//! Claude Code: its hooks, where it keeps a session's transcript, and what
//! the transcript says of the conversation.

mod paths;
mod question_hook;
pub mod record;

pub use question_hook::question_hook;
