//! Which release comes before which, and the release a feed names.
//!
//! A release is a semantic version: `major.minor.patch` and, after a `-`, the
//! dot-separated identifiers of a pre-release. Nothing else is one: a leading
//! `v`, two numbers, build metadata after a `+` are all refused, since a tag or
//! a feed that names one of those names no release the rule can order.

use std::cmp::Ordering;
use std::fmt;

use serde_json::Value;

use crate::failure::Failure;

/// A channel of releases: every release is alpha's, a stable one is stable's too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Alpha,
    Stable,
}

impl Channel {
    /// Both, in the order the manifest names them.
    pub const ALL: [Channel; 2] = [Channel::Alpha, Channel::Stable];

    /// The word the manifest and the messages use for it.
    pub fn name(self) -> &'static str {
        match self {
            Channel::Alpha => "alpha",
            Channel::Stable => "stable",
        }
    }

    /// The channel `name` names, if it names one.
    pub fn from_name(name: &str) -> Option<Channel> {
        Channel::ALL
            .into_iter()
            .find(|channel| channel.name() == name)
    }
}

/// A semantic version, as the text that named it and the order it has.
#[derive(Clone, Debug)]
pub struct Version {
    text: String,
    /// Major, minor and patch as decimal digits without leading zeros: numbers
    /// of any size are compared exactly.
    core: [String; 3],
    /// The identifiers after the first `-`.
    pre: Option<Vec<String>>,
}

impl Version {
    /// The version `text` says, or why it is not one.
    pub fn parse(text: &str) -> Result<Self, Failure> {
        parse(text).ok_or_else(|| Failure::new(format!("not a semantic version: {text}")))
    }

    /// The text it was parsed from.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether it is an alpha: it has a pre-release.
    pub fn is_prerelease(&self) -> bool {
        self.pre.is_some()
    }

    /// The channels a release belongs to: every release is alpha's, a stable
    /// one is stable's too.
    pub fn channels(&self) -> &'static [Channel] {
        if self.is_prerelease() {
            &[Channel::Alpha]
        } else {
            &[Channel::Alpha, Channel::Stable]
        }
    }
}

fn parse(text: &str) -> Option<Version> {
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    let [major, minor, patch] = numbers[..] else {
        return None;
    };
    if ![major, minor, patch].into_iter().all(is_number) {
        return None;
    }
    let pre = match pre {
        None => None,
        Some(pre) => {
            let allowed = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-';
            if pre.is_empty() || !pre.bytes().all(allowed) {
                return None;
            }
            Some(pre.split('.').map(str::to_string).collect())
        }
    };
    Some(Version {
        text: text.to_string(),
        core: [major, minor, patch].map(without_leading_zeros),
        pre,
    })
}

/// Whether `text` is one or more decimal digits.
fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn without_leading_zeros(digits: &str) -> String {
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Two numbers written without leading zeros, of any size.
fn compare_numbers(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// Semantic-version precedence of two pre-releases: identifiers of digits come
/// before the others and are compared as numbers, the others as text, and the
/// shorter list comes first where all it has is the same.
fn compare_identifiers(a: &[String], b: &[String]) -> Ordering {
    for (index, id) in a.iter().enumerate() {
        let Some(other) = b.get(index) else {
            return Ordering::Greater;
        };
        let numbers = (is_number(id), is_number(other));
        if numbers.0 != numbers.1 {
            return if numbers.0 {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        if id == other {
            continue;
        }
        let order = if numbers.0 {
            compare_numbers(&without_leading_zeros(id), &without_leading_zeros(other))
        } else {
            id.cmp(other)
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        for (a, b) in self.core.iter().zip(&other.core) {
            let order = compare_numbers(a, b);
            if order != Ordering::Equal {
                return order;
            }
        }
        match (&self.pre, &other.pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => compare_identifiers(a, b),
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text)
    }
}

/// The version a latest.json (`body`) names, or none where it is not one, or
/// names no semantic version.
pub fn named_release(body: &[u8]) -> Option<Version> {
    let document: Value = serde_json::from_slice(body).ok()?;
    Version::parse(document.get("version")?.as_str()?).ok()
}

/// The version a feed's latest.json (`body`) names where that release comes
/// after `version` by precedence, and none otherwise: it names `version` itself
/// or an earlier release, or none (what is not a latest.json names no release).
/// A feed is never moved to a release before the one it names, so this is what
/// a run for `version` asks of a feed before it changes it, and what the check
/// afterwards accepts of one it left alone.
pub fn later_release(body: &[u8], version: &Version) -> Option<Version> {
    named_release(body).filter(|named| named > version)
}
