//! The files a release publishes, and what the apps before the bridge need of
//! its Mac archive.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::failure::Failure;
use crate::process;
use crate::version::Version;

/// The platform a latest.json names the Mac archive under.
pub const TARGET: &str = "darwin-aarch64";

/// What the updater of the apps before the bridge asks of an archive
/// (`validate_bundle` in app/src-tauri/src/update_install.rs at
/// v3.0.0-alpha.79, which 3.0.0-alpha.80 shares, in every app already
/// installed): these files, and something in these folders. An archive without
/// them is one those apps download and refuse.
const OLD_APPS_REQUIRE: [&str; 6] = [
    "ConsensFlow.app/Contents/MacOS/node",
    "ConsensFlow.app/Contents/Resources/cli/package.json",
    "ConsensFlow.app/Contents/Resources/cli/bin/cf.mjs",
    "ConsensFlow.app/Contents/Resources/cli/bin/cf",
    "ConsensFlow.app/Contents/Resources/cli/hosts/",
    "ConsensFlow.app/Contents/Resources/cli/src/",
];

/// The files a release publishes, where the folder that was built holds each:
/// the names the rest of the crate asks for them by. Paths use `/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseAssets {
    pub dmg: String,
    pub archive: String,
    pub signature: String,
    pub metadata: String,
    pub installer: String,
    pub portable: String,
}

impl ReleaseAssets {
    /// The files of the release `version`.
    pub fn of(version: &Version) -> Self {
        let mac = format!("ConsensFlow_{version}_aarch64");
        Self {
            dmg: format!("{mac}.dmg"),
            archive: format!("{mac}.app.tar.gz"),
            signature: format!("{mac}.app.tar.gz.sig"),
            metadata: "latest.json".to_string(),
            installer: format!("nsis/ConsensFlow_{version}_x64-setup.exe"),
            portable: format!("portable/ConsensFlow_{version}_x64-portable.exe"),
        }
    }

    /// All of them, in the order they are uploaded in.
    pub fn all(&self) -> [&str; 6] {
        [
            &self.dmg,
            &self.archive,
            &self.signature,
            &self.metadata,
            &self.installer,
            &self.portable,
        ]
    }

    /// The bridge's files the old apps still need: the Mac archive, and the
    /// Windows installer and portable they send people to.
    pub fn retained(&self) -> Vec<&str> {
        vec![&self.archive, &self.installer, &self.portable]
    }
}

/// The file name at the end of a `/` path.
pub fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Where the `/` path `path` is in the folder `dir`.
pub fn on_disk(dir: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(dir.to_path_buf(), |at, part| at.join(part))
}

/// What the old apps require of an archive that it lacks: the members of
/// `names`, as `tar -t` lists them.
pub fn missing_for_old_apps(names: &[String]) -> Vec<String> {
    let listed: Vec<&str> = names
        .iter()
        .map(|name| name.strip_prefix("./").unwrap_or(name))
        .collect();
    OLD_APPS_REQUIRE
        .iter()
        .filter(|required| {
            if required.ends_with('/') {
                !listed.iter().any(|name| name.starts_with(**required))
            } else {
                !listed.contains(required)
            }
        })
        .map(|required| (*required).to_string())
        .collect()
}

/// The members of the archive at `archive`.
pub fn members_of(archive: &Path) -> Result<Vec<String>, Failure> {
    let args = [OsStr::new("-tzf"), archive.as_os_str()];
    let listing = process::capture("tar", args, None)
        .map_err(|cause| Failure::new(format!("could not run tar: {cause}")))?;
    if listing.code != 0 {
        return Err(Failure::new(format!(
            "tar could not list {}: {}",
            archive.display(),
            String::from_utf8_lossy(&listing.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}
