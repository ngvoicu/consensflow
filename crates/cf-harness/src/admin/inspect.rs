//! The look at one harness's CLI: its version, how it was installed, and the
//! latest release, in that order, and the extension made for the two harnesses
//! that load one.

use std::path::PathBuf;

use cf_base::env::Env;
use cf_proto::agents::Harness;

use super::row::{Distribution, Extension, Release, Row, Setup, Tested, Update, Version};
use super::source::release_source;
use super::version::{newer, version_of};
use super::{program, Inner, PROBE};
use crate::{claude, codex, devin, muse, pi};

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
        tested: Tested::of(harness, None),
        setup: None,
        distribution: Distribution::Unlooked,
        extension: None,
    };
    let Some(path) = path else {
        return row;
    };
    let (version, release, setup_version) = match harness {
        Harness::Muse => muse_version(inner, &path).await,
        _ => ask_version(inner, &path, &inner.env, harness).await,
    };
    row.version = version;
    row.tested = Tested::of(harness, release.as_deref());
    // Until A2 shares the launch verdict, keep the setup decision on the
    // original first-number reading, independently of the proved release.
    row.setup = setup(harness, setup_version.as_deref());
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
            let release = Release {
                value,
                source: source.url,
                command,
            };
            match comparison {
                Some(true) => Update::Available(release),
                Some(false) => Update::Current(release),
                _ => Update::Unknown(release),
            }
        }
        Err(reason) => Update::Error { reason, command },
    };
    row.extension = match harness {
        Harness::Pi => Some(Extension::of_pi(pi::prepare_extension(&inner.env))),
        _ => None,
    };
    row
}

/// Which Muse Code the launcher at `path` runs: the line of `.muse-version` beside it,
/// and only where there is none what it says to `--version`, asked with its update
/// and login off. Its `--version` alone checks for an update first, which can
/// download a new binary and write the install's files (`muse::version`).
async fn muse_version(inner: &Inner, path: &str) -> (Version, Option<String>, Option<String>) {
    if let Some(value) = muse::version::installed(&PathBuf::from(path)) {
        let release = muse::declared::pin_release(&value).map(str::to_owned);
        let setup_version = Some(value.clone());
        return (Version::Checked { value }, release, setup_version);
    }
    let quiet = muse::version::QUIET
        .iter()
        .fold(inner.env.clone(), |env, (name, value)| {
            env.with_var(name, value)
        });
    ask_version(inner, path, &quiet, Harness::Muse).await
}

/// What the CLI at `path` says to `--version`, run in `env`.
async fn ask_version(
    inner: &Inner,
    path: &str,
    env: &Env,
    harness: Harness,
) -> (Version, Option<String>, Option<String>) {
    let asked = program(env, PathBuf::from(path), vec!["--version".to_owned()]);
    match inner.capture.capture(asked, PROBE).await {
        Ok(said) => {
            let release = match harness {
                Harness::Claude => claude::declared::release(&said.stdout),
                Harness::Codex => codex::declared::release(&said.stdout),
                Harness::Pi => pi::declared::release(&said.stdout),
                Harness::Devin => devin::declared::release(&said.stdout),
                Harness::Muse => muse::declared::release(&said.stdout),
            };
            let setup_version = version_of(&said.stdout);
            let version = match release.or(setup_version) {
                Some(value) => Version::Checked {
                    value: value.to_owned(),
                },
                None => Version::Unknown {
                    reason: "Version output was not recognized".to_owned(),
                },
            };
            (
                version,
                release.map(str::to_owned),
                setup_version.map(str::to_owned),
            )
        }
        Err(failed) => (
            Version::Error {
                reason: if failed.killed {
                    "Version check timed out"
                } else {
                    "Version command failed"
                }
                .to_owned(),
            },
            None,
            None,
        ),
    }
}

/// What the CLI's `version` says of whether a window can be opened on it:
/// Devin's and Muse's always do. No other CLI has a setup.
fn setup(harness: Harness, version: Option<&str>) -> Option<Setup> {
    match harness {
        Harness::Devin => Some(devin_setup(version)),
        Harness::Muse => Some(muse_setup(version)),
        _ => None,
    }
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
