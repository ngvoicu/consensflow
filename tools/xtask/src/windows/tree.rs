//! The working tree as it is now, as one tarball: every tracked file, and the new
//! ones that are not ignored (`git ls-files -co --exclude-standard`), as they are
//! on disk, which is not always as the index has them.
//!
//! A tracked file that was deleted from the tree and not from the index is still
//! listed by git, and `tar` stops at a name it cannot stat: so a name is kept only
//! if there is something of that name on disk (a link counts, whatever it points
//! at). The list of what went over, written beside the files there, is the
//! filtered one: it is what the machine removes from its copy what is not in.

use std::fs;
use std::path::{Path, PathBuf};

use super::script::SYNCED;
use super::Error;
use crate::context::Context;
use crate::process::{self, Invocation};

/// The files of the working tree that go over, as git names them (with `/`).
pub fn files(context: &Context) -> Result<Vec<String>, Error> {
    let listing =
        Invocation::new("git", &context.root).args(["ls-files", "-co", "--exclude-standard", "-z"]);
    let answered = process::capture(&listing, &context.env)?;
    if answered.code != 0 {
        return Err(Error::step(&listing, answered.code, &answered.stderr));
    }
    kept(&answered.stdout, |name| {
        fs::symlink_metadata(context.path(name)).is_ok()
    })
}

/// The names of git's listing (each ended by a NUL) of which `exists` says there
/// is a file, in the order git gave them.
fn kept(listing: &str, exists: impl Fn(&str) -> bool) -> Result<Vec<String>, Error> {
    let mut files = Vec::new();
    for name in listing.split('\0').filter(|name| !name.is_empty()) {
        // A name that was no text was read as another, which nothing on disk has: its
        // file would be left out without a word.
        if name.contains(char::REPLACEMENT_CHARACTER) {
            return Err(Error::Name(name.to_owned()));
        }
        if exists(name) {
            files.push(name.to_owned());
        }
    }
    Ok(files)
}

/// Packs `files` (and the list of them) into `tree.tgz` in `scratch`, and answers where it is.
///
/// The list is a file of the checkout while `tar` reads it, for it names its
/// files from the checkout's root; it is taken away when the tarball is made, or
/// when that fails.
pub fn pack(context: &Context, files: &[String], scratch: &Path) -> Result<PathBuf, Error> {
    let names = scratch.join("files");
    let tarball = scratch.join("tree.tgz");
    let named: Vec<&str> = files.iter().map(String::as_str).chain([SYNCED]).collect();
    fs::write(&names, named.join("\0")).map_err(Error::files("write", &names))?;
    let tar = tar(context, scratch, &names);
    let made = {
        let _list = Listed::write(&context.root, files)?;
        process::capture(&tar, &context.env)
    }?;
    if made.code != 0 {
        return Err(Error::step(&tar, made.code, &made.stderr));
    }
    Ok(tarball)
}

/// The `tar` that packs the names `names` holds (a file of them, each ended by a
/// NUL), from the checkout's root, into `tree.tgz` in `scratch`. It runs in
/// `scratch` and names the archive by its bare name: the GNU tar Git for Windows
/// puts first on a PATH takes an archive named `C:\…` for a host to reach.
fn tar(context: &Context, scratch: &Path, names: &Path) -> Invocation {
    Invocation::new("tar", scratch)
        .args(["czf", "tree.tgz", "-C"])
        .arg(&context.root)
        .args(["--null", "-T"])
        .arg(names)
        // macOS's tar leaves out its ._ resource files.
        .var("COPYFILE_DISABLE", "1")
}

/// The list of what goes over, as a file of the checkout: taken away when it is dropped.
struct Listed(PathBuf);

impl Listed {
    fn write(root: &Path, files: &[String]) -> Result<Self, Error> {
        let path = root.join(SYNCED);
        let mut text = files.join("\n");
        text.push('\n');
        fs::write(&path, text).map_err(Error::files("write", &path))?;
        Ok(Self(path))
    }
}

impl Drop for Listed {
    fn drop(&mut self) {
        // Gone already, or not to be taken away: nothing more to be done about it.
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests;
