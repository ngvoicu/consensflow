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

use super::script::{sweep_script, SWEEP, SYNCED};
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

/// Packs `files`, the list of them and the sweep that removes from the copy what
/// the list does not name ([`sweep_script`]) into `tree.tgz` in `scratch`, and
/// answers where it is.
///
/// The list and the sweep are files of the checkout while `tar` reads them, for it
/// names its files from the checkout's root; they are taken away when the tarball
/// is made, or when that fails.
pub fn pack(context: &Context, files: &[String], scratch: &Path) -> Result<PathBuf, Error> {
    let names = scratch.join("files");
    let tarball = scratch.join("tree.tgz");
    let named: Vec<&str> = files
        .iter()
        .map(String::as_str)
        .chain([SYNCED, SWEEP])
        .collect();
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

/// The list of what goes over and the sweep, as files of the checkout: taken away
/// when it is dropped.
struct Listed(Vec<PathBuf>);

impl Listed {
    fn write(root: &Path, files: &[String]) -> Result<Self, Error> {
        let mut listed = Self(Vec::new());
        let mut list = files.join("\n");
        list.push('\n');
        for (name, text) in [(SYNCED, list), (SWEEP, sweep_script())] {
            let path = root.join(name);
            fs::write(&path, text).map_err(Error::files("write", &path))?;
            listed.0.push(path);
        }
        Ok(listed)
    }
}

impl Drop for Listed {
    fn drop(&mut self) {
        for path in &self.0 {
            // Gone already, or not to be taken away: nothing more to be done about it.
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests;
