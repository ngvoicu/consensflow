//! Devin for Terminal: its hooks, where it keeps its sessions, and what a
//! session's store and its launches' wire logs say of the conversation.

mod paths;
mod question_hook;
pub mod record;
mod session_hook;
mod wire;

pub use question_hook::question_hook;
pub use session_hook::session_hook;
