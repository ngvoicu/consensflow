//! Hiding what looks like a key or a token in text a window showed. A screen
//! can hold one: an error that echoes the key it was given, a header, a
//! pasted environment. What this reads is its shape, not its meaning, so it
//! hides some that are no secret (a long hash, an id with digits in it) and
//! leaves what it does not know the shape of; the quote it protects is cut
//! short on top of this, and says that it hid something.
//!
//! What is hidden, each by `[masked]` in its place:
//! - the user and password of a URL (`https://user:password@host`);
//! - an email address (`ci-bot@example.com`), whose account it names;
//! - the user and password of a command's `-u user:password` (curl's `-u`, `--user`,
//!   `--proxy-user`);
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
//!   base64's own (letters, digits, `+`, `/`, `=`) with the same; and a base64 value with
//!   no digit: forty letters and digits or more with a capital in them, or a run of that
//!   length with a `+` in it or `=` at its end.
//!
//! **The spans of every rule are found in the text as it was**, joined where they overlap
//! or touch, and replaced once (T-823): no part of a value is left to a rule that comes
//! after the one that took the first part of it.

use std::sync::LazyLock;

use regex::Regex;

/// What a hidden secret is replaced with.
pub const MASK: &str = "[masked]";

/// A pattern, which group of it is the secret (0 for all of it), and whether
/// what that group matched is one.
struct Rule {
    pattern: Regex,
    secret: usize,
    is_secret: fn(&str) -> bool,
    /// The rule gives way where a URL's user and password hold its match: an address that
    /// begins inside them (`user:name@host`) is the URL's, and its host stays.
    yields_to_url: bool,
}

/// The patterns are constants of this file, and a test builds each: a mistake
/// in one fails that test, and no input reaches it.
#[allow(clippy::expect_used)]
fn rule(pattern: &str, secret: usize, is_secret: fn(&str) -> bool) -> Rule {
    Rule {
        pattern: Regex::new(pattern).expect("a pattern of the masker"),
        secret,
        is_secret,
        yields_to_url: false,
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
        // An email address: whose account it is.
        Rule {
            yields_to_url: true,
            ..rule(
                r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}\b",
                0,
                always,
            )
        },
        // curl's `-u user:password`, `--user`, `--proxy-user` (and `-U`), on a command a screen echoes.
        rule(
            r"(?:^|\s)(?:-[uU]|--user|--proxy-user)(?:\s+|=)?([^\s:]+:\S+)",
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
        // A base64 value with no digit in it (`YWJj` twelve times): forty letters or more in a
        // run, in both cases or in capitals alone, is no word, and a long name in a case of
        // its own or a number is left (T-823, the review of `a26e0848e`).
        rule(r"[A-Za-z0-9]{40,}={0,2}", 0, is_base64_letters),
        // One with a `+`, or padded: no path and no word has them.
        rule(r"[A-Za-z0-9+/]{39,}={0,2}", 0, is_base64_marked),
    ]
});

/// A run of forty letters and digits or more that is no word: it has a capital in it.
/// All small letters is a word, and digits alone a number.
fn is_base64_letters(found: &str) -> bool {
    let run = found.trim_end_matches('=');
    run.chars().any(|c| c.is_ascii_uppercase())
        && (run.chars().any(|c| c.is_ascii_lowercase()) || run.len() >= 40)
}

/// A base64 run that says what it is by its characters: a `+` in it, or `=` at its end.
fn is_base64_marked(found: &str) -> bool {
    found.contains('+') || found.ends_with('=')
}

/// A text, and how many secrets were hidden in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Masked {
    pub text: String,
    pub count: usize,
}

/// `text` with what looks like a key or a token hidden. Every rule looks at the text as it
/// was, and the spans they find are joined where they overlap or touch before any is
/// replaced: a rule that took a piece of a value first left the rest to a rule that saw
/// a shorter run (`[masked]+/abcdefghijklmnopqr/AA==`, the review of `a26e0848e`). A secret
/// found by two rules is one.
pub fn mask(text: &str) -> Masked {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut users: Vec<(usize, usize)> = Vec::new();
    for (at, rule) in RULES.iter().enumerate() {
        for found in rule.pattern.captures_iter(text) {
            let Some(secret) = found.get(rule.secret) else {
                continue;
            };
            let span = (secret.start(), secret.end());
            if !(rule.is_secret)(secret.as_str()) {
                continue;
            }
            // The first rule is the URL's user and password.
            if at == 0 {
                users.push(span);
            }
            let whole = found
                .get(0)
                .map_or(span, |whole| (whole.start(), whole.end()));
            if rule.yields_to_url
                && users
                    .iter()
                    .any(|user| whole.0 < user.1 && user.0 < whole.1)
            {
                continue;
            }
            spans.push(span);
        }
    }
    spans.sort_unstable();
    let mut joined: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        match joined.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => joined.push((start, end)),
        }
    }
    let mut hidden = String::with_capacity(text.len());
    let mut at = 0;
    for &(start, end) in &joined {
        hidden.push_str(&text[at..start]);
        hidden.push_str(MASK);
        at = end;
    }
    hidden.push_str(&text[at..]);
    Masked {
        text: hidden,
        count: joined.len(),
    }
}

#[cfg(test)]
mod tests;
