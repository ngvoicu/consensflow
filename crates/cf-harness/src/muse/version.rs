//! Which Muse Code a machine has, and whether ConsensFlow reads it.
//!
//! Muse writes its version in three places, each read here:
//! - `.muse-version` beside its launcher (`~/.local/bin`, `%LOCALAPPDATA%\Programs\muse`),
//!   one line naming the binary the launcher runs: `1.4.13-R6996.1`
//!   (`major.minor.patch-R<release>[.<n>]`), which the launcher flips when it updates
//!   itself;
//! - what the binary says to `--version`: `Muse Code 1.4.13 (1.4.13-R6996.1)`;
//! - each session log's `runtime.session.metadata`, `build.semver`: `1.4.13`.
//!
//! The file is read before `--version` is asked: the launcher's `--version` runs its
//! hourly update check first, which may download a new binary and write the install's
//! own files. Where it must be asked, it is asked with the update and its login off
//! ([`QUIET`]).

use std::fs;
use std::path::Path;

/// The oldest Muse Code whose record ConsensFlow reads: the one it was built on
/// (T-703, 2026-10-26).
pub(crate) const MINIMUM: [u32; 3] = [1, 4, 13];

/// The file beside the launcher that names the version it runs.
const VERSION_FILE: &str = ".muse-version";

/// What a `--version` of Muse's launcher is run with: no update check, no download,
/// and no login prompt for one.
pub(crate) const QUIET: [(&str, &str); 2] = [("MUSE_NO_AUTO_UPDATE", "1"), ("MUSE_LOGIN", "0")];

/// A version as Muse writes it: three numbers, and the release (`R<release>[.<n>]`)
/// where it says one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version {
    numbers: [u32; 3],
    release: Option<(u32, u32)>,
}

impl Version {
    /// The version in `text`: a version line of `.muse-version` (`1.4.13-R6996.1`), a
    /// record's semver (`1.4.13`), or what Muse says to `--version` (`Muse Code 1.4.13
    /// (1.4.13-R6996.1)`), whose release, in brackets, is read with it. None where
    /// it names none.
    pub(crate) fn of(text: &str) -> Option<Self> {
        let text = text.trim();
        let words: Vec<&str> = text.split_whitespace().collect();
        let line = match words.as_slice() {
            [one] => *one,
            [.., version, bracketed] => {
                let release = bracketed.strip_prefix('(')?.strip_suffix(')')?;
                let release = Self::line(release)?;
                return (Self::line(version)?.numbers == release.numbers).then_some(release);
            }
            _ => return None,
        };
        Self::line(line)
    }

    /// One version line: `1.4.13`, `1.4.13-R6996` or `1.4.13-R6996.1`.
    fn line(line: &str) -> Option<Self> {
        let (numbers, release) = match line.split_once('-') {
            Some((numbers, release)) => (numbers, Some(release.strip_prefix('R')?)),
            None => (line, None),
        };
        let numbers = whole_numbers::<3>(numbers)?;
        let release = match release {
            None => None,
            Some(release) => match release.split_once('.') {
                Some((release, n)) => Some((whole(release)?, whole(n)?)),
                None => Some((whole(release)?, 0)),
            },
        };
        Some(Self { numbers, release })
    }

    /// Whether ConsensFlow reads this Muse's record: [`MINIMUM`] or later.
    pub(crate) fn supported(self) -> bool {
        self.numbers >= MINIMUM
    }

    /// The three numbers, as a release feed and the page say them: `1.4.13`.
    pub(crate) fn semver(self) -> String {
        self.numbers.map(|number| number.to_string()).join(".")
    }
}

/// `text`'s `N` dot-separated whole numbers, and nothing else.
fn whole_numbers<const N: usize>(text: &str) -> Option<[u32; N]> {
    let parts: Vec<u32> = text.split('.').map(whole).collect::<Option<_>>()?;
    parts.try_into().ok()
}

/// A whole number written in digits alone.
fn whole(text: &str) -> Option<u32> {
    (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Whether `remote`, a release a feed names, is later than `local`, the version a
/// machine has: by the numbers, then by the release where both name one (a
/// `--version` read as three numbers alone is the release of those numbers it runs).
/// None where either names no version of Muse's.
pub(crate) fn newer(local: Option<&str>, remote: &str) -> Option<bool> {
    let (local, remote) = (Version::of(local?)?, Version::of(remote)?);
    if local.numbers != remote.numbers || local.release.is_none() || remote.release.is_none() {
        return Some(remote.numbers > local.numbers);
    }
    Some(remote.release > local.release)
}

/// The version line in `.muse-version` beside the launcher `executable`, where it
/// really is (a link is followed): none where there is no such file, or it names no
/// version.
pub(crate) fn installed(executable: &Path) -> Option<String> {
    let real = fs::canonicalize(executable).unwrap_or_else(|_| executable.to_owned());
    let line = fs::read_to_string(real.parent()?.join(VERSION_FILE)).ok()?;
    let line = line.trim();
    Version::of(line).map(|_| line.to_owned())
}

/// The oldest Muse ConsensFlow reads, as Muse writes a version: `1.4.13`.
pub(crate) fn minimum() -> String {
    MINIMUM.map(|part| part.to_string()).join(".")
}

/// What a launch is refused with, and the page says, of a Muse older than that.
pub(crate) fn required() -> String {
    format!(
        "Muse Code {} or newer is required. Update Muse Code before opening a pane.",
        minimum()
    )
}

#[cfg(test)]
mod tests;
