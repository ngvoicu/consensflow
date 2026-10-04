//! A refusal: what a ConsensFlow component says when it will not do what it
//! was asked, as data. Its code is stable for programs to match, its message
//! is the sentence a person or an agent reads (ported verbatim from the Node
//! code that said it), and its status is the HTTP status the API answers it
//! with.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub status: u16,
    pub message: String,
}

impl Refusal {
    /// A refusal of a request that was wrong: status 400.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self::with_status(code, message, 400)
    }

    pub fn with_status(code: &'static str, message: impl Into<String>, status: u16) -> Self {
        Self {
            code,
            status,
            message: message.into(),
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Refusal {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_its_message_and_keeps_its_code_and_status() {
        let locked = Refusal::with_status("ledger-locked", "another ConsensFlow has x open", 409);
        assert_eq!(locked.to_string(), "another ConsensFlow has x open");
        assert_eq!((locked.code, locked.status), ("ledger-locked", 409));
        assert_eq!(
            Refusal::new("invalid-text", "body must be text").status,
            400
        );
    }
}
