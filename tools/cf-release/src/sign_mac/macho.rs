//! Which files of an app are code, told by their first bytes and not by their
//! names: a signer that went by names would pass over a binary kept with no
//! extension, as `cf` is, and take a script for what it is not.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::tools::fail;
use crate::cli::Failure;

/// The magic of a Mach-O of one architecture: 32 and 64 bits, in either order of
/// bytes.
const THIN: [u32; 4] = [0xfeed_face, 0xfeed_facf, 0xcefa_edfe, 0xcffa_edfe];

/// The magic of a universal binary, which a Java class file shares.
const UNIVERSAL: [u32; 2] = [0xcafe_babe, 0xcafe_babf];

/// Where a binary counts its slices and a class file has its version. A class
/// file's is 45 or more; no universal binary holds so many.
const FIRST_CLASS_VERSION: u32 = 45;

/// Whether the file at `path` is Mach-O code, thin or universal.
pub(super) fn is_mach_o(path: &Path) -> io::Result<bool> {
    let mut head = Vec::with_capacity(8);
    fs::File::open(path)?.take(8).read_to_end(&mut head)?;
    let [a, b, c, d, e, f, g, h] = head[..] else {
        return Ok(false);
    };
    let magic = u32::from_be_bytes([a, b, c, d]);
    let next = u32::from_be_bytes([e, f, g, h]);
    Ok(THIN.contains(&magic) || (UNIVERSAL.contains(&magic) && next < FIRST_CLASS_VERSION))
}

/// Every Mach-O file under `dir`, folder by folder, each folder's entries in the
/// order of their names so that a run signs in the same order every time. A link
/// is neither followed nor taken for a file: what it names is signed where it is.
pub(super) fn mach_os(dir: &Path) -> Result<Vec<PathBuf>, Failure> {
    let unreadable = |cause: io::Error| fail(format!("could not read {}: {cause}", dir.display()));
    let mut entries = fs::read_dir(dir)
        .and_then(|entries| entries.collect::<io::Result<Vec<_>>>())
        .map_err(unreadable)?;
    entries.sort_by_key(fs::DirEntry::file_name);
    let mut found = Vec::new();
    for entry in entries {
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|cause| fail(format!("could not read {}: {cause}", path.display())))?;
        if kind.is_dir() {
            found.extend(mach_os(&path)?);
        } else if kind.is_file()
            && is_mach_o(&path)
                .map_err(|cause| fail(format!("could not read {}: {cause}", path.display())))?
        {
            found.push(path);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests;
