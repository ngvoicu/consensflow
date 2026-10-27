//! The look at one harness's CLI: its version, how it was installed, and the
//! latest release, in that order, and the extension made for the two harnesses
//! that load one.

use std::path::PathBuf;

use cf_base::env::Env;
use cf_proto::agents::Harness;

use super::row::{Distribution, Extension, Release, Row, Setup, Update, Version};
use super::source::release_source;
use super::version::{newer, version_of};
use super::{program, Inner, PROBE};
use crate::{devin, muse, opencode, pi};

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
    row.version = match harness {
        Harness::Muse => muse_version(inner, &path).await,
        _ => ask_version(inner, &path, &inner.env).await,
    };
    row.setup = setup(harness, row.version.value());
    let source = release_source(harness, &path, &inner.env);
    row.distribution = source
        .distribution
        .clone()
        .map_or(Distribution::Unrecognized, Distribution::Named);
    let command = source.update.as_ref().map(|argv| argv.join(" "));
    row.update = match inner.latest.latest(harness, &source).await {
        Ok(value) => {
            let comparison = match harness {
                Harness::Muse => muse::version::newer(row.version.value(), &value),
                _ => newer(row.version.value(), &value),
            };
            let offered = offers(harness, row.setup.as_ref(), &value);
            let release = Release {
                value,
                source: source.url,
                command,
            };
            match comparison {
                Some(true) if offered => Update::Available(release),
                Some(false) => Update::Current(release),
                _ => Update::Unknown(release),
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

/// Which Muse Code the launcher at `path` runs: the line of `.muse-version` beside it,
/// and only where there is none what it says to `--version`, asked with its update
/// and login off. Its `--version` alone checks for an update first, which can
/// download a new binary and write the install's files (`muse::version`).
async fn muse_version(inner: &Inner, path: &str) -> Version {
    if let Some(value) = muse::version::installed(&PathBuf::from(path)) {
        return Version::Checked { value };
    }
    let quiet = muse::version::QUIET
        .iter()
        .fold(inner.env.clone(), |env, (name, value)| {
            env.with_var(name, value)
        });
    ask_version(inner, path, &quiet).await
}

/// What the CLI at `path` says to `--version`, run in `env`.
async fn ask_version(inner: &Inner, path: &str, env: &Env) -> Version {
    let asked = program(env, PathBuf::from(path), vec!["--version".to_owned()]);
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

/// What the CLI's `version` says of whether a window can be opened on it:
/// Devin's and Muse's always do, and OpenCode's only where it is one ConsensFlow
/// does not support yet. No other CLI has a setup.
fn setup(harness: Harness, version: Option<&str>) -> Option<Setup> {
    match harness {
        Harness::Devin => Some(devin_setup(version)),
        Harness::Muse => Some(muse_setup(version)),
        Harness::Opencode if version.is_some_and(opencode::unsupported_version) => {
            Some(Setup::NotSupportedYet {
                reason: format!("{}.", opencode::not_supported()),
            })
        }
        _ => None,
    }
}

/// Whether the release `release` of `harness` is offered as an update to its
/// CLI, whose setup is `setup`. None is to a CLI no window is opened on, and
/// none is of a release no window would be opened on either: OpenCode's feed
/// says its latest is a 2 since 2.0, and the human who went back to the last
/// 1.x is not offered the way to the 2 again.
fn offers(harness: Harness, setup: Option<&Setup>, release: &str) -> bool {
    let held_back = matches!(setup, Some(Setup::NotSupportedYet { .. }))
        || (harness == Harness::Opencode && opencode::unsupported_version(release));
    !held_back
}

/// What the Muse of `version` is to the page: one older than the minimum is to be
/// updated, and any other is ready, as a launch takes it (`muse::launch::pinned`).
fn muse_setup(version: Option<&str>) -> Setup {
    let found = version.and_then(muse::version::Version::of);
    if found.is_some_and(|found| !found.supported()) {
        return Setup::UpdateRequired {
            reason: muse::version::required(),
        };
    }
    Setup::Ready
}

/// Whether the Devin that said `version` is new enough to open a window on.
fn devin_setup(version: Option<&str>) -> Setup {
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
