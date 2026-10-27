//! Claude Code: its windows, what they run with, its hooks, where it keeps
//! a session's transcript, what the transcript says of the conversation,
//! the turn the daemon stopped before Claude wrote a word of it, and the
//! hooks of ours an older version left in its settings.

mod adapter;
pub mod computer_use;
pub(crate) mod input_box;
pub(crate) mod install;
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
