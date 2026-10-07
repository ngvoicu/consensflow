//! Hiding what looks like a key or a token in text a window showed. A screen
//! can hold one: an error that echoes the key it was given, a header, a
//! pasted environment. What this reads is its shape, not its meaning, so it
//! hides some that are no secret (a long hash, an id with digits in it) and
//! leaves what it does not know the shape of; the quote it protects is cut
//! short on top of this, and says that it hid something.
//!
//! What is hidden, each by `[masked]` in its place:
//! - the user and password of a URL (`https://user:password@host`);
//! - what follows `Bearer`, eight characters or more of a token; and what
//!   follows `Basic` when it is base64's (a digit in it, a capital that does
//!   not begin it, or ending in `=`), for `basic setup` is no credential;
//! - the value of a name that says it is secret, `=` or `:` between them
//!   (`DB_PASSWORD=…`, `"secret": "…"`): after a name with `password`,
//!   `passwd`, `pwd`, `secret` or `credential` in it, six characters or more
//!   with no space; after a name with `key` or `token` in it, which are
//!   words too (`token: expired`), a value of eight characters or more with a
//!   digit in it, or sixteen or more;
//! - a token of a kind with a known prefix (`sk-`, `sk_live_`, `ghp_` and its
//!   kin, `github_pat_`, `glpat-`, `xoxb-`, `AKIA…`, `AIza…`, `npm_`, `hf_`,
//!   `ya29.`, `SG.`, a JWT's `eyJ…`);
//! - any run of 24 letters, digits, `_` and `-` with at least one letter and one
//!   digit in it (a hex or base64url token), and any run of 40 characters of
//!   base64's own (letters, digits, `+`, `/`, `=`) with the same.

use std::sync::LazyLock;

use regex::{Captures, Regex};

/// What a hidden secret is replaced with.
pub const MASK: &str = "[masked]";

/// A pattern, which group of it is the secret (0 for all of it), and whether
/// what that group matched is one.
struct Rule {
    pattern: Regex,
    secret: usize,
    is_secret: fn(&str) -> bool,
}

/// The patterns are constants of this file, and a test builds each: a mistake
/// in one fails that test, and no input reaches it.
#[allow(clippy::expect_used)]
fn rule(pattern: &str, secret: usize, is_secret: fn(&str) -> bool) -> Rule {
    Rule {
        pattern: Regex::new(pattern).expect("a pattern of the masker"),
        secret,
        is_secret,
    }
}

fn always(_: &str) -> bool {
    true
}

fn has_a_letter_and_a_digit(found: &str) -> bool {
    found.chars().any(|c| c.is_ascii_alphabetic()) && found.chars().any(|c| c.is_ascii_digit())
}

/// The value of a name that says it is secret, but not the scheme that comes
/// before the token in a header (`Authorization: Bearer …`), nor what is
/// hidden already.
fn a_value(found: &str) -> bool {
    !found.starts_with(MASK)
        && !found.eq_ignore_ascii_case("bearer")
        && !found.eq_ignore_ascii_case("basic")
}

/// What `Basic` carries: base64 of `user:password`, which is not a word: it
/// has a digit in it, a capital that does not begin it, or ends in `=`.
fn base64_credentials(found: &str) -> bool {
    has_a_letter_and_a_digit(found)
        || found.ends_with('=')
        || found.chars().skip(1).any(|c| c.is_ascii_uppercase())
}

/// The value of a name that is a word as well (`key`, `token`): one that is
/// long, or has a digit in it, not the word that says why it failed.
fn a_long_or_numbered_value(found: &str) -> bool {
    a_value(found)
        && (found.chars().count() >= 16
            || (found.chars().count() >= 8 && found.chars().any(|c| c.is_ascii_digit())))
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        // scheme://user:password@host
        rule(
            r"(?i)\b[a-z][a-z0-9+.-]*://([^\s/@:]+:[^\s/@]+)@",
            1,
            always,
        ),
        // Authorization: Bearer <token>, Authorization: Basic <credentials>
        rule(r"(?i)\bbearer\s+([A-Za-z0-9._~+/=-]{8,})", 1, always),
        rule(
            r"(?i)\bbasic\s+([A-Za-z0-9._~+/=-]{8,})",
            1,
            base64_credentials,
        ),
        // NAME=value, NAME: value, "name": "value"
        rule(
            r#"(?i)\b[A-Za-z0-9_.-]*(?:passw(?:or)?d|pwd|secret|credential)[A-Za-z0-9_.-]*["']?\s*[:=]\s*["']?([^\s"',;]{6,})"#,
            1,
            a_value,
        ),
        rule(
            r#"(?i)\b[A-Za-z0-9_.-]*(?:key|token)[A-Za-z0-9_.-]*["']?\s*[:=]\s*["']?([^\s"',;]{8,})"#,
            1,
            a_long_or_numbered_value,
        ),
        // Tokens that say what they are by how they begin.
        rule(
            concat!(
                r"\b(?:(?:sk|pk|rk)[-_][A-Za-z0-9_-]{12,}",
                r"|gh[pousr]_[A-Za-z0-9]{20,}",
                r"|github_pat_[A-Za-z0-9_]{20,}",
                r"|glpat-[A-Za-z0-9_-]{16,}",
                r"|xox[abprs]-[A-Za-z0-9-]{10,}",
                r"|(?:AKIA|ASIA)[A-Z0-9]{16}",
                r"|AIza[A-Za-z0-9_-]{30,}",
                r"|npm_[A-Za-z0-9]{30,}",
                r"|hf_[A-Za-z0-9]{30,}",
                r"|ya29\.[A-Za-z0-9._-]{20,}",
                r"|SG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}",
                r"|eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]*)",
            ),
            0,
            always,
        ),
        // A hex or base64url token with nothing to say what it is.
        rule(r"[A-Za-z0-9_-]{24,}", 0, has_a_letter_and_a_digit),
        // A base64 token, whose own characters include the path's.
        rule(r"[A-Za-z0-9+/=]{40,}", 0, has_a_letter_and_a_digit),
    ]
});

/// A text, and how many secrets were hidden in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Masked {
    pub text: String,
    pub count: usize,
}

/// `text` with what looks like a key or a token hidden.
pub fn mask(text: &str) -> Masked {
    let mut count = 0;
    let mut text = text.to_owned();
    for rule in RULES.iter() {
        text = rule
            .pattern
            .replace_all(&text, |found: &Captures<'_>| hide(rule, found, &mut count))
            .into_owned();
    }
    Masked { text, count }
}

/// What replaces a match of `rule`: the match with its secret hidden, or the
/// match as it was where it holds none.
fn hide(rule: &Rule, found: &Captures<'_>, count: &mut usize) -> String {
    let whole = &found[0];
    let (Some(all), Some(secret)) = (found.get(0), found.get(rule.secret)) else {
        return whole.to_owned();
    };
    if !(rule.is_secret)(secret.as_str()) {
        return whole.to_owned();
    }
    *count += 1;
    // The group lies inside the match.
    let (before, after) = (secret.start() - all.start(), secret.end() - all.start());
    format!("{}{MASK}{}", &whole[..before], &whole[after..])
}

#[cfg(test)]
mod tests;
