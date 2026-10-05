//! The look at one harness's CLI (`#inspect`, `src/harness-admin.js`): its
//! version, how it was installed, and the latest release, in that order,
//! and the extension made for the two harnesses that load one.

use std::path::PathBuf;

use cf_proto::agents::Harness;

use super::row::{Distribution, Extension, Release, Row, Setup, Update, Version};
use super::source::release_source;
use super::version::{newer, version_of};
use super::{program, Inner, PROBE};
use crate::{devin, opencode, pi};

/// The row of the harness `harness`, whose CLI is at `path` (none when it is
/// installed nowhere), the look begun at `checked_at`.
pub(super) async fn inspect(
    inner: &Inner,
    harness: Harness,
    path: Option<String>,
    checked_at: i64,
) -> Row {
    let mut row = Row {
        id: harness,
        installed: path.is_some(),
        path: path.clone(),
        checked_at,
        version: Version::NotInstalled,
        update: Update::NotChecked,
        setup: None,
        distribution: Distribution::Unlooked,
        extension: None,
    };
    let Some(path) = path else {
        return row;
    };
    row.version = ask_version(inner, &path).await;
    if harness == Harness::Devin {
        row.setup = Some(setup(row.version.value()));
    }
    let source = release_source(harness, &path, &inner.env);
    row.distribution = source
        .distribution
        .clone()
        .map_or(Distribution::Unrecognized, Distribution::Named);
    let command = source.update.as_ref().map(|argv| argv.join(" "));
    row.update = match inner.latest.latest(harness, &source).await {
        Ok(value) => {
            let comparison = newer(row.version.value(), &value);
            let release = Release {
                value,
                source: source.url,
                command,
            };
            match comparison {
                None => Update::Unknown(release),
                Some(true) => Update::Available(release),
                Some(false) => Update::Current(release),
            }
        }
        Err(reason) => Update::Error { reason, command },
    };
    row.extension = match harness {
        Harness::Pi => Some(Extension::of_pi(pi::prepare_extension(&inner.env))),
        Harness::Opencode => Some(Extension::of_opencode(opencode::prepare_extension(
            &inner.env,
        ))),
        _ => None,
    };
    row
}

/// What the CLI at `path` says to `--version`.
async fn ask_version(inner: &Inner, path: &str) -> Version {
    let asked = program(
        &inner.env,
        PathBuf::from(path),
        vec!["--version".to_owned()],
    );
    match inner.capture.capture(asked, PROBE).await {
        Ok(said) => match version_of(&said.stdout) {
            Some(value) => Version::Checked {
                value: value.to_owned(),
            },
            None => Version::Unknown {
                reason: "Version output was not recognized".to_owned(),
            },
        },
        Err(failed) => Version::Error {
            reason: if failed.killed {
                "Version check timed out"
            } else {
                "Version command failed"
            }
            .to_owned(),
        },
    }
}

/// Whether the Devin that said `version` is new enough to open a window on.
fn setup(version: Option<&str>) -> Setup {
    if version.is_some_and(devin::supported_version) {
        return Setup::Ready;
    }
    Setup::UpdateRequired {
        reason: format!(
            "Devin {} or newer is required. Update Devin before opening a pane.",
            devin::minimum_version()
        ),
    }
}
