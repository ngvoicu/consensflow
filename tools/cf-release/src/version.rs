//! The version the sources agree on. `package.json`, `app/src-tauri/tauri.conf.json`
//! and the root `Cargo.toml` each carry it, and a release goes on only when the
//! three say one and the same, as a canonical semantic version. This is
//! `sourceVersions` and `semver` of `app/scripts/prepare-update.mjs`, with their
//! rules and their words, so that the release's other steps (and `cargo xtask`)
//! ask this one thing of the sources. [`canonical`] is the form every version
//! the release handles must be in, the bundle's among them (`prepare-update`).

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;

use crate::args::Flags;
use crate::cli::{Command, Console, Failure};

/// `cf-release version`.
pub const COMMAND: Command = Command {
    name: "version",
    about: "Print the version package.json, Cargo.toml and tauri.conf.json agree on",
    usage: "[--repo DIR]",
    run,
};

/// The table of the root manifest that holds the version every crate takes.
const WORKSPACE_PACKAGE: &str = "[workspace.package]";

/// Why the sources have no one version.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VersionError {
    /// A source could not be read: `label` names it as the script did.
    #[error("could not read {label}: {cause}")]
    Unreadable { label: &'static str, cause: String },
    #[error("source Cargo.toml has no workspace package version")]
    NoWorkspaceVersion,
    /// A source has no version, or one that is not text.
    #[error("{label} must be a semantic version")]
    NotText { label: &'static str },
    #[error("{label} is not a canonical semantic version: {text}")]
    NotCanonical { label: &'static str, text: String },
    #[error("{label} has a leading-zero prerelease identifier")]
    LeadingZero { label: &'static str },
    /// Each file and what it says, so that the one to mend is plain.
    #[error(
        "source package, Cargo and Tauri versions do not match: \
         package.json {package}, Cargo.toml {cargo}, tauri.conf.json {tauri}"
    )]
    Disagree {
        package: String,
        cargo: String,
        tauri: String,
    },
}

/// The version the sources of the checkout at `root` agree on.
///
/// The files are read in the order the script read them (`package.json`, the
/// root `Cargo.toml`, the Tauri configuration) and the first that cannot be
/// read ends it; then each version is held to the canonical form, in that
/// order; then the three are held to each other.
pub fn source_version(root: &Path) -> Result<String, VersionError> {
    let package = json_version(&root.join("package.json"), "source package.json")?;
    let cargo = workspace_version_of(&root.join("Cargo.toml"))?;
    let tauri_conf: PathBuf = ["app", "src-tauri", "tauri.conf.json"]
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part));
    let tauri = json_version(&tauri_conf, "Tauri config")?;

    let package = text("source package.json version", package.as_deref())?;
    let cargo = canonical("source Cargo.toml version", &cargo)?;
    let tauri = text("source tauri.conf.json version", tauri.as_deref())?;
    if package == cargo && package == tauri {
        return Ok(package.to_string());
    }
    Err(VersionError::Disagree {
        package: package.to_string(),
        cargo: cargo.to_string(),
        tauri: tauri.to_string(),
    })
}

/// `file`'s text, or why it could not be read, said with the path.
fn read(file: &Path, label: &'static str) -> Result<String, VersionError> {
    fs::read_to_string(file).map_err(|cause| unreadable(label, file, &cause))
}

fn unreadable(label: &'static str, file: &Path, cause: &dyn std::fmt::Display) -> VersionError {
    VersionError::Unreadable {
        label,
        cause: format!("{}: {cause}", file.display()),
    }
}

/// The `version` a JSON file says, when it says one as text.
fn json_version(file: &Path, label: &'static str) -> Result<Option<String>, VersionError> {
    let json: serde_json::Value = serde_json::from_str(&read(file, label)?)
        .map_err(|cause| unreadable(label, file, &cause))?;
    Ok(json
        .get("version")
        .and_then(|version| version.as_str())
        .map(str::to_string))
}

fn workspace_version_of(file: &Path) -> Result<String, VersionError> {
    let manifest = read(file, "source Cargo.toml")?;
    workspace_version(&manifest)
        .map(str::to_string)
        .ok_or(VersionError::NoWorkspaceVersion)
}

/// The `version = "…"` of the manifest's `[workspace.package]` table, as the
/// script's pattern found it: a line of its own after the table's header, with
/// no `[` anywhere between the header and it (the next table, or an array,
/// ends the search).
fn workspace_version(manifest: &str) -> Option<&str> {
    let mut lines = manifest.lines();
    let header = lines.find(|line| line.starts_with(WORKSPACE_PACKAGE))?;
    if header[WORKSPACE_PACKAGE.len()..].contains('[') {
        return None;
    }
    for line in lines {
        if let Some(version) = version_value(line) {
            return Some(version);
        }
        if line.contains('[') {
            return None;
        }
    }
    None
}

/// `version = "1.2.3"` as a line says it: the text between the quotes.
fn version_value(line: &str) -> Option<&str> {
    let after_key = line.strip_prefix("version")?.trim_start();
    let after_equals = after_key.strip_prefix('=')?.trim_start();
    let text = after_equals.strip_prefix('"')?;
    let end = text.find('"')?;
    (end > 0).then(|| &text[..end])
}

/// What a source says its version is, when it says one as text.
fn text<'a>(label: &'static str, version: Option<&'a str>) -> Result<&'a str, VersionError> {
    canonical(label, version.ok_or(VersionError::NotText { label })?)
}

/// `version` if it is a canonical semantic version: three numbers without
/// leading zeros, then a prerelease of dotted identifiers if there is one.
/// Build metadata (`+…`) is not part of it, and a prerelease's number has no
/// leading zero. `label` says whose version it is, in the refusal.
pub fn canonical<'a>(label: &'static str, version: &'a str) -> Result<&'a str, VersionError> {
    let (core, prerelease) = match version.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (version, None),
    };
    let well_formed = is_core(core) && prerelease.is_none_or(is_prerelease);
    if !well_formed {
        return Err(VersionError::NotCanonical {
            label,
            text: version.to_string(),
        });
    }
    if prerelease.is_some_and(|prerelease| prerelease.split('.').any(has_leading_zero)) {
        return Err(VersionError::LeadingZero { label });
    }
    Ok(version)
}

/// The dotted identifiers of a canonical `version`'s prerelease: none for a
/// release, `alpha.83` for `3.0.0-alpha.83`.
pub fn prerelease(version: &str) -> Option<&str> {
    version.split_once('-').map(|(_, prerelease)| prerelease)
}

/// `major.minor.patch`: numbers with no leading zero (`0` itself is one).
fn is_core(core: &str) -> bool {
    let numbers: Vec<_> = core.split('.').collect();
    numbers.len() == 3
        && numbers.iter().all(|number| {
            !number.is_empty()
                && number.bytes().all(|byte| byte.is_ascii_digit())
                && (*number == "0" || !number.starts_with('0'))
        })
}

/// Dotted identifiers of letters, digits and hyphens, none empty.
fn is_prerelease(prerelease: &str) -> bool {
    prerelease.split('.').all(|identifier| {
        !identifier.is_empty()
            && identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

/// A numeric identifier of two digits or more that starts with `0`.
fn has_leading_zero(identifier: &str) -> bool {
    identifier.len() > 1
        && identifier.starts_with('0')
        && identifier.bytes().all(|byte| byte.is_ascii_digit())
}

fn run(_env: &Env, args: &[OsString], console: &mut Console) -> Result<(), Failure> {
    let root = Flags::read(args, &["repo"])?.repo()?;
    let version = source_version(&root).map_err(|cause| Failure::Failed(cause.to_string()))?;
    writeln!(console.out, "{version}")?;
    Ok(())
}

#[cfg(test)]
mod tests;
