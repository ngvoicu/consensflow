//! What a trace of the API names that differs from one run to the next: the
//! host and port a run of `cf` is told the API is at (`«api»`), and each window's
//! token by the name the trace gave it (`«token:T1»`, in the order they were
//! issued). Only the API has windows and a port a client is told, so only its
//! player reads these names.

use std::collections::HashMap;

/// The names put where the trace uses them.
pub struct Names {
    address: String,
    tokens: HashMap<String, String>,
}

impl Names {
    pub fn new(address: &str) -> Self {
        Self {
            address: address.to_owned(),
            tokens: HashMap::new(),
        }
    }

    /// These names, as a run of `cf` has them: the tokens that were issued, and
    /// `address` where `«api»` is (a run is told the address of the relay that
    /// writes down what it sends, not the API's own).
    pub fn facing(&self, address: &str) -> Self {
        Self {
            address: address.to_owned(),
            tokens: self.tokens.clone(),
        }
    }

    /// Window `name` has `token`.
    pub fn issued(&mut self, name: &str, token: String) {
        self.tokens.insert(name.to_owned(), token);
    }

    /// The token a window was issued, or the text itself for what no
    /// `issue` named (a token the test made up).
    pub fn token(&self, name: &str) -> String {
        self.tokens
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_owned())
    }

    /// `text` with the names put back.
    pub fn put(&self, text: &str) -> String {
        let mut text = text.replace("«api»", &self.address);
        for (name, token) in &self.tokens {
            text = text.replace(&format!("«token:{name}»"), token);
        }
        text
    }
}
