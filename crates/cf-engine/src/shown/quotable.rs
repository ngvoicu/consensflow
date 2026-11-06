//! The words of a harness's program, as they may be quoted outside a screen.
//!
//! A harness's own error can echo what it was given: Pi's error stream quotes an extension's
//! failure, which can carry the header it was sent, and a read that fails says what the program
//! said. Those words are kept (the ledger's reason a model list is stale, which the page reads)
//! and told (the note of a model that is gone). **A line that may hold a credential is withheld
//! whole, and no part of it is quoted** (the review T-1270, second reading, F2): masking a value
//! left the rest of a key with a blank in it (`API_KEY: sk-abcd efghijkl mnopqrst`) and the words
//! of a password after its first (`password: correct horse battery staple`), and no masker
//! holds every syntax a secret is written in (the screens' reviews found one gap after another,
//! [`super::screen`]).
//!
//! The rule is the screens' own, one line at a time, before the line is quoted or cut
//! ([`screen::withheld`]: a secret-like word, a token's beginning, a key's body, a value in
//! groups), and a credential's word with a value after it (`key: abc`, `token=…`, `"secret":
//! "…"`, `Authorization: …`, `bearer …`, in any case): a bare `key` is no secret in a sentence
//! ("key not found"), but `key:` or `key=` followed by something holds what it names. What a
//! line that is quoted shows is masked as a note masks it ([`mask`]).
//!
//! The cost is the lines that name a credential and hold none ("Invalid key: expected string"):
//! they are left out, as a screen that names one is. The words of a harness that name nothing
//! of the kind come out as they went in.

use std::sync::LazyLock;

use regex::Regex;

use super::mask::mask;
use super::screen;

/// What a line [`quotable`] withholds is replaced with.
pub const WITHHELD_LINE: &str = "(a line withheld: it may hold a credential)";

/// A credential's word, the delimiter that gives it a value, and the value's first character:
/// `key: abc`, `"token" = "a b"`, `API_KEY=…`, `Authorization: …`. A name that has the word
/// anywhere in it counts (`ssh_key`, `x-api-key`), and so does one that has it as a piece of
/// another word (`monkey: 1`): a false alarm costs a line, a miss a credential.
#[allow(clippy::expect_used)]
static NAMED_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:key|token|secret|passw(?:or)?d|authorization|bearer)s?["']?\s*[:=]\s*["']?[^\s"']"#,
    )
    .expect("a pattern")
});

/// Whether `line` may hold a credential.
fn may_hold_a_credential(line: &str) -> bool {
    screen::withheld(&[line.to_owned()]).is_some() || NAMED_VALUE.is_match(line)
}

/// `words` with each line that may hold a credential replaced by [`WITHHELD_LINE`], and the
/// others masked ([`mask`]), in the order they came, joined with `\n`.
pub fn quotable(words: &str) -> String {
    words
        .lines()
        .map(|line| {
            if may_hold_a_credential(line) {
                WITHHELD_LINE.to_owned()
            } else {
                mask(line).text
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
