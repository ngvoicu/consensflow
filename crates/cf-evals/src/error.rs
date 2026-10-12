//! What goes wrong in the evals, as a value.

use std::io;
use std::path::PathBuf;

/// The result of a step of the evals.
pub type Result<T = ()> = std::result::Result<T, Error>;

/// Why a step of the evals could not be done.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file or a folder could not be read.
    #[error("could not {action} {}: {source}", path.display())]
    File {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    /// SQLite refused a ledger file, or a statement run on it.
    #[error(transparent)]
    Ledger(#[from] rusqlite::Error),
    /// What was asked cannot be: a ledger of a newer ConsensFlow, migrations that
    /// are not numbered in order, rows that leave a reference that does not hold,
    /// a harness or a scenario that is none.
    #[error("{0}")]
    Refused(String),
    /// The words of the command are not ones it takes: the status is 2.
    #[error("{0}")]
    Usage(String),
    /// The support the run is made of (the rig, the machine's harnesses, the
    /// harness code of the product) failed.
    #[error(transparent)]
    Support(#[from] cf_e2e::Error),
    /// What the command had to say could not be written.
    #[error("could not write what it says: {0}")]
    Say(#[from] io::Error),
}

impl Error {
    /// Turns the failure of `action` on `path` into an error that names both.
    pub(crate) fn file(
        action: &'static str,
        path: &std::path::Path,
    ) -> impl FnOnce(io::Error) -> Self {
        let path = path.to_path_buf();
        move |source| Self::File {
            action,
            path,
            source,
        }
    }
}
