//! The copies' folders on Unix: each opened by its descriptor in the folder
//! above it (`openat`, with `O_NOFOLLOW`), and what they hold read, removed and
//! written through those descriptors alone, never through a link.

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use rustix::fs::{mkdirat, openat, statat, unlinkat, AtFlags, Dir, FileType, Mode, OFlags, CWD};
use rustix::io::Errno;

use super::{Refused, Seen};

/// How a folder below the home is opened: as itself, never through a link.
const FOLDER: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
/// How deep the sweep empties a folder: a copy is two deep, and anything
/// deeper is no copy of the daemon's, and is left.
const DEEPEST: usize = 4;

/// A folder of the copies' way, held open by its descriptor, and its path.
pub(super) struct Folder {
    fd: OwnedFd,
    path: PathBuf,
}

impl Folder {
    /// ConsensFlow's home, at `path`, made where it is not there: a link on its
    /// way there, or the home itself one, is the human's, and followed.
    pub(super) fn home(path: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(path)?;
        let fd = openat(
            CWD,
            path,
            OFlags::RDONLY
                .union(OFlags::DIRECTORY)
                .union(OFlags::CLOEXEC),
            Mode::empty(),
        )?;
        Ok(Self {
            fd,
            path: path.to_path_buf(),
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// The folder `name` in this one, made for the user alone where it is not
    /// there, opened as itself.
    pub(super) fn child(&self, name: &str) -> Result<Self, Refused> {
        match mkdirat(&self.fd, name, Mode::RWXU) {
            Ok(()) | Err(Errno::EXIST) => self.opened(name),
            Err(failed) => Err(Refused::Io(failed.into())),
        }
    }

    /// The folder `name` in this one, made now: none where the name is taken.
    pub(super) fn new_child(&self, name: &str) -> Result<Option<Self>, Refused> {
        match mkdirat(&self.fd, name, Mode::RWXU) {
            Ok(()) => self.opened(name).map(Some),
            Err(Errno::EXIST) => Ok(None),
            Err(failed) => Err(Refused::Io(failed.into())),
        }
    }

    /// The folder `name` in this one, as itself: a link is refused, and so is
    /// what is no folder. Opened with `O_NOFOLLOW`, a link fails with `ELOOP`
    /// on Linux and `ENOTDIR` on macOS, so the name is looked at, not
    /// followed, to say which it is.
    fn opened(&self, name: &str) -> Result<Self, Refused> {
        let fd = openat(&self.fd, name, FOLDER, Mode::empty()).map_err(|failed| match failed {
            Errno::LOOP | Errno::NOTDIR => {
                match statat(&self.fd, name, AtFlags::SYMLINK_NOFOLLOW) {
                    Ok(seen) if FileType::from_raw_mode(seen.st_mode) == FileType::Symlink => {
                        Refused::Link
                    }
                    _ => Refused::NotFolder,
                }
            }
            other => Refused::Io(other.into()),
        })?;
        Ok(Self {
            fd,
            path: self.path.join(name),
        })
    }

    /// The names this folder holds, each as it is, not followed; a name that is
    /// no UTF-8 is none of the daemon's, and is left out.
    pub(super) fn names(&self) -> io::Result<Vec<(String, Seen)>> {
        let mut names = Vec::new();
        for entry in Dir::read_from(&self.fd)? {
            let entry = entry?;
            let Ok(name) = entry.file_name().to_str() else {
                continue;
            };
            if name == "." || name == ".." {
                continue;
            }
            let Ok(seen) = statat(&self.fd, name, AtFlags::SYMLINK_NOFOLLOW) else {
                continue;
            };
            names.push((
                name.to_owned(),
                Seen {
                    folder: FileType::from_raw_mode(seen.st_mode) == FileType::Directory,
                    written: Some(milliseconds(seen.st_mtime)),
                },
            ));
        }
        Ok(names)
    }

    /// Removes `name` (`seen` as it is) from this folder: a folder with what
    /// it holds, opened as itself; anything else, a link included, as itself.
    pub(super) fn remove(&self, name: &str, seen: &Seen) {
        self.removed(name, seen, 0);
    }

    fn removed(&self, name: &str, seen: &Seen, depth: usize) {
        if !seen.folder {
            let _ = unlinkat(&self.fd, name, AtFlags::empty());
            return;
        }
        if depth < DEEPEST {
            if let Ok(inner) = self.opened(name) {
                for (held, seen) in inner.names().unwrap_or_default() {
                    inner.removed(&held, &seen, depth + 1);
                }
            }
        }
        let _ = unlinkat(&self.fd, name, AtFlags::REMOVEDIR);
    }

    /// Writes `bytes` to a file `name` made new in this folder, for the user.
    pub(super) fn write_new(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let fd = openat(
            &self.fd,
            name,
            OFlags::WRONLY
                .union(OFlags::CREATE)
                .union(OFlags::EXCL)
                .union(OFlags::NOFOLLOW)
                .union(OFlags::CLOEXEC),
            Mode::RUSR.union(Mode::WUSR),
        )?;
        File::from(fd).write_all(bytes)
    }
}

/// `seconds` since 1970 in milliseconds, of whatever width and sign the system
/// keeps them in.
fn milliseconds(seconds: impl TryInto<i64>) -> i64 {
    seconds.try_into().unwrap_or(i64::MAX).saturating_mul(1000)
}
