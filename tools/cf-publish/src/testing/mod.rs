//! What the tests are made of: GitHub on this machine ([`Github`], which the
//! publisher's tests run against, and which a `gh` that is a process of its
//! own asks over HTTP, `fake-gh`), the files a built release holds, and the
//! text of the workflow the steps are read off. Built only for tests, with the
//! `test-support` feature: the binary the workflow runs has none of it.

mod built;
mod http;
mod sim;
pub mod workflow;
pub mod worlds;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::json;

use crate::manifest::Manifest;
use crate::read::Patience;
use crate::version::Version;

pub use built::{
    archive_name, archive_of, built_files, download, folder_of, installer_name, latest_json,
    portable_name, published_assets, with_archive_url, Files, TempDir,
};
pub use sim::{Call, Github, Handle, Served, Snapshot, Spec};

/// The rule these tests hold the code to, whatever bridge the repository's
/// manifest names today. alpha.80 is built from the code before the bridge: an
/// ordinary release on the old feed, which is what the old feed serves until
/// the bridge reaches it.
pub const BRIDGE: &str = "3.0.0-alpha.81";
pub const OLDER: &str = "3.0.0-alpha.80";
pub const LATER: &str = "3.0.0-alpha.82";
pub const NEWER: &str = "3.0.0-alpha.83";

/// The version `text` says.
pub fn v(text: &str) -> Version {
    Version::parse(text).expect("a semantic version")
}

/// The manifest of the rule above: both generations of feeds, and [`BRIDGE`] as
/// the bridge, with alpha the channel in use.
pub fn rule() -> Manifest {
    rule_with(BRIDGE, &["alpha"])
}

/// The same feeds with another bridge, and other channels in use.
pub fn rule_with(bridge: &str, legacy: &[&str]) -> Manifest {
    Manifest::from_json(&json!({
        "feeds": { "alpha": "feed-alpha", "stable": "feed-stable" },
        "legacy": { "alpha": "update-alpha", "stable": "update-stable" },
        "bridge": { "version": bridge, "legacy": legacy },
    }))
    .expect("a manifest")
}

/// Two reads at most, a millisecond apart: a feed that is not right is not waited for.
pub fn quick() -> Patience {
    Patience::new(2, Duration::from_millis(1))
}

/// What the old apps check an archive for, as `tar -t` would list it: the
/// archive is not read where this is given.
pub const OLD_LAYOUT: [&str; 6] = [
    "ConsensFlow.app/Contents/MacOS/node",
    "ConsensFlow.app/Contents/Resources/cli/package.json",
    "ConsensFlow.app/Contents/Resources/cli/bin/cf.mjs",
    "ConsensFlow.app/Contents/Resources/cli/bin/cf",
    "ConsensFlow.app/Contents/Resources/cli/hosts/pi-extension/consensflow-delivery.mjs",
    "ConsensFlow.app/Contents/Resources/cli/src/core/daemon.js",
];

/// What an archive the apps before the bridge install holds, as `tar -t` lists it.
pub fn old_apps_layout() -> Vec<String> {
    const ROOT: &str = "ConsensFlow.app/Contents/";
    [
        "MacOS/app",
        "MacOS/node",
        "Resources/cli/package.json",
        "Resources/cli/bin/cf.mjs",
        "Resources/cli/bin/cf",
        "Resources/cli/hosts/pi-extension/consensflow-delivery.mjs",
        "Resources/cli/src/core/daemon.js",
    ]
    .iter()
    .map(|name| format!("{ROOT}{name}"))
    .collect()
}

/// The same without a byte of Node's: what the release after the bridge may hold.
pub fn node_free_layout() -> Vec<String> {
    ["MacOS/app", "Resources/cli/bin/cf"]
        .iter()
        .map(|name| format!("ConsensFlow.app/Contents/{name}"))
        .collect()
}

/// How a process ended, and what it said.
#[derive(Debug)]
pub struct Ran {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Ran {
    /// Everything it said, for a failed assertion to show.
    pub fn said(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

/// The directories of the PATH this test runs with.
pub fn path_dirs() -> Vec<PathBuf> {
    // The test reads its own environment, for the PATH that finds `curl` and `tar`.
    #[allow(clippy::disallowed_methods)]
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).collect()
}

/// A PATH of `first`, then this test's own.
pub fn path_with(first: &Path) -> std::ffi::OsString {
    let dirs = std::iter::once(first.to_path_buf()).chain(path_dirs());
    std::env::join_paths(dirs).expect("a PATH")
}

/// A folder that holds `fake_gh` (the built `fake-gh`) as `gh`, to put first on
/// a PATH: whatever runs `gh` then asks the simulator `GH_SIM` names.
pub fn gh_dir(fake_gh: &Path) -> TempDir {
    let dir = TempDir::new("gh");
    let name = format!("gh{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(fake_gh, dir.path().join(name)).expect("the fake gh is copied");
    dir
}

/// The variables a run of `cf-publish` reads, which a test sets for itself.
const WORKFLOW_VARIABLES: [&str; 5] = [
    "GITHUB_EVENT_NAME",
    "GITHUB_REF_TYPE",
    "GITHUB_REF_NAME",
    "GH_REPO",
    "GITHUB_REPOSITORY",
];

/// Runs `binary` (the built `cf-publish`) with `args`, as the workflow would
/// but for the variables it sets itself: those are cleared and then `env` is
/// set, and `path`, where given, is the PATH it runs with.
#[allow(clippy::disallowed_methods)] // The test starts what it tests.
fn run_with(
    binary: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    path: Option<std::ffi::OsString>,
) -> Ran {
    let mut command = Command::new(binary);
    command.args(args);
    for name in WORKFLOW_VARIABLES {
        command.env_remove(name);
    }
    command.envs(env.iter().copied());
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let output = command.output().expect("the binary runs");
    Ran {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Runs `binary` with `args` and `env`, and `path_first`, where given, first on
/// the PATH it runs with.
pub fn run_binary(
    binary: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    path_first: Option<&Path>,
) -> Ran {
    run_with(binary, args, env, path_first.map(path_with))
}

/// Runs `binary` with `args` and `env`, and a PATH of the folder `only` and
/// nothing else: the programs it starts are the ones that folder holds.
pub fn run_binary_on(binary: &Path, args: &[&str], env: &[(&str, &str)], only: &Path) -> Ran {
    run_with(binary, args, env, Some(only.as_os_str().to_owned()))
}
