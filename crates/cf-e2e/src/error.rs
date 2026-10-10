//! What goes wrong in the support, as a value: a case returns it and fails with
//! it said. What a command answers is not an error here, whatever its exit
//! code: that is for the case to judge.

use std::path::{Path, PathBuf};
use std::{fmt, io};

/// The result of a step of the support.
pub type Result<T = ()> = std::result::Result<T, Error>;

/// Why a step of the support could not be done.
#[derive(thiserror::Error)]
pub enum Error {
    /// A program could not be started, or its end could not be waited for.
    #[error("could not {action} `{program}`: {source}")]
    Program {
        action: &'static str,
        program: String,
        source: io::Error,
    },
    /// A file or a folder a case makes, reads or lists could not be used.
    #[error("could not {action} {}: {source}", path.display())]
    File {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    /// The `cf` under test could not be built, or the build left none.
    #[error("{0}")]
    Build(String),
    /// What a command printed, which was to be JSON, is not.
    #[error("not JSON ({source}):\n{text}")]
    Json {
        text: String,
        source: serde_json::Error,
    },
    /// A request to a server on this machine got no answer.
    #[error("could not ask {url}: {message}")]
    Http { url: String, message: String },
    /// The daemon under test is not the one asked for, did not start, or did
    /// not answer as a daemon does: what it says, with what the daemon said.
    #[error("{0}")]
    Daemon(String),
    /// Something a case waited for did not happen in time: what, and what the
    /// programs said meanwhile.
    #[error("{0}")]
    Timeout(String),
}

/// A case that returns an error fails with what the test harness prints of it
/// by `Debug`: that is what it says, on the lines it says it, and not the
/// variant's name and the text escaped into one.
impl fmt::Debug for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl Error {
    /// Turns the failure of `action` on `path` into an error that names both.
    pub(crate) fn file(action: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Self {
        let path = path.to_path_buf();
        move |source| Self::File {
            action,
            path,
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_case_that_fails_with_an_error_prints_what_it_says_on_the_lines_it_says_it() {
        let error =
            Error::Timeout("the daemon never did it:\n  its log:\n    a \"line\"".to_owned());
        assert_eq!(format!("{error:?}"), error.to_string());
        assert_eq!(
            format!("Error: {error:?}"),
            "Error: the daemon never did it:\n  its log:\n    a \"line\""
        );
    }

    #[test]
    fn a_file_that_cannot_be_used_is_named_with_what_was_done_to_it() {
        let error = Error::file("read", Path::new("a/b.json"))(io::ErrorKind::NotFound.into());
        assert_eq!(
            error.to_string(),
            "could not read a/b.json: entity not found"
        );
    }
}
