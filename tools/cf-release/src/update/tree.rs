//! The files of an app as an update must hold them. The archive that installed
//! apps unpack is the bundle the release built, byte for byte: so the bundle on
//! disk and the archive are each read into a [`Tree`], every path under
//! `ConsensFlow.app` with what it is, and the two must be one and the same.
//! What a path is: a folder or a file; its permission bits, the special ones
//! included (an archive that sets a bit its bundle does not have is not that
//! bundle); a file's size and its SHA-256. Owners and times are not compared:
//! an archive does not keep the first and installing sets the second.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

/// The folder every path of an update is under, and its name in the archive
/// whatever the bundle's own folder is called.
pub const ROOT: &str = "ConsensFlow.app";

/// The permission bits a mode is held to: read, write and run for the owner,
/// the group and the others, and the set-user-id, set-group-id and sticky bits.
pub const PERMISSION_BITS: u32 = 0o7777;

/// What a path of the app is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File { size: u64, sha256: String },
}

/// A path of the app: what it is and its permission bits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    pub mode: u32,
}

/// Every path of an app by its name inside the archive, `ConsensFlow.app` and
/// then `/` between the names of the folders and the file.
pub type Tree = BTreeMap<String, Entry>;

/// A bundle that cannot be held to an archive.
#[derive(Debug, thiserror::Error)]
pub enum TreeError {
    #[error("the app bundle contains a symlink: {0}")]
    Symlink(String),
    #[error("the app bundle contains a special file: {0}")]
    Special(String),
    #[error("the app bundle contains a name that is not text: {0}")]
    NotText(String),
    #[error("could not read the app bundle: {path}: {cause}")]
    Unreadable { path: String, cause: io::Error },
    /// A system that keeps no permission bits has nothing to hold to the archive's.
    #[cfg(not(unix))]
    #[error("the app bundle's file modes are read on macOS: this system keeps none")]
    NoModes,
}

/// The tree of the bundle in the folder `root`, whose name in the archive is [`ROOT`].
pub fn of_bundle(root: &Path) -> Result<Tree, TreeError> {
    let mut tree = Tree::new();
    visit(root, ROOT, &mut tree)?;
    Ok(tree)
}

fn visit(path: &Path, name: &str, tree: &mut Tree) -> Result<(), TreeError> {
    let unreadable = |cause| TreeError::Unreadable {
        path: path.display().to_string(),
        cause,
    };
    let metadata = fs::symlink_metadata(path).map_err(unreadable)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(TreeError::Symlink(name.to_string()));
    }
    let mode = permission_bits(&metadata)?;
    if file_type.is_dir() {
        tree.insert(
            name.to_string(),
            Entry {
                kind: Kind::Dir,
                mode,
            },
        );
        let mut children: Vec<OsString> = fs::read_dir(path)
            .and_then(|children| {
                children
                    .map(|child| child.map(|child| child.file_name()))
                    .collect()
            })
            .map_err(unreadable)?;
        children.sort_unstable();
        for child in children {
            visit(&path.join(&child), &named(name, &child)?, tree)?;
        }
    } else if file_type.is_file() {
        let mut file = File::open(path).map_err(unreadable)?;
        let (size, sha256) = digest(&mut file).map_err(unreadable)?;
        tree.insert(
            name.to_string(),
            Entry {
                kind: Kind::File { size, sha256 },
                mode,
            },
        );
    } else {
        return Err(TreeError::Special(name.to_string()));
    }
    Ok(())
}

/// The name inside the archive of `child` in the folder called `parent` there.
fn named(parent: &str, child: &OsStr) -> Result<String, TreeError> {
    child
        .to_str()
        .map(|child| format!("{parent}/{child}"))
        .ok_or_else(|| TreeError::NotText(format!("{parent}/{}", child.to_string_lossy())))
}

/// The permission bits of a file as `stat` says them.
#[cfg(unix)]
fn permission_bits(metadata: &Metadata) -> Result<u32, TreeError> {
    use std::os::unix::fs::PermissionsExt;
    Ok(metadata.permissions().mode() & PERMISSION_BITS)
}

#[cfg(not(unix))]
fn permission_bits(_: &Metadata) -> Result<u32, TreeError> {
    Err(TreeError::NoModes)
}

/// How many bytes `reader` holds to its end, and their SHA-256 as lower-case hex.
pub fn digest(reader: &mut impl Read) -> io::Result<(u64, String)> {
    let mut hash = Sha256::new();
    let size = io::copy(reader, &mut hash)?;
    let hex = hash.finalize().iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    });
    Ok((size, hex))
}

/// The first path, in the order of the names, where the archive's tree is not
/// the bundle's, and how: none when they are one and the same.
pub fn first_difference(bundle: &Tree, archive: &Tree) -> Option<String> {
    let names: BTreeSet<&String> = bundle.keys().chain(archive.keys()).collect();
    names
        .into_iter()
        .find_map(|name| match (bundle.get(name), archive.get(name)) {
            (Some(in_bundle), Some(in_archive)) if in_bundle == in_archive => None,
            (Some(in_bundle), Some(in_archive)) => Some(format!(
                "{name} differs: {}",
                how_they_differ(in_bundle, in_archive)
            )),
            (Some(_), None) => Some(format!("{name} is in the bundle and not in the archive")),
            (None, Some(_)) => Some(format!("{name} is in the archive and not in the bundle")),
            (None, None) => None,
        })
}

/// What sets two entries of one name apart, the most plain of it first: a
/// folder or a file, the mode, the size, the bytes.
fn how_they_differ(bundle: &Entry, archive: &Entry) -> String {
    match (&bundle.kind, &archive.kind) {
        (Kind::Dir, Kind::File { .. }) => "a folder in the bundle, a file in the archive".into(),
        (Kind::File { .. }, Kind::Dir) => "a file in the bundle, a folder in the archive".into(),
        _ if bundle.mode != archive.mode => format!(
            "mode {:o} in the bundle, {:o} in the archive",
            bundle.mode, archive.mode
        ),
        (Kind::File { size: bundled, .. }, Kind::File { size: archived, .. })
            if bundled != archived =>
        {
            format!("{bundled} bytes in the bundle, {archived} in the archive")
        }
        _ => "the bytes differ".into(),
    }
}

#[cfg(test)]
mod tests;
