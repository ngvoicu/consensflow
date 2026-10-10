//! What the updater smoke builds: the installed apps (the bridge and the flip
//! release, each exported from its release tag) and the update (this checkout,
//! which ships no Node), each with the run's public key, and the update with
//! the next version. All are given by the build's own override
//! (`tauri build --config`), a file in the run's folder: the product's
//! configuration (`tauri.conf.json`) is never written, and a build with no
//! override is the product's own.
//!
//! An old tag's tree is built the way that tag builds: with its own `npm run`
//! scripts and the Tauri CLI it has, which are not this tree's.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use serde_json::{json, Value};

use super::bundle::{remove_all, under};
use super::evidence::Kind;
use super::signing::{clean_env, tauri_bin};
use super::versions::flip_tag;
use super::{files, Error, Result};
use crate::process::{self, Invocation};

/// The release the installed app is, for a user who skips the flip: the first
/// whose installed check is the relaxed one (identity, version, `cli/bin/cf`,
/// the seal), which is what lets it take an update that ships no Node. Its
/// daemon is Node's, and its `cf setup` writes the launcher that names Node.
pub const BRIDGE_TAG: &str = "v3.0.0-alpha.81";

/// The releases an app can be installed from, and what each is: the daemon its
/// app starts in a home that has not taken the way back, and whose `cf setup`
/// wrote the terminal's command (Node's, which names the bundled Node and its
/// `cf.mjs`, or the native `cf`'s, which names the `cf` and nothing else).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    Bridge,
    Flip,
}

impl Release {
    /// Every release, as the table of them has them.
    pub const ALL: [Self; 2] = [Self::Bridge, Self::Flip];

    /// The name the options and the reports use.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bridge => "bridge",
            Self::Flip => "flip",
        }
    }

    /// The release a name names.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|release| release.name() == name)
    }

    /// The daemon its app starts in a home that has not taken the way back.
    pub fn daemon(self) -> Kind {
        match self {
            Self::Bridge => Kind::Node,
            Self::Flip => Kind::Native,
        }
    }

    /// Whose `cf setup` wrote the terminal's command.
    pub fn setup(self) -> Kind {
        self.daemon()
    }
}

/// The tags of the releases this checkout follows: those in its history, but not
/// the one at the commit checked out, which is this release if it is tagged.
pub fn earlier_releases(repo: &Path, env: &Env) -> Result<Vec<String>> {
    let git = |args: &[&str]| -> Result<Vec<String>> {
        let ran = process::capture(
            &Invocation::new("git", repo).args(args.iter().copied()),
            env,
        )?;
        ensure!(
            ran.code == 0,
            "git {} failed: {}",
            args.join(" "),
            ran.stderr.trim()
        );
        Ok(ran
            .stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect())
    };
    let here: BTreeSet<String> = git(&["tag", "--points-at", "HEAD"])?.into_iter().collect();
    Ok(git(&["tag", "--merged", "HEAD", "--list", "v*"])?
        .into_iter()
        .filter(|tag| !here.contains(tag))
        .collect())
}

/// The tag of the flip release in this checkout's history: the newest release after the bridge.
pub fn flip_release(repo: &Path, env: &Env) -> Result<String> {
    let tags = earlier_releases(repo, env)?;
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    flip_tag(&tags, BRIDGE_TAG).map(str::to_string)
}

/// Where a build leaves its bundle: the workspace's one build folder.
pub fn built_app(checkout: &Path) -> PathBuf {
    under(
        checkout,
        &[
            "app",
            "src-tauri",
            "target",
            "release",
            "bundle",
            "macos",
            "ConsensFlow.app",
        ],
    )
}

/// The override a build is given: the run's public key, and the version where the build is the update.
pub fn override_config(public_key: &str, version: Option<&str>) -> Value {
    let mut config = json!({ "plugins": { "updater": { "pubkey": public_key } } });
    if let (Some(version), Some(config)) = (version, config.as_object_mut()) {
        config.insert("version".into(), json!(version));
    }
    config
}

/// The app's executable holds the run's public key and not the product's: the
/// override reached the build, and an update signed by the run's key is one that
/// the app, as built, could take. (The product's public key is in its
/// configuration, which is the one thing of its key this reads.)
pub fn assert_built_with(app: &Path, public_key: &str, product_key: &str, label: &str) -> Result {
    let executable = under(app, &["Contents", "MacOS", "app"]);
    let bytes = fs::read(&executable).map_err(files("read", &executable))?;
    let holds = |key: &str| memchr::memmem::find(&bytes, key.as_bytes()).is_some();
    ensure!(
        holds(public_key),
        "{label} was not built with this run's updater key"
    );
    ensure!(
        !holds(product_key),
        "{label} carries the product's updater key"
    );
    Ok(())
}

/// The updater key the product's configuration carries: public, and the one a build must not keep.
pub fn product_key_of(checkout: &Path) -> Result<String> {
    let file = under(checkout, &["app", "src-tauri", "tauri.conf.json"]);
    let text = fs::read_to_string(&file).map_err(files("read", &file))?;
    let config: Value = serde_json::from_str(&text)
        .map_err(|cause| Error::new(format!("could not read {}: {cause}", file.display())))?;
    config
        .pointer("/plugins/updater/pubkey")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::new(format!("{} names no updater key", file.display())))
}

/// An app to build.
pub struct Build<'a> {
    /// The tree it is built in.
    pub checkout: &'a Path,
    /// The run's folder, where the override is written.
    pub work: &'a Path,
    pub public_key: &'a str,
    /// The version the build is given, where it is the update.
    pub version: Option<&'a str>,
}

/// Runs `invocation` with the terminal as its output, and refuses a status other than 0.
fn ran(invocation: &Invocation, env: &Env) -> Result {
    match process::run(invocation, env)? {
        0 => Ok(()),
        status => Err(Error::new(format!(
            "{} ended with status {status}",
            invocation.display()
        ))),
    }
}

/// Builds the app of `checkout` as `npm --prefix app run build` does (the page's
/// bundle, the bundled `cf` and, in a checkout of the releases that ship it,
/// Node and the CLI staged, then Tauri), with the override in `work`, and
/// returns the bundle it left. The signing is ad hoc, as the product's
/// configuration has it (`signingIdentity: "-"`), and nothing in the environment
/// can say otherwise.
pub fn build_app(build: &Build, env: &Env) -> Result<PathBuf> {
    let app = build.checkout.join("app");
    let env = clean_env(env);
    ran(
        &Invocation::new("npm", &app).args(["run", "bundle:ui"]),
        &env,
    )?;
    ran(
        &Invocation::new("npm", &app).args(["run", "prepare-sidecar"]),
        &env,
    )?;
    fs::create_dir_all(build.work).map_err(files("make", build.work))?;
    let override_file = build
        .work
        .join(format!("tauri-{}.json", build.version.unwrap_or("as-is")));
    let config = override_config(build.public_key, build.version);
    // serde_json's pretty form is JSON.stringify's with two spaces.
    let text = serde_json::to_string_pretty(&config)
        .map_err(|cause| Error::new(format!("could not write the override: {cause}")))?;
    fs::write(&override_file, format!("{text}\n")).map_err(files("write", &override_file))?;
    let tauri = Invocation::new(tauri_bin(build.checkout), &app)
        .args(["build", "--bundles", "app", "--config"])
        .arg(&override_file);
    ran(&tauri, &env)?;
    let built = built_app(build.checkout);
    assert_built_with(
        &built,
        build.public_key,
        &product_key_of(build.checkout)?,
        &format!("the app built from {}", build.checkout.display()),
    )?;
    Ok(built)
}

/// The mark an export leaves: the tag it was made from.
const MARK: &str = ".exported-from";

/// What a build offline cannot fetch, and the checkout it is built in is given
/// from `repo`: the node modules, and the Node and console-host downloads (the
/// releases that ship Node fetch it).
const SHARED: [&[&str]; 3] = [
    &["node_modules"],
    &["app", "node_modules"],
    &["app", ".cache"],
];

/// The checkout of a release, exported from its tag (or any commit) into `into`
/// (once: a tag does not change) and given the caches of `repo` to build from.
pub fn export_release(repo: &Path, into: &Path, tag: &str, env: &Env) -> Result<PathBuf> {
    let mark = into.join(MARK);
    if fs::read_to_string(&mark).is_ok_and(|text| text.trim() == tag) {
        return Ok(into.to_path_buf());
    }
    remove_all(into)?;
    fs::create_dir_all(into).map_err(files("make", into))?;
    let archive = PathBuf::from(format!("{}.tar", into.display()));
    let exported = Invocation::new("git", repo)
        .args(["archive", "--format=tar", "--output"])
        .arg(&archive)
        .arg(tag);
    let exported = process::capture(&exported, env)?;
    if exported.code != 0 {
        let said = exported.stderr.trim();
        let said = if said.is_empty() {
            format!("git archive ended with status {}", exported.code)
        } else {
            said.to_string()
        };
        return Err(Error::new(format!(
            "the release {tag} is not in this repository (git fetch --tags): {said}"
        )));
    }
    let unpacked = process::capture(
        &Invocation::new("/usr/bin/tar", Path::new("."))
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(into),
        env,
    )?;
    ensure!(
        unpacked.code == 0,
        "/usr/bin/tar -xf {} -C {} failed: {}",
        archive.display(),
        into.display(),
        unpacked.stderr.trim()
    );
    remove_all(&archive)?;
    // A second root configuration inside the checkout would stop its linter from running.
    remove_all(&into.join("biome.json"))?;
    for shared in SHARED {
        let from = under(repo, shared);
        // What is not there to give is left out: a build says so, and the export does not.
        if from.exists() {
            let target = fs::canonicalize(&from).map_err(files("find", &from))?;
            let link = under(into, shared);
            link_dir(&target, &link).map_err(files("link", &link))?;
        }
    }
    fs::write(&mark, format!("{tag}\n")).map_err(files("write", &mark))?;
    Ok(into.to_path_buf())
}

/// A link at `link` to the folder `target`.
#[cfg(unix)]
fn link_dir(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// A link at `link` to the folder `target`.
#[cfg(windows)]
fn link_dir(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(test)]
mod tests;
