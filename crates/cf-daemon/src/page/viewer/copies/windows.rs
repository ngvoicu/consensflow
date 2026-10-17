//! The copies' folders on Windows, which has no `openat` in std: each opened as
//! itself, not as what a link or a junction leads to, refused where it is any
//! reparse point, and held while what it holds is read, removed and written by
//! its path, so it cannot be moved aside meanwhile.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::{Refused, Seen};

/// What a held folder is opened for: listing it, a right that takes part in
/// sharing.
const LISTED: u32 = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES;
/// What others may do with a held folder: read it and write in it, not rename
/// or delete it.
const SHARED: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;

/// A folder of the copies' way, held open, and its path.
pub(super) struct Folder {
    path: PathBuf,
    _held: File,
}

impl Folder {
    /// ConsensFlow's home, at `path`, made where it is not there: a link or a
    /// junction on its way there, or the home itself one, is the human's, and
    /// followed.
    pub(super) fn home(path: &Path) -> io::Result<Self> {
        fs::create_dir_all(path)?;
        let held = OpenOptions::new()
            .access_mode(LISTED)
            .share_mode(SHARED)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            _held: held,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// The folder `name` in this one, made where it is not there, opened as
    /// itself.
    pub(super) fn child(&self, name: &str) -> Result<Self, Refused> {
        match fs::create_dir(self.path.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(Refused::Io(error)),
        }
        self.opened(name)
    }

    /// The folder `name` in this one, made now: none where the name is taken.
    pub(super) fn new_child(&self, name: &str) -> Result<Option<Self>, Refused> {
        match fs::create_dir(self.path.join(name)) {
            Ok(()) => self.opened(name).map(Some),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
            Err(error) => Err(Refused::Io(error)),
        }
    }

    /// The folder `name` in this one, as itself and held: any reparse point
    /// (a link, a junction) is refused, and so is what is no folder.
    fn opened(&self, name: &str) -> Result<Self, Refused> {
        let path = self.path.join(name);
        let held = OpenOptions::new()
            .access_mode(LISTED)
            .share_mode(SHARED)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)?;
        let metadata = held.metadata()?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Refused::Link);
        }
        if !metadata.is_dir() {
            return Err(Refused::NotFolder);
        }
        Ok(Self { path, _held: held })
    }

    /// The names this folder holds, each as it is, not followed: a reparse
    /// point is no folder here, whatever it leads to.
    pub(super) fn names(&self) -> io::Result<Vec<(String, Seen)>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let reparse = metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
            let written = metadata
                .modified()
                .ok()
                .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
                .and_then(|since| i64::try_from(since.as_millis()).ok());
            names.push((
                name,
                Seen {
                    folder: metadata.is_dir() && !reparse,
                    written,
                },
            ));
        }
        Ok(names)
    }

    /// Removes `name` (`seen` as it is) from this folder: a folder with what it
    /// holds (std removes a link or a junction in it as itself); anything else,
    /// a link or a junction included, as itself.
    pub(super) fn remove(&self, name: &str, seen: &Seen) {
        let path = self.path.join(name);
        let _ = if seen.folder {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path).or_else(|_| fs::remove_dir(&path))
        };
    }

    /// Writes `bytes` to a file `name` made new in this folder.
    pub(super) fn write_new(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.path.join(name))?
            .write_all(bytes)
    }
}
