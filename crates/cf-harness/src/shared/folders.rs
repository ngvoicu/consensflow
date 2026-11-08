//! Whether a folder is inside another, by the names of their folders: the same on every system.
//!
//! The links of a folder are resolved by whoever asks (`cf_base::file::resolved_path`); what is
//! compared here is two paths as they are written, in any form a system writes one: a Windows
//! path with `/` or `\\`, a drive in any case, a verbatim prefix (`\\?\C:\x`, what
//! `fs::canonicalize` gives there), a network path; a Mac's in any case. Nothing is looked for on
//! disk, so a case holds on every system, whichever it is run on.

use std::path::Path;

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

    pub(crate) fn of_this_system() -> Self {
        if cfg!(windows) {
            Self::WINDOWS
        } else if cfg!(target_os = "macos") {
            Self::MAC
        } else {
            Self::LINUX
        }
    }
}

/// Whether `folder` is `outer` or below it, both as they are written (the links of neither
/// resolved): by the names of their folders, compared as `fold` says.
pub(crate) fn inside(folder: &Path, outer: &Path, fold: Fold) -> bool {
    let outer = components(outer, fold);
    !outer.is_empty() && components(folder, fold).starts_with(&outer)
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
