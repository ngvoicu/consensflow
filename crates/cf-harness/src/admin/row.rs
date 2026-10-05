//! What the admin says of a harness, as the page reads it: a row of what is
//! known of one CLI, and what became of an update. Each is written with its
//! fields in the order `src/harness-admin.js` builds them, absent where it
//! left a key out, so that its JSON reads as Node's did.

use std::rc::Rc;

use cf_proto::agents::Harness;
use serde::{Serialize, Serializer};

use crate::opencode;
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
    /// Whether Devin is new enough to open a window on (Devin's alone).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup: Option<Setup>,
    #[serde(skip_serializing_if = "Distribution::is_unlooked")]
    pub distribution: Distribution,
    /// The extension made for the CLI (Pi's and OpenCode's alone).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<Extension>,
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
    /// The release was read; the versions are no three whole numbers each.
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

/// Whether Devin is new enough to open a window on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Setup {
    Ready,
    UpdateRequired { reason: String },
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
    pub(super) fn of_pi(prepared: pi::Extension) -> Self {
        match prepared {
            pi::Extension::NotInstalled => Self::not_installed(),
            pi::Extension::InstalledUnverified { path } => Self::unverified(path),
            pi::Extension::Error { reason } => Self::failed(reason),
        }
    }

    /// OpenCode's plugin, as it was prepared: its settings file is the
    /// launch's, and not part of what the row says.
    pub(super) fn of_opencode(prepared: opencode::Extension) -> Self {
        match prepared {
            opencode::Extension::NotInstalled => Self::not_installed(),
            opencode::Extension::InstalledUnverified { path, .. } => Self::unverified(path),
            opencode::Extension::Error { reason } => Self::failed(reason),
        }
    }
}

/// How an update ended, in the words the page switches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Ended {
    /// The install method is not recognized: nothing was run.
    Unsupported,
    Updated,
    Unchanged,
    Failed,
}

/// What an update came to.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Outcome {
    /// Nothing was run: the install method is not recognized.
    Unsupported {
        id: Harness,
        state: Ended,
        reason: String,
        harness: Rc<Row>,
    },
    /// The command ran, and the CLI was checked again.
    Ran {
        id: Harness,
        state: Ended,
        /// The version before and after, when one was read.
        before: Option<String>,
        after: Option<String>,
        command: String,
        /// The last of what the command wrote, to either stream.
        output: String,
        /// Why it failed.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// The row as it is after the update.
        harness: Rc<Row>,
    },
}

#[cfg(test)]
mod tests;
