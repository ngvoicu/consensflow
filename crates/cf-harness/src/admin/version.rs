//! The version a CLI says and whether a release is newer than it.

use std::sync::LazyLock;

use cf_base::js;
use regex::Regex;

use crate::shared::pattern::compile;

/// The version in what a CLI says: three dotted numbers, with a
/// pre-release after them, that follow the start of the text, white space
/// or a `v`, and are followed by white space, the end of the text or `)`
/// (`/(?:^|\s|v)(\d+\.\d+\.\d+(?:-[\w.-]+)?)(?=\s|$|\))/`).
///
/// The lookahead is consumed here, which the first match, the only one
/// taken, does not tell apart: no shorter reading of the number or of the
/// pre-release ends before white space, `)` or the end, so what the
/// lookahead refuses, backtracking never rescues.
static VERSION: LazyLock<Regex> = LazyLock::new(|| {
    compile(r"(?:^|\s|v)([0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9_.-]+)?)(?:\s|$|\))")
});

/// The version `text` names, or none (`versionOf`).
pub(super) fn version_of(text: &str) -> Option<&str> {
    VERSION
        .captures(text)
        .and_then(|found| found.get(1))
        .map(|version| version.as_str())
}

/// Whether `remote` is a later release than `local`, both read as three
/// whole numbers, a number at a time; none when either is not three whole
/// numbers, so no release is called newer or current on a guess (`newer`).
pub(super) fn newer(local: Option<&str>, remote: &str) -> Option<bool> {
    let (local, remote) = (numbers(local?)?, numbers(remote)?);
    for (before, after) in local.into_iter().zip(remote) {
        if before != after {
            return Some(after > before);
        }
    }
    Some(false)
}

/// The three numbers of `version` (`/^\d+\.\d+\.\d+$/`), each as `Number`
/// reads it.
fn numbers(version: &str) -> Option<[f64; 3]> {
    let mut parts = version.split('.');
    let found = [parts.next()?, parts.next()?, parts.next()?];
    let whole = |part: &&str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if parts.next().is_some() || !found.iter().all(whole) {
        return None;
    }
    Some(found.map(js::number))
}

#[cfg(test)]
mod tests;
