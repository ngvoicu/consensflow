//! The door on Unix: every name opened relative to the descriptor of the folder
//! above it, never through a link, and a file without waiting on it.

use std::fs::File;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;
use std::rc::Rc;

use rustix::fs::{openat, statat, AtFlags, Dir, FileType, Mode, OFlags, CWD};
use rustix::io::Errno;

use super::{unnamed, Kind, Named, Opened, Reading, Refused};

/// How every name is opened: read only, never through a link, never left open
/// in a program the daemon starts, and never waited on (a FIFO, a device).
const OPENED: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::NOCTTY)
    .union(OFlags::CLOEXEC);

/// A folder of the specs folder, held open by its descriptor.
pub(in super::super) struct Held {
    fd: OwnedFd,
}

/// The names of a held folder, read as they are asked for.
pub(in super::super) struct Entries<'a> {
    folder: &'a OwnedFd,
    names: Dir,
}

impl Held {
    /// The project's own folder, at `path` as the human chose it: a link on its
    /// way there, or the folder itself one, is theirs, and followed. Unix lists
    /// no folder to take a name: the look (`_reading`) is the walk's alone.
    pub(in super::super) fn project(path: &Path, _reading: &Rc<Reading>) -> Result<Self, Refused> {
        let fd = openat(
            CWD,
            path,
            OFlags::RDONLY
                .union(OFlags::DIRECTORY)
                .union(OFlags::CLOEXEC),
            Mode::empty(),
        )
        .map_err(refused)?;
        Ok(Self { fd })
    }

    /// The folder `name` in this one, as itself: a link is refused, and so is
    /// anything else that is no folder.
    pub(in super::super) fn folder(&self, name: &str) -> Result<Self, Refused> {
        match self.kind(name)? {
            Kind::Folder => {}
            Kind::Link => return Err(Refused::Link),
            Kind::File { .. } => return Err(Refused::NotFolder),
            Kind::Other(what) => return Err(Refused::Other(what)),
        }
        // Swapped since it was looked at: a link or a file is refused here.
        let fd = openat(
            &self.fd,
            name,
            OPENED.union(OFlags::DIRECTORY),
            Mode::empty(),
        )
        .map_err(refused)?;
        Ok(Self { fd })
    }

    /// The folder `name` in this one, a name its own listing gave (a walk's):
    /// opened as any is.
    pub(in super::super) fn listed_folder(&self, name: &str) -> Result<Self, Refused> {
        self.folder(name)
    }

    /// The regular file `name` in this folder, opened for reading. It is looked
    /// at, then `between` is called (where a test swaps it), then it is opened,
    /// and what was opened is what is checked: a regular file of one name.
    pub(in super::super) fn file(&self, name: &str, between: &dyn Fn()) -> Result<Opened, Refused> {
        match self.kind(name)? {
            Kind::File { .. } => {}
            Kind::Folder => return Err(Refused::Folder),
            Kind::Link => return Err(Refused::Link),
            Kind::Other(what) => return Err(Refused::Other(what)),
        }
        between();
        let file = File::from(openat(&self.fd, name, OPENED, Mode::empty()).map_err(refused)?);
        let metadata = file.metadata()?;
        let kind = metadata.file_type();
        if kind.is_dir() {
            return Err(Refused::Folder);
        }
        if !kind.is_file() {
            return Err(Refused::Other(other_words(&kind)));
        }
        if metadata.nlink() != 1 {
            return Err(Refused::Names(metadata.nlink()));
        }
        Ok(Opened {
            file,
            size: metadata.len(),
        })
    }

    /// The names in this folder, read as they are asked for.
    pub(in super::super) fn entries(&self) -> io::Result<Entries<'_>> {
        Ok(Entries {
            folder: &self.fd,
            names: Dir::read_from(&self.fd)?,
        })
    }

    /// What `name` in this folder is, not following it.
    fn kind(&self, name: &str) -> Result<Kind, Refused> {
        let seen = statat(&self.fd, name, AtFlags::SYMLINK_NOFOLLOW).map_err(refused)?;
        Ok(kind_of(
            FileType::from_raw_mode(seen.st_mode),
            u64::try_from(seen.st_size).unwrap_or(0),
        ))
    }
}

impl Iterator for Entries<'_> {
    type Item = Named;

    /// The next name the folder holds and what it is; none for one passed
    /// over: `.` and `..`, a name that is no UTF-8 (the page could not name
    /// it), one gone since the folder was read.
    fn next(&mut self) -> Option<Self::Item> {
        let entry = match self.names.next()? {
            Ok(entry) => entry,
            Err(failed) => return Some(Err(failed.into())),
        };
        let Some(name) = entry
            .file_name()
            .to_str()
            .ok()
            .filter(|name| !unnamed(name))
        else {
            return Some(Ok(None));
        };
        if name == "." || name == ".." {
            return Some(Ok(None));
        }
        let name = name.to_owned();
        // A file's size, and what the folder did not say, are asked of the name itself.
        let kind = match entry.file_type() {
            FileType::Directory => Kind::Folder,
            FileType::Symlink => Kind::Link,
            _ => match statat(self.folder, name.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
                Ok(seen) => kind_of(
                    FileType::from_raw_mode(seen.st_mode),
                    u64::try_from(seen.st_size).unwrap_or(0),
                ),
                Err(Errno::NOENT) => return Some(Ok(None)),
                Err(failed) => return Some(Err(failed.into())),
            },
        };
        Some(Ok(Some((name, kind))))
    }
}

/// What a type of file is to the viewer.
fn kind_of(kind: FileType, size: u64) -> Kind {
    match kind {
        FileType::RegularFile => Kind::File { size },
        FileType::Directory => Kind::Folder,
        FileType::Symlink => Kind::Link,
        FileType::Fifo => Kind::Other("a FIFO"),
        FileType::Socket => Kind::Other("a socket"),
        FileType::CharacterDevice | FileType::BlockDevice => Kind::Other("a device"),
        _ => Kind::Other("neither a file nor a folder"),
    }
}

/// What an opened handle that is no regular file is, in words.
fn other_words(kind: &std::fs::FileType) -> &'static str {
    if kind.is_fifo() {
        "a FIFO"
    } else if kind.is_socket() {
        "a socket"
    } else if kind.is_char_device() || kind.is_block_device() {
        "a device"
    } else {
        "neither a file nor a folder"
    }
}

/// Why a name was not opened, from the system's error: a link met with
/// `O_NOFOLLOW` is `ELOOP`, a file met with `O_DIRECTORY` is `ENOTDIR`.
fn refused(errno: Errno) -> Refused {
    match errno {
        Errno::NOENT => Refused::Missing,
        Errno::LOOP => Refused::Link,
        Errno::NOTDIR => Refused::NotFolder,
        other => Refused::Io(other.into()),
    }
}
