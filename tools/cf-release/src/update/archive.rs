//! The update archive, `ConsensFlow_<version>_aarch64.app.tar.gz`: the file
//! installed apps download and unpack. It is read here without being unpacked
//! (nothing of it is ever written to disk) and must be the bundle the release
//! built: the same paths, each the same kind of entry with the same mode, size
//! and bytes (see [`tree`]), and nothing in it that unpacking could use against
//! the machine: a path outside `ConsensFlow.app`, a path twice, a link, a device.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::Path;

use flate2::read::GzDecoder;
use tar::Archive;

use super::bundle::Bundle;
use super::info::{self, Info};
use super::tree::{self, Entry, Kind, Tree, TreeError, PERMISSION_BITS, ROOT};
use crate::version::{self, VersionError};

/// The bundle's `Info.plist` inside the archive.
const INFO_PLIST: &str = "ConsensFlow.app/Contents/Info.plist";

/// The most of an `Info.plist` that is read into memory: an archive that holds a
/// larger one has not an app's.
const INFO_PLIST_LIMIT: u64 = 1 << 20;

/// An archive the release will not publish.
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("could not read archive: {0}")]
    Unreadable(String),
    #[error("archive filename must be a safe, versioned .app.tar.gz asset")]
    Filename,
    #[error("archive safety/content check failed: {0}")]
    Check(#[from] Finding),
    #[error(transparent)]
    Version(#[from] VersionError),
    #[error("archive packaged version does not match the bundle")]
    VersionMismatch,
}

/// What the check of the archive and the bundle found.
#[derive(Debug, thiserror::Error)]
pub enum Finding {
    #[error(transparent)]
    Bundle(#[from] TreeError),
    #[error("could not read the archive: {0}")]
    Unreadable(#[from] io::Error),
    #[error("the update archive contains an unsafe path: {0}")]
    UnsafePath(String),
    #[error("the update archive contains a duplicate path: {0}")]
    Duplicate(String),
    #[error("the update archive contains a symlink or hard link: {0}")]
    Link(String),
    #[error("the update archive contains a special entry: {0}")]
    Special(String),
    #[error("the update archive has no ConsensFlow.app root")]
    NoRoot,
    #[error("the update archive has no readable ConsensFlow.app/Contents/Info.plist")]
    NoInfo,
    #[error("the archive has no readable packaged version")]
    NoVersion,
    #[error("the archive content manifest does not match the supplied bundle: {0}")]
    Mismatch(String),
}

/// Holds the archive at `archive` to the `bundle` it was made from, and answers
/// its file name, the name of the asset the feed's URL ends with.
///
/// The archive must not be empty, must be named for the bundle's version, must
/// hold the bundle and nothing else, and must say in its own `Info.plist` the
/// version of the bundle.
pub fn inspect(archive: &Path, bundle: &Bundle) -> Result<String, ArchiveError> {
    let size = fs::metadata(archive)
        .map_err(|cause| unreadable(archive, &cause))?
        .len();
    if size == 0 {
        return Err(ArchiveError::Unreadable("archive is empty".into()));
    }
    let asset = asset_name(archive, &bundle.version)?;

    let expected = tree::of_bundle(&bundle.path).map_err(Finding::from)?;
    let found = scan(archive)?;
    if !found.tree.contains_key(ROOT) {
        return Err(Finding::NoRoot.into());
    }
    let info = found
        .info
        .as_deref()
        .and_then(Info::parse)
        .ok_or(Finding::NoInfo)?;
    let packaged = info.text(info::VERSION).ok_or(Finding::NoVersion)?;
    version::canonical("archived bundle version", packaged)?;
    if packaged != bundle.version {
        return Err(ArchiveError::VersionMismatch);
    }
    if let Some(difference) = tree::first_difference(&expected, &found.tree) {
        return Err(Finding::Mismatch(difference).into());
    }
    Ok(asset)
}

fn unreadable(path: &Path, cause: &io::Error) -> ArchiveError {
    ArchiveError::Unreadable(format!("{}: {cause}", path.display()))
}

/// The file's name, if it is one a release asset can have: letters, digits,
/// `.`, `_` and `-`, ending in `.app.tar.gz`, and naming the version.
fn asset_name(archive: &Path, version: &str) -> Result<String, ArchiveError> {
    let name = archive.file_name().and_then(OsStr::to_str).unwrap_or("");
    let safe = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if safe && name.ends_with(".app.tar.gz") && name.contains(version) {
        Ok(name.to_string())
    } else {
        Err(ArchiveError::Filename)
    }
}

/// What an archive holds: its tree, and the bytes of its `Info.plist`.
struct Scan {
    tree: Tree,
    info: Option<Vec<u8>>,
}

/// Reads the archive from its first entry to its last.
fn scan(path: &Path) -> Result<Scan, Finding> {
    let file = File::open(path)?;
    let mut archive = Archive::new(GzDecoder::new(BufReader::new(file)));
    let mut scan = Scan {
        tree: Tree::new(),
        info: None,
    };
    for entry in archive.entries()? {
        scan.add(&mut entry?)?;
    }
    Ok(scan)
}

impl Scan {
    /// Adds the entry the archive is at: its path, what it is, and its mode.
    fn add<R: Read>(&mut self, entry: &mut tar::Entry<'_, R>) -> Result<(), Finding> {
        let raw = entry.path_bytes().into_owned();
        let Some(name) = std::str::from_utf8(&raw).ok().and_then(inside_root) else {
            return Err(Finding::UnsafePath(
                String::from_utf8_lossy(&raw).into_owned(),
            ));
        };
        if self.tree.contains_key(&name) {
            return Err(Finding::Duplicate(name));
        }
        let kind = entry.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            return Err(Finding::Link(name));
        }
        let mode = entry.header().mode()? & PERMISSION_BITS;
        let kind = if kind.is_dir() {
            Kind::Dir
        } else if kind.is_file() {
            let (size, sha256) = self.digest(&name, entry)?;
            Kind::File { size, sha256 }
        } else {
            return Err(Finding::Special(name));
        };
        self.tree.insert(name, Entry { kind, mode });
        Ok(())
    }

    /// The size and the SHA-256 of a file's bytes, keeping them if it is the `Info.plist`.
    fn digest<R: Read>(
        &mut self,
        name: &str,
        entry: &mut tar::Entry<'_, R>,
    ) -> Result<(u64, String), Finding> {
        if name != INFO_PLIST || entry.size() > INFO_PLIST_LIMIT {
            return Ok(tree::digest(entry)?);
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        let digested = tree::digest(&mut bytes.as_slice())?;
        self.info = Some(bytes);
        Ok(digested)
    }
}

/// The name `raw` gives an entry inside the archive, if it is a path that stays
/// under [`ROOT`]: not absolute, no `..`, and starting at the root. Slashes
/// after the last name, empty names and `.` names are no part of it
/// (`./ConsensFlow.app//Contents/` is `ConsensFlow.app/Contents`).
fn inside_root(raw: &str) -> Option<String> {
    if raw.starts_with('/') {
        return None;
    }
    let parts: Vec<&str> = raw
        .trim_end_matches('/')
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if parts.first() != Some(&ROOT) || parts.contains(&"..") {
        return None;
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_under_the_root_is_named_as_the_archive_means_it() {
        for (raw, name) in [
            ("ConsensFlow.app", "ConsensFlow.app"),
            ("ConsensFlow.app/", "ConsensFlow.app"),
            ("ConsensFlow.app//", "ConsensFlow.app"),
            (
                "ConsensFlow.app/Contents/Info.plist",
                "ConsensFlow.app/Contents/Info.plist",
            ),
            ("./ConsensFlow.app/Contents/", "ConsensFlow.app/Contents"),
            (
                "ConsensFlow.app//Contents/./MacOS",
                "ConsensFlow.app/Contents/MacOS",
            ),
            (
                "ConsensFlow.app/Contents/..hidden",
                "ConsensFlow.app/Contents/..hidden",
            ),
            ("ConsensFlow.app/a b/ü", "ConsensFlow.app/a b/ü"),
        ] {
            assert_eq!(inside_root(raw).as_deref(), Some(name), "{raw:?}");
        }
    }

    #[test]
    fn a_path_that_leaves_the_root_or_never_was_under_it_is_unsafe() {
        for raw in [
            "",
            ".",
            "./",
            "/",
            "..",
            "../evil.txt",
            "/tmp/cf-evil.txt",
            "//ConsensFlow.app/Contents",
            "/ConsensFlow.app",
            "ConsensFlow.app/..",
            "ConsensFlow.app/../evil",
            "ConsensFlow.app/Contents/../../evil",
            "ConsensFlow.app/../ConsensFlow.app",
            "Other.app/Contents",
            "consensflow.app",
            "ConsensFlow.app.evil/x",
            "Contents/Info.plist",
            "evil/ConsensFlow.app",
            "../ConsensFlow.app",
        ] {
            assert_eq!(inside_root(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn a_filename_is_a_safe_asset_name_that_ends_as_an_app_archive_and_names_the_version() {
        let accepted = |path: &str| asset_name(Path::new(path), "3.0.0-alpha.99").is_ok();
        assert!(accepted("ConsensFlow_3.0.0-alpha.99_aarch64.app.tar.gz"));
        assert!(accepted("3.0.0-alpha.99.app.tar.gz"));
        // The asset is the file's name, not the path to it.
        assert_eq!(
            asset_name(
                Path::new("/some/where/ConsensFlow_3.0.0-alpha.99_aarch64.app.tar.gz"),
                "3.0.0-alpha.99"
            )
            .unwrap(),
            "ConsensFlow_3.0.0-alpha.99_aarch64.app.tar.gz"
        );
        // The version, its ending, and the letters a name may have.
        assert!(!accepted("ConsensFlow-latest.app.tar.gz"));
        assert!(!accepted("ConsensFlow_3.0.0-alpha.98_aarch64.app.tar.gz"));
        assert!(!accepted("ConsensFlow_3.0.0-alpha.99_aarch64.tar.gz"));
        assert!(!accepted("ConsensFlow_3.0.0-alpha.99_aarch64.app.tgz"));
        assert!(!accepted("ConsensFlow 3.0.0-alpha.99.app.tar.gz"));
        assert!(!accepted("ConsensFlow_3.0.0-alpha.99_é.app.tar.gz"));
        assert!(!accepted("ConsensFlow_3.0.0-alpha.99;rm.app.tar.gz"));
        assert!(!accepted(""));
        assert!(!accepted("/"));
    }
}
