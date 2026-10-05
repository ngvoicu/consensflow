//! What the Node tests matched with a pattern (`assert.match`), matched
//! with the same one.

use regex::Regex;

/// Whether `pattern` (a JavaScript pattern written as a Rust one) matches `text`.
pub fn found(pattern: &str, text: &str) -> bool {
    Regex::new(pattern).unwrap().is_match(text)
}
