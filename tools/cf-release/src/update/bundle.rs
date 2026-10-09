//! The app bundle the release built. Its `Info.plist` names the executable and
//! the version; its `cf` (the command a window runs) says the version it was
//! compiled with. Both must be one and the same canonical semantic version,
//! and the one the sources say.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::js;

use super::info::{self, Info};
use crate::process;
use crate::version::{self, VersionError};

/// A bundle that is what the release says it is.
#[derive(Debug)]
pub struct Bundle {
    /// The folder, whole.
    pub path: PathBuf,
    /// The version its `Info.plist` and its `cf` agree on.
    pub version: String,
}

/// Why a folder is not the bundle of a release.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("bundle is not a .app directory: {}", .0.display())]
    NotAnApp(PathBuf),
    /// The `Info.plist` has no such text, or cannot be read at all.
    #[error("bundle has no readable {0} in Info.plist")]
    NoField(&'static str),
    #[error("bundle executable name is unsafe")]
    UnsafeExecutable,
    #[error("bundle is missing {what}: {}", .path.display())]
    Missing { what: &'static str, path: PathBuf },
    #[error(transparent)]
    Version(#[from] VersionError),
    #[error("the bundled cf did not say its version (cf --version): {0}")]
    CfSilent(String),
    #[error("bundle and its cf versions differ: {version} != {cf}")]
    VersionsDiffer { version: String, cf: String },
}

/// Reads the bundle in the folder `app` and holds it to what a release is.
/// Its `cf` is run, with `env`, to ask it for its version.
pub fn read(app: &Path, env: &Env) -> Result<Bundle, BundleError> {
    let path = std::path::absolute(app).map_err(|_| BundleError::NotAnApp(app.to_path_buf()))?;
    let named_app = path
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.ends_with(".app"));
    if !named_app || !path.is_dir() {
        return Err(BundleError::NotAnApp(path));
    }

    let info = fs::read(path.join("Contents").join("Info.plist"))
        .ok()
        .and_then(|bytes| Info::parse(&bytes));
    let field = |key: &'static str| {
        info.as_ref()
            .and_then(|info| info.text(key))
            .ok_or(BundleError::NoField(key))
    };

    let executable = field(info::EXECUTABLE)?;
    if executable.contains(['/', '\\', '\0']) {
        return Err(BundleError::UnsafeExecutable);
    }
    let binary = path.join("Contents").join("MacOS").join(executable);
    let cf = path
        .join("Contents")
        .join("Resources")
        .join("cli")
        .join("bin")
        .join("cf");
    for (what, required) in [("native executable", &binary), ("a window's cf", &cf)] {
        if !required.is_file() {
            return Err(BundleError::Missing {
                what,
                path: required.clone(),
            });
        }
    }

    let version = version::canonical("bundle version", field(info::VERSION)?)?;
    let cf_version = cf_version(&cf, env)?;
    version::canonical("bundled cf's version", &cf_version)?;
    if version != cf_version {
        return Err(BundleError::VersionsDiffer {
            version: version.to_string(),
            cf: cf_version,
        });
    }
    Ok(Bundle {
        path,
        version: version.to_string(),
    })
}

/// What `cf --version` of the bundle says: the version compiled into it.
fn cf_version(cf: &Path, env: &Env) -> Result<String, BundleError> {
    let asked = process::capture(cf.as_os_str(), &[OsString::from("--version")], env)
        .map_err(|cause| BundleError::CfSilent(cause.to_string()))?;
    if asked.code != 0 {
        let said = js::trim(&asked.stderr);
        let ended = format!("it ended with code {}", asked.code);
        return Err(BundleError::CfSilent(if said.is_empty() {
            ended
        } else {
            format!("{ended}: {said}")
        }));
    }
    Ok(js::trim(&asked.stdout).to_string())
}
