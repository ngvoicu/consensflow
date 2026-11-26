//! The only way the kit reads the owner's files (K3, `live-kit-isolation-doors.md` section 1).
//!
//! The account's stores (`live::stores`) are the owner's: logins, settings, records, a harness's
//! own program. A live run reads a few of them, each for a reason the design names: it copies a
//! login or a config into the run's own home (**seed**), it copies a marketplace or a provider's
//! package a harness loads by (**marketplace**), and it compares the owner's files before and
//! after a run to say they are unchanged (**guard**). Each such read is of a [`Source`], which
//! [`Account`] makes from a row of the table, and which names its purpose.
//!
//! A path of the kit's own cannot read what is in a store: the door's plain readers
//! ([`super::reads`]) refuse a path that resolves inside a protected root, and the audit
//! ([`super::rule`]) finds a read of the file system anywhere else in the files that touch a
//! store. So a new read of the owner's files has to ask the account for a [`Source`], with a
//! purpose, in one of the files the audit names for it.
//!
//! A `Source` holds no path a caller can take: it reads (`read`, `metadata`, `hash`, `names`,
//! `walk`, `copy_into` a judged place) and shows its name in a message.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest as _, Sha256};

use super::account;
use crate::live::stores::{self, Store};

/// Why the kit reads a file of the owner's. A purpose is stated at the one place a [`Source`] is
/// made, and the audit holds each to the files that have it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Purpose {
    /// A login or a config copied into the run's own home, so that a harness runs signed in
    /// there.
    Seed,
    /// A marketplace, a package or a model cache a harness loads by, copied into the run's home.
    Marketplace,
    /// A file compared before and after a run, to say the owner's setup is as it was.
    Guard,
}

/// A file or a folder of the owner's, read for one [`Purpose`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Source {
    path: PathBuf,
    why: Purpose,
}

impl fmt::Debug for Source {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Source({:?}, {})", self.why, self.path.display())
    }
}

impl fmt::Display for Source {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.path.display().fmt(formatter)
    }
}

/// The account that started the run, as the kit asks it for files to read.
#[derive(Debug, Clone, Copy)]
pub struct Account(());

impl Account {
    /// The account of this process.
    pub fn this() -> Account {
        Account(())
    }

    /// The place `row` is at on this account, as the harness itself would find it: the folder its
    /// variable names where that is set, else below the account's first home. None where the
    /// account has no such place (a variable unset and no home, or a row of a system folder that
    /// is not this platform's).
    pub fn source(&self, row: &Store, why: Purpose) -> Option<Source> {
        self.sources(row, why).into_iter().next()
    }

    /// Every place `row` has on this account, as the table spells them, the variable's first.
    pub fn sources(&self, row: &Store, why: Purpose) -> Vec<Source> {
        stores::roots(&account::lookup_in_this_process)
            .into_iter()
            .filter(|root| root.store == *row)
            .map(|root| Source {
                path: root.path,
                why,
            })
            .collect()
    }

    /// A folder or file a caller named (`--claude-config`), or a decoy a test made to stand for
    /// the owner's, read for `why`: no store's row stands behind it, and it is read, never
    /// written. A file of the owner's is named by this only where the caller said so.
    pub fn named(&self, path: impl AsRef<Path>, why: Purpose) -> Source {
        Source {
            path: path.as_ref().to_path_buf(),
            why,
        }
    }

    /// The account's home (`HOME`, else `USERPROFILE`), which the rows are below: a source to
    /// say the places below it.
    pub fn home(&self, why: Purpose) -> Option<Source> {
        account::home_of_this_process().map(|path| Source { path, why })
    }

    /// The folder a variable of the table names (`CODEX_HOME`, `XDG_CONFIG_HOME`, `APPDATA`,
    /// `PI_CODING_AGENT_DIR`), where the account set it: none where it did not.
    pub fn variable(&self, name: &str, why: Purpose) -> Option<Source> {
        account::variable_of_this_process(name).map(|path| Source { path, why })
    }

    /// A program the machine installed, found on the search path (a harness's), read for `why`:
    /// not in a store's row, which the table gives for a harness's own installer only.
    pub fn program(&self, found: impl AsRef<Path>, why: Purpose) -> Source {
        self.named(found, why)
    }
}

impl Source {
    /// A source at `path`, of this one's purpose and origin.
    fn at(&self, path: PathBuf) -> Source {
        Source {
            path,
            why: self.why,
        }
    }

    /// The place to read.
    fn place(&self) -> io::Result<&Path> {
        Ok(&self.path)
    }

    /// Why this is read.
    pub fn why(&self) -> Purpose {
        self.why
    }

    /// Its name, to say in a message.
    pub fn name(&self) -> String {
        self.path.display().to_string()
    }

    /// The name of its last part.
    pub fn file_name(&self) -> Option<OsString> {
        self.path.file_name().map(OsString::from)
    }

    /// Its last part's name, as text.
    pub fn last(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// `name` below it (`name` may hold several parts, spelled with `/`).
    pub fn join(&self, name: &str) -> Source {
        self.at(name
            .split('/')
            .filter(|part| !part.is_empty())
            .fold(self.path.clone(), |path, part| path.join(part)))
    }

    /// The folder it is in.
    pub fn parent(&self) -> Option<Source> {
        self.path.parent().map(|path| self.at(path.to_path_buf()))
    }

    /// This for another purpose.
    pub fn for_why(&self, why: Purpose) -> Source {
        Source {
            why,
            ..self.clone()
        }
    }

    /// Whether this is `other` or below it, by components as spelled.
    pub fn is_inside(&self, other: &Source) -> bool {
        stores::spelled_inside(&self.path, &other.path)
    }

    /// This place made absolute against the folder the process is in, as spelled.
    pub fn made_absolute(&self) -> Source {
        self.at(std::path::absolute(&self.path).unwrap_or_else(|_| self.path.clone()))
    }

    /// Whether `path` is this place, by components as spelled.
    pub fn is_path(&self, path: &Path) -> bool {
        self.path == path
    }

    /// Whether `path` is this place or below it, by components as spelled.
    pub fn holds(&self, path: &Path) -> bool {
        stores::spelled_inside(path, &self.path)
    }

    /// Whether this is `folder` or below it, by components as spelled.
    pub fn is_inside_path(&self, folder: &Path) -> bool {
        stores::spelled_inside(&self.path, folder)
    }

    /// Whether the two are one path once each is made absolute against the folder the process is in.
    pub fn is_the_same_spelling_as(&self, other: &Source) -> bool {
        matches!((std::path::absolute(&self.path), std::path::absolute(&other.path)),
            (Ok(left), Ok(right)) if left == right)
    }

    /// Whether this path, as the system resolves it now, is where the other's is.
    pub fn is_the_same_place_as(&self, other: &Source) -> bool {
        match (fs::canonicalize(&self.path), fs::canonicalize(&other.path)) {
            (Ok(left), Ok(right)) => left == right,
            _ => self.path == other.path,
        }
    }

    /// Its bytes.
    pub fn read(&self) -> io::Result<Vec<u8>> {
        fs::read(self.place()?)
    }

    /// Its text.
    pub fn read_to_string(&self) -> io::Result<String> {
        fs::read_to_string(self.place()?)
    }

    /// A reader of it, from its start.
    pub fn open(&self) -> io::Result<fs::File> {
        fs::File::open(self.place()?)
    }

    /// Its metadata, through a link.
    pub fn meta(&self) -> io::Result<fs::Metadata> {
        fs::metadata(self.place()?)
    }

    /// Its metadata, a link as itself.
    pub fn lstat(&self) -> io::Result<fs::Metadata> {
        fs::symlink_metadata(self.place()?)
    }

    /// Whether it is there, a link as itself.
    pub fn present(&self) -> bool {
        self.lstat().is_ok()
    }

    /// Whether it is a file, through a link.
    pub fn is_a_file(&self) -> bool {
        self.meta().is_ok_and(|found| found.is_file())
    }

    /// Whether it is a folder, through a link.
    pub fn is_a_folder(&self) -> bool {
        self.meta().is_ok_and(|found| found.is_dir())
    }

    /// Whether it is a link.
    pub fn is_a_link(&self) -> bool {
        self.lstat()
            .is_ok_and(|found| found.file_type().is_symlink())
    }

    /// When it was last written.
    pub fn modified(&self) -> io::Result<SystemTime> {
        self.meta()?.modified()
    }

    /// The real place it resolves to, which is read for the same purpose.
    pub fn real(&self) -> io::Result<Source> {
        fs::canonicalize(self.place()?).map(|path| self.at(path))
    }

    /// Where a link leads, as the text it holds.
    pub fn link_target(&self) -> io::Result<PathBuf> {
        fs::read_link(self.place()?)
    }

    /// What is in it, each entry a source of the same purpose, in no order.
    pub fn entries(&self) -> io::Result<Vec<Source>> {
        let mut found = Vec::new();
        for entry in fs::read_dir(self.place()?)? {
            found.push(self.at(entry?.path()));
        }
        Ok(found)
    }

    /// The names of what is in it, in no order.
    pub fn names(&self) -> io::Result<Vec<OsString>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(self.place()?)? {
            names.push(entry?.file_name());
        }
        Ok(names)
    }

    /// The SHA-256 of its bytes, in hex.
    pub fn hash(&self) -> io::Result<String> {
        Ok(hex(&Sha256::digest(self.read()?)))
    }

    /// Every entry below it that is no link (files and the folders themselves), by walking, in no
    /// order. A link is not followed and not listed.
    pub fn walk(&self) -> io::Result<Vec<Source>> {
        let mut found = Vec::new();
        let mut folders = vec![self.clone()];
        while let Some(folder) = folders.pop() {
            for entry in folder.entries()? {
                let kind = entry.lstat()?.file_type();
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    folders.push(entry.clone());
                }
                found.push(entry);
            }
        }
        Ok(found)
    }

    /// Its bytes written to `to`, a place the write door judges (new below a folder this run made).
    pub fn copy_into(&self, to: impl AsRef<Path>) -> crate::Result {
        let bytes = self
            .read()
            .map_err(crate::Error::file("read", &self.path))?;
        crate::door::write(to, bytes)
    }

    /// Its bytes written to `to` with the permissions it has: a login file is 0600.
    pub fn copy_keeping_mode_into(&self, to: impl AsRef<Path>) -> crate::Result {
        self.place()
            .map_err(crate::Error::file("read", &self.path))?;
        crate::door::copy_keeping_mode_from(self, to)
    }

    /// The file it is, cloned to `to` where the system makes a clone (a Claude Code's program is
    /// three hundred megabytes), copied where it does not.
    pub fn clone_into(&self, to: impl AsRef<Path>) -> crate::Result {
        self.place()
            .map_err(crate::Error::file("read", &self.path))?;
        crate::door::copy_cloning_from(self, to)
    }

    /// The folder it is, copied into `to` (made private), links that leave it skipped, a link inside
    /// it made to point into the copy; `skip` names what is left out.
    pub fn copy_tree_into(
        &self,
        to: impl AsRef<Path>,
        skip: &dyn Fn(&Source) -> bool,
    ) -> crate::Result {
        self.place()
            .map_err(crate::Error::file("read", &self.path))?;
        crate::door::copy_tree_from(self, to, skip)
    }

    /// An entry met in a walk of this (`path` is below it), read for the same purpose.
    pub(crate) fn of_walk(&self, path: &Path) -> Source {
        self.at(path.to_path_buf())
    }

    /// In a stand-in run, refuses (by a panic) a place that is not the stand-in's own decoy account:
    /// the guard of [`crate::standin_process::require_home`].
    pub fn require_home(&self, what: &str) {
        crate::standin_process::require_home(&self.path, what);
    }

    /// Its name as the text a start is given, for the one kind of start that reads it: the
    /// renewal's own account variables, and `git -C` of a provider's checkout.
    pub fn to_argument(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    /// In a stand-in run, refuses (by a panic) a watched place outside the stand-in's decoy account
    /// ([`crate::standin_process::require_watch_path`]).
    pub fn require_watch_path(&self, what: &str) {
        crate::standin_process::require_watch_path(&self.path, what);
    }

    /// Itself and each folder above it, nearest first.
    pub fn ancestors(&self) -> Vec<Source> {
        self.path
            .ancestors()
            .map(|path| self.at(path.to_path_buf()))
            .collect()
    }

    /// What the system says of it without opening it for its bytes: size, modification time and
    /// identity (a login file is compared by this and never read).
    pub fn stat(&self) -> io::Result<cf_base::file::Stat> {
        cf_base::file::stat(self.place()?)
    }

    /// Where it is below `home`, spelled with `/` between the parts: none where it is not below.
    pub fn below(&self, home: &Source) -> Option<String> {
        let below = self.path.strip_prefix(&home.path).ok()?;
        Some(
            below
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
        )
    }

    /// The path, for the door's writers that read what they copy: never given to a caller.
    pub(crate) fn path_for_door(&self) -> &Path {
        &self.path
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl From<&Source> for Source {
    fn from(source: &Source) -> Source {
        source.clone()
    }
}

/// A test's decoy for the owner's file or folder: any path, read for `Guard`. Unit tests of the
/// kit's own readers make their owner by a folder they made, which no row of the table holds.
#[cfg(test)]
impl From<&Path> for Source {
    fn from(path: &Path) -> Source {
        Account::this().named(path, Purpose::Guard)
    }
}

#[cfg(test)]
impl From<&PathBuf> for Source {
    fn from(path: &PathBuf) -> Source {
        Account::this().named(path, Purpose::Guard)
    }
}

#[cfg(test)]
impl From<PathBuf> for Source {
    fn from(path: PathBuf) -> Source {
        Account::this().named(path, Purpose::Guard)
    }
}
