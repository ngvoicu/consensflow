//! Codex's memories folder, and whether a command runs inside it.
//!
//! Codex's memory job (consolidation) is a thread of the window's own `codex app-server`,
//! whose working folder is `$CODEX_HOME/memories`, and which is given the window's config:
//! ConsensFlow's role and variables, the board's token among them (the issue
//! `codex-memories-post-to-the-board-as-the-chief`). No window works from that folder, so a
//! `cf` command whose working folder is inside it is the job's: `cf` changes nothing on the
//! board there ([`holds`]). The class fix is the board taking a Codex window's token only from
//! the window's own thread (native control, 3.0.8).

use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::resolved_path;

use super::paths::codex_home_or_default;

/// Whether `folder` is Codex's memories folder of the environment `env`, or inside it: its
/// links resolved, on the system this is built for. The memories folder is
/// `$CODEX_HOME/memories`, and `~/.codex/memories` where `CODEX_HOME` is unset or empty; none
/// where neither it nor the home is known, and then nothing is inside it.
pub fn holds(env: &Env, folder: &Path) -> bool {
    holds_on(env, folder, Fold::of_this_system())
}

/// How the system compares two names of folders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fold {
    /// Whether a drive's and a folder's name are the same in any case (the default volumes of
    /// Windows and the Mac).
    ignore_case: bool,
    /// Whether a path is written with `\` and `/` both, may begin with a drive or a server, and
    /// may be a verbatim one (`\\?\C:\…`, what `fs::canonicalize` gives).
    windows: bool,
}

impl Fold {
    pub(crate) const WINDOWS: Self = Self {
        ignore_case: true,
        windows: true,
    };
    pub(crate) const MAC: Self = Self {
        ignore_case: true,
        windows: false,
    };
    pub(crate) const LINUX: Self = Self {
        ignore_case: false,
        windows: false,
    };

    fn of_this_system() -> Self {
        if cfg!(windows) {
            Self::WINDOWS
        } else if cfg!(target_os = "macos") {
            Self::MAC
        } else {
            Self::LINUX
        }
    }
}

/// [`holds`], with the names compared as `fold` says.
pub(crate) fn holds_on(env: &Env, folder: &Path, fold: Fold) -> bool {
    memories_folder(env)
        .is_some_and(|memories| inside(&resolved_path(folder), &resolved_path(&memories), fold))
}

/// Codex's memories folder of the environment `env`, as it is named: `$CODEX_HOME/memories`,
/// `~/.codex/memories` where `CODEX_HOME` is unset or empty, none where the home is not known
/// either.
pub(crate) fn memories_folder(env: &Env) -> Option<PathBuf> {
    codex_home_or_default(env)
        .ok()
        .map(|home| PathBuf::from(home).join("memories"))
}

/// Whether `folder` is `memories` or below it, both as they are written (the links of neither
/// resolved): by the names of their folders, compared as `fold` says.
pub(crate) fn inside(folder: &Path, memories: &Path, fold: Fold) -> bool {
    let memories = components(memories, fold);
    !memories.is_empty() && components(folder, fold).starts_with(&memories)
}

/// The names of the folders of `path`, in order, as they are compared: the verbatim prefix of
/// a Windows path gone (`\\?\C:\x` is `C:\x`, and `\\?\UNC\server\share\x` is
/// `\\server\share\x`), `/` and `\` both separators there, `.` and an empty name dropped, a
/// `..` taking the name before it, and every name lower case where the volume ignores case.
/// Its root is the first: `/` on a Mac, the drive or the `\\server` on Windows.
fn components(path: &Path, fold: Fold) -> Vec<String> {
    let text = path.to_string_lossy();
    let text = if fold.windows {
        without_verbatim_prefix(text.replace('/', "\\"))
    } else {
        text.into_owned()
    };
    let separator = if fold.windows { '\\' } else { '/' };
    // What a `..` cannot go above: the root of a Mac's path, the drive, the server and share of
    // a network path.
    let network = fold.windows && text.starts_with(r"\\");
    let drive = fold.windows
        && text.split_once(':').is_some_and(|(drive, _)| {
            drive.len() == 1 && drive.chars().all(|c| c.is_ascii_alphabetic())
        });
    let rooted = !fold.windows && text.starts_with('/');
    let floor = if network {
        2
    } else {
        usize::from(rooted || drive)
    };
    let mut names: Vec<String> = Vec::new();
    if rooted {
        names.push("/".to_owned());
    }
    for name in text.split(separator) {
        match name {
            "" | "." => {}
            ".." => {
                // The root has no parent: `/..` is `/`.
                if names.len() > floor {
                    names.pop();
                }
            }
            other => names.push(if fold.ignore_case {
                other.to_lowercase()
            } else {
                other.to_owned()
            }),
        }
    }
    if network && names.len() >= 2 {
        let share = names.remove(1);
        names[0] = format!(r"\\{}\{share}", names[0]);
    }
    names
}

/// `text`, a Windows path written with `\`, without its verbatim prefix: `\\?\C:\x` is `C:\x`
/// and `\\?\UNC\server\share\x` is `\\server\share\x`. What `fs::canonicalize` gives there.
fn without_verbatim_prefix(text: String) -> String {
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_owned(),
        None => text,
    }
}

#[cfg(test)]
mod tests;
