//! Claude Code: its windows, what they run with, its hooks, where it keeps
//! a session's transcript, what the transcript says of the conversation,
//! the turn the daemon stopped before Claude wrote a word of it, and the
//! hooks of ours an older version left in its settings.

mod adapter;
pub mod computer_use;
pub(crate) mod computer_use_prompt;
pub(crate) mod declared;
pub(crate) mod dialog_identity;
pub(crate) mod input_box;
pub(crate) mod install;
mod last_used;
pub(crate) mod models;
pub(crate) mod paths;
mod question_hook;
pub mod record;
mod stale_hooks;
mod status;
mod stopped;
pub(crate) mod trust;

pub use adapter::ClaudeAdapter;
pub use question_hook::question_hook;
pub use stale_hooks::{stale_hooks, StaleHooks};
pub(crate) use status::Scanner;
pub use status::Status;

use cf_base::env::Env;

/// Whether Claude Code's own statuses say no Claude it runs in the folders `env`
/// names shows the conversation `session`: none writes its transcript. False
/// where they cannot say.
pub(crate) fn shown_by_none(session: &str, env: &Env) -> bool {
    status::folder(env)
        .ok()
        .and_then(|folder| status::holders(&folder, session))
        .is_some_and(|holders| holders.is_empty())
}

/// Claude's statuses read here with no memory ([`Records::claude_statuses`]'s default).
pub(crate) fn scan_statuses(folder: &str) -> Vec<(u32, Status)> {
    status::statuses(folder)
}

/// The holders of `session` read here with no memory ([`Records::claude_holders`]'s default).
pub(crate) fn scan_holders(folder: &str, session: &str) -> Option<Vec<u32>> {
    status::holders(folder, session)
}
