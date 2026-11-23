//! What the admin says of a harness, as the page reads it: a row of what is
//! known of one CLI. Each is written with its
//! fields in the order Node's admin built them, absent where it left a key out,
//! so that its JSON reads as Node's did.

use cf_proto::agents::Harness;
use serde::{Serialize, Serializer};

use crate::pi;

/// What is known of one harness's CLI.
#[derive(Debug, Serialize)]
pub struct Row {
    pub id: Harness,
    /// Where its CLI is; none when it is not installed.
    pub path: Option<String>,
    pub installed: bool,
    /// When it was looked at, the time the look began, in milliseconds.
    #[serde(rename = "checkedAt")]
    pub checked_at: i64,
    pub version: Version,
    pub update: Update,
    pub tested: Tested,
    /// Whether a window can be opened on the CLI: Devin's always.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup: Option<Setup>,
    #[serde(skip_serializing_if = "Distribution::is_unlooked")]
    pub distribution: Distribution,
    /// The extension made for the CLI (Pi's alone).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<Extension>,
}

/// The releases tested with ConsensFlow and the installed release, when proved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tested {
    pub newest: &'static str,
    pub oldest: Option<String>,
    pub release: Option<String>,
    pub beyond: bool,
}

impl Tested {
    pub(super) fn of(harness: Harness, release: Option<&str>) -> Self {
        let (newest, oldest) = crate::launch::declared(harness);
        Self::with_newest(harness, newest, oldest, release)
    }

    fn with_newest(
        harness: Harness,
        newest: &'static str,
        oldest: Option<String>,
        release: Option<&str>,
    ) -> Self {
        let beyond = release
            .and_then(|release| crate::launch::newer(harness, Some(newest), release))
            .unwrap_or(false);
        Self {
            newest,
            oldest,
            release: release.map(str::to_owned),
            beyond,
        }
    }
}

/// What the CLI said to `--version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Version {
    /// There is no CLI to ask.
    NotInstalled,
    Checked {
        value: String,
    },
    /// It answered, and no version is in what it said.
    Unknown {
        reason: String,
    },
    /// It did not answer.
    Error {
        reason: String,
    },
}

impl Version {
    /// The version, when one was read.
    pub fn value(&self) -> Option<&str> {
        match self {
            Version::Checked { value } => Some(value),
            _ => None,
        }
    }
}

/// How the CLI's version stands against the latest release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Update {
    /// The feed was not asked.
    NotChecked,
    /// The release was read, and says nothing of being newer: the versions
    /// are no three whole numbers each.
    Unknown(Release),
    Available(Release),
    Current(Release),
    /// The feed did not say.
    Error {
        reason: String,
        command: Option<String>,
    },
}

/// The latest release the feed said, where it was asked, and the command that
/// brings the CLI to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Release {
    pub value: String,
    pub source: String,
    pub command: Option<String>,
}

/// Whether a window can be opened on the CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Setup {
    Ready,
    /// The launch's check read the CLI and refuses it: the launch's own sentence.
    UpdateRequired {
        reason: String,
    },
    /// The launch's check could not read the CLI: the launch's own sentence.
    Unchecked {
        reason: String,
    },
}

/// How the CLI was installed, as the row says it: no key before its
/// install is looked at, and null when it is not recognized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Distribution {
    /// Not looked at: a CLI that is not installed has none to say.
    Unlooked,
    Unrecognized,
    Named(String),
}

impl Distribution {
    fn is_unlooked(&self) -> bool {
        matches!(self, Distribution::Unlooked)
    }
}

impl Serialize for Distribution {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Distribution::Named(name) => serializer.serialize_str(name),
            Distribution::Unlooked | Distribution::Unrecognized => serializer.serialize_none(),
        }
    }
}

/// What came of making the extension a CLI is told to load: Node's
/// `{ state, path }`, with the reason where it failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Extension {
    state: &'static str,
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

impl Extension {
    fn not_installed() -> Self {
        Self {
            state: "not-installed",
            path: None,
            reason: None,
        }
    }

    fn unverified(path: String) -> Self {
        Self {
            state: "installed-unverified",
            path: Some(path),
            reason: None,
        }
    }

    fn failed(reason: String) -> Self {
        Self {
            state: "error",
            path: None,
            reason: Some(reason),
        }
    }

    /// Pi's extension, as it was prepared.
    pub(crate) fn of_pi(prepared: pi::Extension) -> Self {
        match prepared {
            pi::Extension::NotInstalled => Self::not_installed(),
            pi::Extension::InstalledUnverified { path } => Self::unverified(path),
            pi::Extension::Error { reason } => Self::failed(reason),
        }
    }
}

#[cfg(test)]
mod tests;
