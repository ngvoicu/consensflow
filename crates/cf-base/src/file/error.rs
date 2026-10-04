//! A failed file operation said as Node's error says it: `error.message`
//! word for word, and `error.code`, as the API answers `{error:
//! cause.message}` and the page shows that text.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use super::errno::{error_code, words_of};

/// A file operation that failed, with what Node's error said of it. Display
/// is Node's `message`; [`FileError::code`] is Node's `error.code`; the
/// failure the system gave is the [`source`](Error::source).
#[derive(Debug)]
pub struct FileError {
    source: io::Error,
    said: Said,
}

/// The three shapes of message Node's file calls have.
#[derive(Debug)]
enum Said {
    /// `uvException`, which every call of libuv's that fails is said by:
    /// `EACCES: permission denied, open '/path'`, and ` -> '/dest'` after the
    /// path when the call has a second one. A failure libuv has no name for
    /// is `UNKNOWN: unknown error` (`uvUnmappedError`). No golden reaches
    /// that one: none of the situations they play makes the system fail with
    /// an errno libuv does not name.
    Call {
        code: &'static str,
        syscall: &'static str,
        path: Option<PathBuf>,
        dest: Option<PathBuf>,
    },
    /// `ERR_FS_EISDIR`, a `SystemError` that `rmSync` throws itself when the
    /// path is a directory and it was not asked to remove one: no system
    /// call failed.
    Directory { path: PathBuf },
    /// What the C++ behind `rmSync` throws when the system refuses to remove
    /// what is there: the code, the system's own words (`strerror`:
    /// `Permission denied`) and the path twice. Node names the code from its
    /// own table of the system's errnos, which may hold names libuv's does
    /// not (`ESTALE`); those are said here as `UNKNOWN`. Probed on macOS
    /// only; Linux and Windows may word it otherwise.
    Removal { code: &'static str, path: PathBuf },
}

impl FileError {
    /// A call of libuv's that failed with `source`, as `uvException` says it:
    /// `syscall` is what Node calls the call and `path` what it was given,
    /// none for the one that is given no path (`write`).
    pub(super) fn call(source: io::Error, syscall: &'static str, path: Option<&Path>) -> Self {
        let code = error_code(&source);
        Self::named(code, source, syscall, path, None)
    }

    /// A call that failed on a path and was to put it at another: `rename`.
    pub(super) fn call_to(
        source: io::Error,
        syscall: &'static str,
        path: &Path,
        dest: &Path,
    ) -> Self {
        let code = error_code(&source);
        Self::named(code, source, syscall, Some(path), Some(dest))
    }

    /// A failure Node says by a name of its own, where the system gave
    /// another: its recursive `mkdir` says `ENOTDIR` for a file in the way of
    /// the path, where the system says that the file exists.
    pub(super) fn named(
        code: &'static str,
        source: io::Error,
        syscall: &'static str,
        path: Option<&Path>,
        dest: Option<&Path>,
    ) -> Self {
        Self {
            source,
            said: Said::Call {
                code,
                syscall,
                path: path.map(Path::to_path_buf),
                dest: dest.map(Path::to_path_buf),
            },
        }
    }

    /// `rmSync` found a directory at `path` and will not remove it.
    pub(super) fn directory(path: &Path) -> Self {
        Self {
            source: io::Error::from(io::ErrorKind::IsADirectory),
            said: Said::Directory {
                path: path.to_path_buf(),
            },
        }
    }

    /// The system refused `rmSync` the removal of what is at `path`.
    pub(super) fn removal(source: io::Error, path: &Path) -> Self {
        let code = error_code(&source);
        Self {
            source,
            said: Said::Removal {
                code,
                path: path.to_path_buf(),
            },
        }
    }

    /// Node's `error.code`: `EACCES`, `ERR_FS_EISDIR`, or `UNKNOWN`.
    pub fn code(&self) -> &str {
        match &self.said {
            Said::Call { code, .. } | Said::Removal { code, .. } => code,
            Said::Directory { .. } => "ERR_FS_EISDIR",
        }
    }
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.said {
            Said::Call {
                code,
                syscall,
                path,
                dest,
            } => {
                write!(f, "{code}: {}, {syscall}", words_of(code))?;
                if let Some(path) = path {
                    write!(f, " '{}'", path.display())?;
                }
                if let Some(dest) = dest {
                    write!(f, " -> '{}'", dest.display())?;
                }
                Ok(())
            }
            Said::Directory { path } => write!(
                f,
                "Path is a directory: rm returned EISDIR (is a directory) {}",
                path.display()
            ),
            Said::Removal { code, path } => {
                let path = path.display();
                write!(f, "{code}, {}: {path} '{path}'", system_words(&self.source))
            }
        }
    }
}

impl Error for FileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// What the system says of the failure (`strerror`): Rust's text for it
/// without the code it adds after, `Permission denied (os error 13)`.
fn system_words(error: &io::Error) -> String {
    let said = error.to_string();
    error
        .raw_os_error()
        .and_then(|code| said.strip_suffix(&format!(" (os error {code})")))
        .map_or_else(|| said.clone(), str::to_owned)
}

#[cfg(test)]
mod tests;
