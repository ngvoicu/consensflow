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
    /// a file (`RmSync`, `src/node_file.cc` of Node 26.7.0, the Node the app
    /// bundles): `<code>, <words> '<path>'`, its words a sentence of four it
    /// knows, or `Unknown error` and the system's own with no code at all.
    Removal {
        code: &'static str,
        words: String,
        path: String,
    },
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
        let path = namespaced(path);
        let (code, words) = match refused(&source) {
            Some((code, sentence)) => (code, format!("{sentence}: {path}")),
            None => ("", format!("Unknown error: {}", system_words(&source))),
        };
        Self {
            source,
            said: Said::Removal { code, words, path },
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
            Said::Removal { code, words, path } => write!(f, "{code}, {words} '{path}'"),
        }
    }
}

impl Error for FileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// The code and the sentence `rmSync`'s C++ says a refusal of the removal
/// in, for the four it knows, as the C++ library classes the system's
/// error (`std::errc`); none for any other. On Unix the class is the errno.
#[cfg(unix)]
fn refused(error: &io::Error) -> Option<(&'static str, &'static str)> {
    Some(match error.raw_os_error()? {
        libc::EPERM => ("EPERM", "Operation not permitted"),
        libc::ENOTEMPTY => ("ENOTEMPTY", "Directory not empty"),
        libc::ENOTDIR => ("ENOTDIR", "Not a directory"),
        libc::EACCES => ("EACCES", "Permission denied"),
        _ => return None,
    })
}

/// The code and the sentence of a refused removal on Windows. The Microsoft
/// C++ library maps a Win32 code to its `std::errc` (`stl/src/syserror.cpp`),
/// and none maps to `operation_not_permitted` or `not_a_directory`. Node
/// says EPERM where it was denied there.
#[cfg(windows)]
fn refused(error: &io::Error) -> Option<(&'static str, &'static str)> {
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_CANNOT_MAKE, ERROR_CURRENT_DIRECTORY, ERROR_DIR_NOT_EMPTY,
        ERROR_INVALID_ACCESS, ERROR_NOACCESS, ERROR_SHARING_VIOLATION, ERROR_WRITE_PROTECT,
    };
    Some(match u32::try_from(error.raw_os_error()?).ok()? {
        ERROR_ACCESS_DENIED
        | ERROR_INVALID_ACCESS
        | ERROR_CURRENT_DIRECTORY
        | ERROR_WRITE_PROTECT
        | ERROR_SHARING_VIOLATION
        | ERROR_CANNOT_MAKE
        | ERROR_NOACCESS => ("EPERM", "Permission denied"),
        ERROR_DIR_NOT_EMPTY => ("ENOTEMPTY", "Directory not empty"),
        _ => return None,
    })
}

/// `path` as `rmSync`'s C++ names it: as it is on Unix; on Windows made
/// whole (`ToNamespacedPath`): a drive's path after `\\?\`, a share's
/// after `\\?\UNC\`.
fn namespaced(path: &Path) -> String {
    #[cfg(windows)]
    {
        let whole = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let whole = whole.to_string_lossy();
        let bytes = whole.as_bytes();
        if let Some(share) = whole.strip_prefix(r"\\") {
            if !share.starts_with(['?', '.']) {
                return format!(r"\\?\UNC\{share}");
            }
        } else if bytes.len() > 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\'
        {
            return format!(r"\\?\{whole}");
        }
        whole.into_owned()
    }
    #[cfg(not(windows))]
    {
        path.display().to_string()
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
