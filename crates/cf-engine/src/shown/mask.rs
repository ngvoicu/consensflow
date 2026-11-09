//! Hiding what looks like a key or a token in text a window showed. A screen
//! can hold one: an error that echoes the key it was given, a header, a
//! pasted environment. What this reads is its shape, not its meaning, so it
//! hides some that are no secret (a long hash, an id with digits in it) and
//! leaves what it does not know the shape of; the quote it protects is cut
//! short on top of this, and says that it hid something.
//!
//! What is hidden, each by `[masked]` in its place:
//! - the user and password of a URL (`https://user:password@host`), and the user of one with no
//!   password (`https://TOKEN@host`) unless its scheme names an account (`ssh://git@host`);
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
//!   no digit: forty letters and digits or more with a letter in them, in any case, or a
//!   run of that length with a `+` in it or `=` at its end.
//!
//! **A token's prefix is enough to withhold a screen** ([`begins_a_token`], T-1205 round 4):
//! `sk-abcd efgh …` is a key whose body the rules above never see whole, because the window
//! wrapped it, put blanks in it or decorated it. So a screen that shows the beginning of a token
//! of a known kind, with a few characters of its body or none, is withheld whole, and no guess is
//! made of where the token ends. Four characters of the body, `_` and `-` among them, or up to
//! three and then a blank or the end, count (`sk-abc defg …`, `sk-_ab cd`, `sk-ant-…`).
//!
//! **The spans of every rule are found in the text as it was**, joined where they overlap
//! or touch, and replaced once (T-823): no part of a value is left to a rule that comes
//! after the one that took the first part of it.

use std::sync::LazyLock;

use regex::Regex;

/// What a hidden secret is replaced with.
pub const MASK: &str = "[masked]";

/// The user and password of a URL: `scheme://user:password@host`.
const URL_USER_PASSWORD: &str = r"(?i)\b[a-z][a-z0-9+.-]*://([^\s/@:]+:[^\s/@]+)@";

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
        rule(URL_USER_PASSWORD, 1, always),
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
        // A base64 value with no digit in it (`YWJj` twelve times): forty letters and digits or
        // more in a run, in any case, is no word and no number (T-823, the reviews of `a26e0848e`
        // and `860010398`).
        rule(r"[A-Za-z0-9]{40,}={0,2}", 0, is_base64_letters),
        // One with a `+`, or padded: no path and no word has them.
        rule(r"[A-Za-z0-9+/]{39,}={0,2}", 0, is_base64_marked),
    ]
});

/// Where a token of a kind with a known prefix begins, with a few characters of its body: the
/// same kinds as the rule above, with the body cut to what is enough to tell the kind from a
/// word. `npm_config_cache` and `hf_hub_download` are names, not tokens: their kinds need twelve
/// characters in a row, and the others four, or three or fewer and then a blank or the end of
/// the text (`sk-abc defg …`, `sk-_ab cd`), so that a prefix a window's wrap or a blank cut the
/// rest of is one too. Of Stripe's keys only the live and
/// test shapes (`pk_live_…`, `rk_test_…`) count: `pk_users_id` is the name of a constraint.
#[allow(clippy::expect_used)]
static PREFIXES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\b(?:sk[-_]|(?:pk|rk)_(?:live|test)_|gh[pousr]_|github_pat_|glpat-|xox[abprs]-)(?:[A-Za-z0-9_-]{4}|\S{0,3}(?:\s|$))",
        r"|\b(?:AKIA|ASIA)[A-Z0-9]{4}",
        r"|\bAIza[A-Za-z0-9_-]{4}",
        r"|\beyJ[A-Za-z0-9_-]{4}",
        r"|\bnpm_[A-Za-z0-9]{12}",
        r"|\bhf_[A-Za-z0-9]{12}",
        r"|\bya29\.[A-Za-z0-9._-]{12}",
        r"|\bSG\.[A-Za-z0-9_-]{8}",
    ))
    .expect("a pattern of the masker")
});

/// What follows `Basic` when it is base64's, as short as four characters, and no word of capitals
/// alone (`Basic HTTP`, `Basic JSON`).
#[allow(clippy::expect_used)]
static BASIC_SCHEME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bbasic\s+([A-Za-z0-9._~+/=-]{4,})").expect("a pattern of the masker")
});

#[allow(clippy::expect_used)]
static URL_CREDENTIALS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(URL_USER_PASSWORD).expect("a pattern of the masker"));

/// The user of a URL that has no password: `scheme://user@host`.
const URL_USER_ONLY: &str = r"(?i)\b([a-z][a-z0-9+.-]*)://([^\s/@:]+)@";

#[allow(clippy::expect_used)]
static URL_USER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(URL_USER_ONLY).expect("a pattern of the masker"));

/// The schemes whose URL names the account to log in as, and not a token: `ssh://git@host` is
/// `git clone`'s way to say which account, and its user is seen in every screen of a pull. The
/// user of a URL of any other scheme is a token as often as a name (`https://TOKEN@host`,
/// GitHub's and GitLab's way to clone with one).
const ACCOUNT_SCHEMES: [&str; 8] = [
    "ssh", "sftp", "scp", "git", "git+ssh", "ssh+git", "svn+ssh", "rsync",
];

/// Where the user of each `scheme://user@` in `text` begins and ends, as byte offsets, for a
/// URL with no password in it and a scheme that is not an [`ACCOUNT_SCHEMES`] one.
fn url_user_spans(text: &str) -> Vec<(usize, usize)> {
    URL_USER
        .captures_iter(text)
        .filter(|found| {
            found.get(1).is_some_and(|scheme| {
                !ACCOUNT_SCHEMES
                    .iter()
                    .any(|account| scheme.as_str().eq_ignore_ascii_case(account))
            })
        })
        .filter_map(|found| found.get(2))
        .map(|user| (user.start(), user.end()))
        .collect()
}

/// Whether `text` holds a URL with a user and a password in it, or a user only (see
/// [`url_user_spans`]).
pub(crate) fn has_url_credentials(text: &str) -> bool {
    URL_CREDENTIALS.is_match(text) || !url_user_spans(text).is_empty()
}

/// Where the user and password of each `scheme://user:password@` in `text` begin and end, as
/// byte offsets, and the user of each one that has none (see [`url_user_spans`]).
pub(crate) fn url_credential_spans(text: &str) -> Vec<(usize, usize)> {
    URL_CREDENTIALS
        .captures_iter(text)
        .filter_map(|found| found.get(1))
        .map(|userinfo| (userinfo.start(), userinfo.end()))
        .chain(url_user_spans(text))
        .collect()
}

/// Whether `text` shows the beginning of a token of a kind with a known prefix (`sk-`, `ghp_`,
/// `AKIA…`, a JWT's `eyJ…`) or of a `Basic` credential, whatever follows it.
pub(crate) fn begins_a_token(text: &str) -> bool {
    PREFIXES.is_match(text)
        || BASIC_SCHEME.captures_iter(text).any(|found| {
            found.get(1).is_some_and(|m| {
                base64_credentials(m.as_str())
                    && !m.as_str().chars().all(|c| c.is_ascii_uppercase())
            })
        })
}

/// A run of forty letters and digits or more that is no number: it has a letter in it, in
/// any case. A run of small letters alone was taken for a word, and the review of `860010398`
/// found a 48-letter base64 value of them: no word is forty letters long, and a camel-case
/// name of that length is a false alarm that costs nothing.
fn is_base64_letters(found: &str) -> bool {
    found
        .trim_end_matches('=')
        .chars()
        .any(|c| c.is_ascii_alphabetic())
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
    let mut users: Vec<(usize, usize)> = url_user_spans(text);
    spans.extend(users.iter().copied());
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
