//! The door to a project's specs folder: each name from the project's folder
//! down opened through the folder above it, never through a link, and a file
//! opened without waiting on it, so that what is read is the object that was
//! checked, not whatever a path names a moment later.
//!
//! The project's own folder is opened as the human chose it ([`Held::project`]):
//! a link on its way there is theirs. Every folder below it, the specs folder's
//! own names first, is opened in the folder above and held in its turn
//! ([`Held::folder`]), so the specs folder is found inside the project by that
//! walk, not by a path looked up once and opened later; a file is opened in the
//! folder that holds it ([`Held::file`]) and is read through that handle alone.
//! A name that is a link is refused wherever it is, as a folder or as a file,
//! and so is anything that is neither a folder nor a regular file (a FIFO, a
//! device, a socket), and a file with more than one name: another name of the
//! same file may be anywhere on its volume. A folder's names are read one at a
//! time, and every name it holds is one ([`Named`]), those passed over too (no
//! UTF-8, gone since): a walk counts each against its budget.
//!
//! On Unix every name is opened relative to the descriptor of the folder above
//! it (`openat`, with `O_NOFOLLOW`; a file with `O_NONBLOCK` too, so a FIFO
//! never waits for a writer), and the type and the names of the file are read
//! from its own descriptor. On Windows, which has no `openat` in std, each
//! folder is opened as itself, not as what a link or a junction leads to
//! (`FILE_FLAG_OPEN_REPARSE_POINT`), and held with a right that takes part in
//! sharing (`FILE_LIST_DIRECTORY`) and without letting it be renamed or deleted
//! (no `FILE_SHARE_DELETE`) while a name in it is opened by its path: no folder
//! on the way can be moved aside for a link meanwhile. What sharing cannot keep
//! out is a junction set on a held folder in place, once it is emptied: a
//! handle with `FILE_WRITE_ATTRIBUTES` alone sets one, and such a handle takes
//! no part in sharing. Only opening each name relative to the handle of the
//! folder above (`NtCreateFile`) would close that, and it is unsafe code, which
//! this crate forbids: it is a race the viewer's threat model accepts
//! ([`viewer`](super)). The file's handle says it is on a disk, a file, and of
//! one name, and lets the file be replaced meanwhile (`FILE_SHARE_DELETE`), so
//! an editor saving it never fails for the viewer's read. And a name the page
//! writes is taken in a folder only where it is one of the long names the
//! folder lists, exactly as written: Windows answers to a short (8.3) name too,
//! which may be any name, given by hand, and a dotfile's. A folder is listed at
//! most once a look ([`Reading`]), each name of it charged to the look's budget.

use std::cell::Cell;
use std::fs::File;
use std::io;
use std::rc::Rc;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(super) use unix::Held;
#[cfg(windows)]
pub(super) use windows::Held;

/// One look at a specs folder (a tree, whether it holds a file, a file, an HTML
/// file with its assets), shared by every folder held in it: what is left of
/// its budget of names, which a walk takes each name it reads from and, on
/// Windows, every listing read for the names a folder holds; and on Windows,
/// each folder's long names as last listed, and in which look, so a folder is
/// listed at most once a look however many names are asked of it.
pub(super) struct Reading {
    left: Cell<usize>,
    #[cfg(windows)]
    look: Cell<u64>,
    #[cfg(windows)]
    listed: std::cell::RefCell<std::collections::HashMap<std::path::PathBuf, windows::Listed>>,
    /// How many listings were read, for a test to count.
    #[cfg(all(test, windows))]
    listings: Cell<usize>,
}

impl Reading {
    /// A reading of no look yet: [`begin`](Self::begin) gives it its budget.
    pub(super) fn new() -> Rc<Self> {
        Rc::new(Self {
            left: Cell::new(0),
            #[cfg(windows)]
            look: Cell::new(0),
            #[cfg(windows)]
            listed: std::cell::RefCell::default(),
            #[cfg(all(test, windows))]
            listings: Cell::new(0),
        })
    }

    /// A new look begins, which may read `budget` names.
    pub(super) fn begin(&self, budget: usize) {
        self.left.set(budget);
        #[cfg(windows)]
        self.look.set(self.look.get() + 1);
    }

    /// One name read: whether the budget had one left for it.
    pub(super) fn charge(&self) -> bool {
        match self.left.get() {
            0 => false,
            left => {
                self.left.set(left - 1);
                true
            }
        }
    }

    /// What is left of the budget of the look.
    #[cfg(all(test, windows))]
    pub(super) fn left(&self) -> usize {
        self.left.get()
    }
}

/// What a name in a folder is, as the folder says it, not following it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    File {
        size: u64,
    },
    Folder,
    Link,
    /// Neither a regular file nor a folder: what it is, in words (`a FIFO`).
    Other(&'static str),
}

/// One name a folder holds, as its listing gives it: none for one passed over
/// (`.` and `..`, a name that is no UTF-8, which the page could not name, or
/// one gone since the folder was read), which is a name read all the same.
pub(super) type Named = io::Result<Option<(String, Kind)>>;

/// Why a name was not opened.
#[derive(Debug)]
pub(super) enum Refused {
    Missing,
    Link,
    Folder,
    NotFolder,
    /// Neither a regular file nor a folder: what it is, in words.
    Other(&'static str),
    /// A file with this many names.
    Names(u64),
    Io(io::Error),
}

impl From<io::Error> for Refused {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::Missing,
            _ => Self::Io(error),
        }
    }
}

/// A regular file of the folder, open for reading, and its size.
pub(super) struct Opened {
    pub(super) file: File,
    pub(super) size: u64,
}

#[cfg(test)]
thread_local! {
    /// Names a test has the door take as no UTF-8, which a Mac's disk never
    /// holds: each is passed over as such a name is, and read all the same.
    pub(super) static UNNAMED: std::cell::Cell<fn(&str) -> bool> =
        const { std::cell::Cell::new(named as fn(&str) -> bool) };
}

/// Every name is a name, but where a test says otherwise.
#[cfg(test)]
pub(super) fn named(_: &str) -> bool {
    false
}

/// Whether a test has the door take `name` as no UTF-8.
#[cfg(test)]
fn unnamed(name: &str) -> bool {
    UNNAMED.with(|unnamed| unnamed.get()(name))
}

#[cfg(not(test))]
fn unnamed(_: &str) -> bool {
    false
}

#[cfg(test)]
mod tests;
