//! What a step that could not finish says: the words a person reads, as the
//! scripts this replaces said them. A problem found in the feeds is not one of
//! these (it is a line the checks return); this is the run itself stopping.

use std::fmt;
use std::io;

/// Why a step stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure(String);

impl Failure {
    /// A failure that says `message`.
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    /// The words.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Failure {}

impl From<io::Error> for Failure {
    /// What could not be written to the console, or read from the disk without
    /// a word of context: the system's own.
    fn from(cause: io::Error) -> Self {
        Self(cause.to_string())
    }
}
