//! The versions of the apps the updater smoke runs, and the one the update is
//! given. The installed apps are the bridge (`v3.0.0-alpha.81`) and the flip
//! release, and the update is this checkout, the release that ships no Node,
//! whose version is the flip's until a release moves it: an app takes only a
//! newer update (`install_archive`), so the update's build is given the next
//! version through the build's own override (`build.rs`), never in a file of
//! the product's.

use std::cmp::Ordering;

use super::{Error, Result};

/// One dot-separated part of a pre-release: numbers go before words.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Number(u64),
    Word(String),
}

/// A version as semver reads it: `3.0.0-alpha.81` is the core `[3, 0, 0]` and
/// the pre-release `[Word("alpha"), Number(81)]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<Part>,
}

/// Whether `text` is one or more ASCII digits.
fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// The version `text` says, or why it is not one: `major.minor.patch` and, after
/// a `-`, a pre-release of letters, digits, dots and dashes. Nothing else is
/// one (not `v3.0.0`, not `3.0`, not a build after a `+`).
pub fn parse_version(text: &str) -> Result<Version> {
    let not_one = || Error::new(format!("not a semantic version: {text}"));
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    let [major, minor, patch] = numbers[..] else {
        return Err(not_one());
    };
    if ![major, minor, patch].into_iter().all(is_number) {
        return Err(not_one());
    }
    let allowed = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-';
    if pre.is_some_and(|pre| pre.is_empty() || !pre.bytes().all(allowed)) {
        return Err(not_one());
    }
    let number = |text: &str| text.parse::<u64>().map_err(|_| not_one());
    let parts = pre
        .unwrap_or_default()
        .split('.')
        .filter(|part| !part.is_empty())
        .map(|part| {
            if is_number(part) {
                number(part).map(Part::Number)
            } else {
                Ok(Part::Word(part.to_string()))
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Version {
        core: [number(major)?, number(minor)?, number(patch)?],
        pre: parts,
    })
}

/// Semver's order: the core first, a pre-release before its release, numbers
/// before words.
pub fn compare_versions(a: &str, b: &str) -> Result<Ordering> {
    let (left, right) = (parse_version(a)?, parse_version(b)?);
    Ok(left.cmp(&right))
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| {
            // A release is after its own pre-releases.
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Part {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Number(x), Self::Number(y)) => x.cmp(y),
            (Self::Word(x), Self::Word(y)) => x.cmp(y),
            (Self::Number(_), Self::Word(_)) => Ordering::Less,
            (Self::Word(_), Self::Number(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for Part {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The version after `text`: the next number of a pre-release (`alpha.81` to
/// `alpha.82`), else the next patch.
pub fn next_version(text: &str) -> Result<String> {
    let Version { core, mut pre } = parse_version(text)?;
    if let Some(last) = pre.iter().rposition(|part| matches!(part, Part::Number(_))) {
        if let Part::Number(number) = &mut pre[last] {
            *number += 1;
        }
        let parts: Vec<String> = pre
            .iter()
            .map(|part| match part {
                Part::Number(number) => number.to_string(),
                Part::Word(word) => word.clone(),
            })
            .collect();
        return Ok(format!(
            "{}.{}.{}-{}",
            core[0],
            core[1],
            core[2],
            parts.join(".")
        ));
    }
    Ok(format!("{}.{}.{}", core[0], core[1], core[2] + 1))
}

/// The newest of `versions`.
pub fn newest<'a>(versions: &[&'a str]) -> Result<&'a str> {
    let mut latest = *versions
        .first()
        .ok_or_else(|| Error::new("no version to take the newest of"))?;
    for each in &versions[1..] {
        if compare_versions(each, latest)? == Ordering::Greater {
            latest = each;
        }
    }
    Ok(latest)
}

/// The release the flip is: the newest of `tags` (`v3.0.0-alpha.82`, as `git
/// tag` names them) that is newer than the bridge's. The update goes to an
/// installed app of the release before it, and a release between the bridge and
/// this checkout's, the flip's or a later one that still ships Node, is such an
/// app. Nothing between them is an error that says how to name the flip.
pub fn flip_tag<'a>(tags: &[&'a str], bridge: &str) -> Result<&'a str> {
    let bridge_version = bridge.strip_prefix('v').unwrap_or(bridge);
    let mut later = Vec::new();
    for tag in tags {
        let Some(version) = tag.strip_prefix('v') else {
            continue;
        };
        // What is no release tag is none.
        if parse_version(version).is_err() {
            continue;
        }
        if compare_versions(version, bridge_version)? == Ordering::Greater {
            later.push((*tag, version));
        }
    }
    let Some(&(mut latest, mut latest_version)) = later.first() else {
        return Err(Error::new(format!(
            "no release newer than the bridge ({bridge}) is tagged here: the flip release is \
             not out, or its tag is not fetched (git fetch --tags); name the flip with \
             --flip-ref <tag or commit> or --flip <a checkout of it>"
        )));
    };
    for &(tag, version) in &later[1..] {
        if compare_versions(version, latest_version)? == Ordering::Greater {
            (latest, latest_version) = (tag, version);
        }
    }
    Ok(latest)
}

/// The version the update is built as: this checkout's own where a release has
/// moved it past every installed app's, else the one after the newest installed
/// app's.
pub fn update_version(checkout: &str, installed: &[&str]) -> Result<String> {
    let latest = newest(installed)?;
    if compare_versions(checkout, latest)? == Ordering::Greater {
        Ok(checkout.to_string())
    } else {
        next_version(latest)
    }
}

#[cfg(test)]
mod tests;
