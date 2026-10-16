//! The door on Windows: each folder from the project's down opened as itself,
//! not as what a link or a junction leads to, and held with a right that takes
//! part in sharing, without letting it be renamed or deleted while a name in it
//! is opened, so no folder on the way is moved aside meanwhile; a file is opened
//! as itself too, and its handle says it is a file on a disk, of one name.
//!
//! Windows answers to a file's short (8.3) name as to its long one, and a short
//! name may be any name, given by the volume (`GIT~1`) or by hand (`fsutil file
//! setshortname`, `PUBLIC.CSS` for `.private.css`): a dotfile, which the viewer
//! leaves out, would be read by it. So a name the page writes is taken in a
//! folder only where it is one of the long names the folder lists, exactly as
//! written, case and all: Windows matches names whatever their case by rules of
//! its own (it keeps `ß.css` and `SS.CSS` apart), which no folding here would
//! follow, and the page asks for the names the tree listed, and an HTML file's
//! assets as it writes them. A folder's listing is kept for the names it holds,
//! and read again, at most once a look ([`Reading`]), for one it does not hold,
//! so a file written into the folder since is found as on Unix; each name a
//! listing reads is taken from the look's budget.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::{unnamed, Kind, Named, Opened, Reading, Refused};

/// A folder's long names as a look listed them, and which look that was.
pub(in super::super) struct Listed {
    names: HashSet<String>,
    look: u64,
}

impl Reading {
    /// Whether the folder at `path` lists `name`, exactly as written. A name
    /// its listing as last read holds is taken; one it does not hold is looked
    /// for in the listing read again, unless this look read it already: a
    /// folder is listed at most once a look, and each name read is taken from
    /// the look's budget, so a listing reads no further than it allows.
    fn lists(&self, path: &Path, name: &str) -> io::Result<bool> {
        let look = self.look.get();
        if let Some(listed) = self.listed.borrow().get(path) {
            if listed.names.contains(name) {
                return Ok(true);
            }
            if listed.look == look {
                return Ok(false);
            }
        }
        #[cfg(test)]
        self.listings.set(self.listings.get() + 1);
        let mut names = HashSet::new();
        for entry in fs::read_dir(path)? {
            if !self.charge() {
                break;
            }
            names.insert(entry?.file_name().to_string_lossy().into_owned());
        }
        let found = names.contains(name);
        self.listed
            .borrow_mut()
            .insert(path.to_path_buf(), Listed { names, look });
        Ok(found)
    }

    /// How many listings were read.
    #[cfg(test)]
    pub(in super::super) fn listings(&self) -> usize {
        self.listings.get()
    }
}

/// What a held folder is opened for: listing it, a right that takes part in
/// sharing (one for its attributes alone takes none, and keeps nothing out).
const LISTED: u32 = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES;
/// What others may do with a held folder: read it, and write in it (a file
/// saved there goes on), but not rename or delete it (no `FILE_SHARE_DELETE`).
const FOLDER_SHARED: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;
/// What others may do with a file being read: anything, its replacement by an
/// editor's save included; the handle is the file that was checked, whatever
/// its name holds next.
const FILE_SHARED: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// A folder of the project, held open: its path, the handle that keeps it from
/// being renamed or deleted while it is held, and the look it is held in,
/// which keeps every folder's listing.
pub(in super::super) struct Held {
    path: PathBuf,
    _held: File,
    reading: Rc<Reading>,
}

/// The names of a held folder, read as they are asked for.
pub(in super::super) struct Entries<'a> {
    names: fs::ReadDir,
    _folder: std::marker::PhantomData<&'a Held>,
}

impl Held {
    /// The project's own folder, at `path` as the human chose it, held in the
    /// look `reading`: a link or a junction on its way there, or the folder
    /// itself one, is theirs, and followed.
    pub(in super::super) fn project(path: &Path, reading: &Rc<Reading>) -> Result<Self, Refused> {
        let held = OpenOptions::new()
            .access_mode(LISTED)
            .share_mode(FOLDER_SHARED)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        if !held.metadata()?.is_dir() {
            return Err(Refused::NotFolder);
        }
        Ok(Self {
            path: path.to_path_buf(),
            _held: held,
            reading: Rc::clone(reading),
        })
    }

    /// Whether `name`, as the page writes it, is one of the long names this
    /// folder lists, exactly: a short name is none of them, nor a name in
    /// another case, and either is taken for a name the folder does not hold.
    fn lists(&self, name: &str) -> Result<(), Refused> {
        if self.reading.lists(&self.path, name)? {
            Ok(())
        } else {
            Err(Refused::Missing)
        }
    }

    /// The folder `name` in this one, as itself: a link or a junction is
    /// refused, and so is anything else that is no folder, and a name the
    /// folder does not list (a short name, a name in another case).
    pub(in super::super) fn folder(&self, name: &str) -> Result<Self, Refused> {
        self.lists(name)?;
        self.listed_folder(name)
    }

    /// The folder `name` in this one, a name its own listing gave (a walk's):
    /// one of its long names already, and opened as [`folder`](Self::folder)
    /// opens one.
    pub(in super::super) fn listed_folder(&self, name: &str) -> Result<Self, Refused> {
        let path = self.path.join(name);
        let held = OpenOptions::new()
            .access_mode(LISTED)
            .share_mode(FOLDER_SHARED)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)?;
        let metadata = held.metadata()?;
        if metadata.file_type().is_symlink() {
            return Err(Refused::Link);
        }
        if !metadata.is_dir() {
            return Err(Refused::NotFolder);
        }
        Ok(Self {
            path,
            _held: held,
            reading: Rc::clone(&self.reading),
        })
    }

    /// The regular file `name` in this folder, opened for reading: a name the
    /// folder does not list as written (a short name, a name in another case)
    /// is none. It is looked at, then
    /// `between` is called (where a test swaps it), then it is opened as
    /// itself, and what was opened is what is checked.
    pub(in super::super) fn file(&self, name: &str, between: &dyn Fn()) -> Result<Opened, Refused> {
        self.lists(name)?;
        let path = self.path.join(name);
        match kind_of(&fs::symlink_metadata(&path)?) {
            Kind::File { .. } => {}
            Kind::Folder => return Err(Refused::Folder),
            Kind::Link => return Err(Refused::Link),
            Kind::Other(what) => return Err(Refused::Other(what)),
        }
        between();
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARED)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)?;
        if !winapi_util::file::typ(&file)?.is_disk() {
            return Err(Refused::Other("a device or a pipe"));
        }
        let metadata = file.metadata()?;
        match kind_of(&metadata) {
            Kind::File { .. } => {}
            Kind::Folder => return Err(Refused::Folder),
            Kind::Link => return Err(Refused::Link),
            Kind::Other(what) => return Err(Refused::Other(what)),
        }
        let names = winapi_util::file::information(&file)?.number_of_links();
        if names != 1 {
            return Err(Refused::Names(names));
        }
        Ok(Opened {
            file,
            size: metadata.len(),
        })
    }

    /// The names in this folder, read as they are asked for.
    pub(in super::super) fn entries(&self) -> io::Result<Entries<'_>> {
        Ok(Entries {
            names: fs::read_dir(&self.path)?,
            _folder: std::marker::PhantomData,
        })
    }
}

impl Iterator for Entries<'_> {
    type Item = Named;

    /// The next name the folder holds and what it is, as its own listing says
    /// it (not following a link); none for one passed over: a name that is no
    /// UTF-8, one gone since the folder was read.
    fn next(&mut self) -> Option<Self::Item> {
        let entry = match self.names.next()? {
            Ok(entry) => entry,
            Err(failed) => return Some(Err(failed)),
        };
        let Some(name) = entry
            .file_name()
            .into_string()
            .ok()
            .filter(|name| !unnamed(name))
        else {
            return Some(Ok(None));
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(failed) if failed.kind() == io::ErrorKind::NotFound => return Some(Ok(None)),
            Err(failed) => return Some(Err(failed)),
        };
        Some(Ok(Some((name, kind_of(&metadata)))))
    }
}

/// What a name is, from what Windows says of it without following it: a link
/// or a junction is a link (std's `is_symlink`, a name-surrogate reparse point;
/// a placeholder of a synced folder is not one).
fn kind_of(metadata: &fs::Metadata) -> Kind {
    let kind = metadata.file_type();
    if kind.is_symlink() {
        Kind::Link
    } else if kind.is_dir() {
        Kind::Folder
    } else if kind.is_file() {
        Kind::File {
            size: metadata.len(),
        }
    } else {
        Kind::Other("neither a file nor a folder")
    }
}
