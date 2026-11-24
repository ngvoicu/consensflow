//! The door's plain readers: a read of a path the run made, or of a place of the machine's that is
//! no store of the account's (K3, `live-kit-isolation-doors.md` section 1).
//!
//! The kit reads a file of the owner's only as a [`super::source::Source`], which the account makes
//! from a row of the table for a purpose. A path of the kit's own that resolves inside a protected
//! root, by any way it resolves, is refused here: so the one place a run reads a store is a source,
//! and a mistake that names a store by a path is an error at the read, not a read.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::Roots;

/// `path`, where it is no place a store's row protects: why not, where it is.
pub(super) fn admitted(path: &Path) -> io::Result<()> {
    if Roots::now().holds(path) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} is inside a store of the account's, which only a Source of the account reads",
                path.display()
            ),
        ));
    }
    Ok(())
}

/// The bytes of `path`.
pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    admitted(path.as_ref())?;
    fs::read(path)
}

/// The text of `path`.
pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    admitted(path.as_ref())?;
    fs::read_to_string(path)
}

/// `path` opened to read.
pub fn open(path: impl AsRef<Path>) -> io::Result<fs::File> {
    admitted(path.as_ref())?;
    fs::File::open(path)
}

/// The metadata of `path`, through a link.
pub fn metadata(path: impl AsRef<Path>) -> io::Result<fs::Metadata> {
    admitted(path.as_ref())?;
    fs::metadata(path)
}

/// The metadata of `path`, a link as itself.
pub fn symlink_metadata(path: impl AsRef<Path>) -> io::Result<fs::Metadata> {
    admitted(path.as_ref())?;
    fs::symlink_metadata(path)
}

/// Whether `path` is there, a link as itself: a place of a store is never said to be.
pub fn exists(path: impl AsRef<Path>) -> bool {
    symlink_metadata(path).is_ok()
}

/// Whether `path` is a file, through a link.
pub fn is_file(path: impl AsRef<Path>) -> bool {
    metadata(path).is_ok_and(|found| found.is_file())
}

/// Whether `path` is a folder, through a link.
pub fn is_dir(path: impl AsRef<Path>) -> bool {
    metadata(path).is_ok_and(|found| found.is_dir())
}

/// Whether `path` is a link.
pub fn is_symlink(path: impl AsRef<Path>) -> bool {
    symlink_metadata(path).is_ok_and(|found| found.file_type().is_symlink())
}

/// The real place `path` resolves to.
pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    admitted(path.as_ref())?;
    fs::canonicalize(path)
}

/// Where the link `path` leads, as the text it holds.
pub fn read_link(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    admitted(path.as_ref())?;
    fs::read_link(path)
}

/// The entries of the folder `path`, each as its path.
pub fn read_dir(path: impl AsRef<Path>) -> io::Result<Vec<PathBuf>> {
    admitted(path.as_ref())?;
    fs::read_dir(path)?
        .map(|entry| entry.map(|found| found.path()))
        .collect()
}
