//! Codex's memories folder, where its memory job works.
//!
//! Codex's memory job (consolidation) is a thread of the window's own `codex app-server`,
//! whose working folder is `$CODEX_HOME/memories`, and which is given the window's config:
//! ConsensFlow's role and variables, the board's token among them, and no sandbox (the issue
//! `codex-memories-post-to-the-board-as-the-chief`). No window works from that folder, so a
//! `cf` command whose working folder is inside it is the job's, and changes nothing on the board
//! (`crate::background`). The class fix, the board taking a Codex window's token only from the
//! window's own thread, is native control's (3.0.8).

use std::path::PathBuf;

use cf_base::env::Env;

use super::paths::codex_homes;

/// What `cf` says to a command that changes the board and ran in Codex's memories folder.
pub(crate) const WORDS: &str =
    "This command ran in Codex's memories folder: Codex's memory job isn't on the board.";

/// Codex's memories folders of the environment `env`, as they are named: `$CODEX_HOME/memories`,
/// else `.codex/memories` of the user's home (of each of `HOME` and `USERPROFILE`, where they
/// name two); none where the home is not known.
pub(crate) fn memories_folders(env: &Env) -> Vec<PathBuf> {
    codex_homes(env)
        .into_iter()
        .map(|home| PathBuf::from(home).join("memories"))
        .collect()
}

#[cfg(test)]
mod tests;
