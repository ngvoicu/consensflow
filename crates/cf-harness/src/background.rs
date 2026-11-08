//! Work a harness runs in the background of its windows, with a window's config.
//!
//! Codex's memory job is one: a thread of the window's own server, in a folder of its own,
//! given the window's role and variables (the board's token among them) and no sandbox. It
//! posted a task as the chief (the issue `codex-memories-post-to-the-board-as-the-chief`). Which
//! harness has such work, and the folder it works in, is the registry's to say
//! (`crate::launch::background_work`); a `cf` command whose working folder is inside one is
//! that work's, not a window's, and changes nothing on the board ([`refusal`]). No window works
//! from there: every window opens in its project's folder (`cf-engine`'s `workspace`).

use std::path::Path;

use cf_base::env::Env;
use cf_base::file::resolved_path;
use cf_proto::agents::Harness;

use crate::launch::background_work;
use crate::shared::folders::{inside, Fold};

/// Why a `cf` command run with `folder` as its working folder changes nothing on the board, in
/// the words it is told in: the folder is one a harness runs background work in, or inside one,
/// its links resolved. None for any other folder.
pub fn refusal(env: &Env, folder: &Path) -> Option<&'static str> {
    refusal_on(env, folder, Fold::of_this_system())
}

/// [`refusal`], with the names compared as `fold` says.
pub(crate) fn refusal_on(env: &Env, folder: &Path, fold: Fold) -> Option<&'static str> {
    let real = resolved_path(folder);
    Harness::ALL.into_iter().find_map(|harness| {
        let (folders, words) = background_work(harness, env)?;
        folders
            .iter()
            .any(|work| inside(&real, &resolved_path(work), fold))
            .then_some(words)
    })
}

#[cfg(test)]
mod tests;
