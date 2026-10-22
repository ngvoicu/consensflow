//! The working tree as it is now, as one tarball: every tracked file, and the new
//! ones that are not ignored (`git ls-files -co --exclude-standard`), as they are
//! on disk, which is not always as the index has them.
//!
//! A tracked file that was deleted from the tree and not from the index is still
//! listed by git, and `tar` stops at a name it cannot stat: so a name is kept only
//! if there is something of that name on disk (a link counts, whatever it points
//! at). The list of what went over, written beside the files there, is the
//! filtered one: it is what the machine removes from its copy what is not in.
//!
//! The tarball also holds the tree's identity ([`identity`]): the commit the checkout
//! is at and a hash of every file that goes over, which the machine says once it has
//! unpacked the tarball, so that this program can tell whether what ran there is what
//! it sent.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::script::{sweep_script, SWEEP, SYNCED, TREE};
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

/// What a tree is, as a line of text: the commit the checkout is at (`none` where it
/// has no commit yet), and the first 16 hex digits of a SHA-256 over what goes over,
/// each file's name, its kind and its bytes (a link's, the text of the link), in the
/// order `files` has them. Two checkouts at the same commit with other changes made
/// are two trees; so are two at different commits whose changes make the same files.
pub fn identity(context: &Context, files: &[String]) -> Result<String, Error> {
    let head =
        Invocation::new("git", &context.root).args(["rev-parse", "--verify", "--quiet", "HEAD"]);
    let answered = process::capture(&head, &context.env)?;
    let commit = if answered.code == 0 {
        answered.stdout.trim().to_owned()
    } else {
        NO_COMMIT.to_owned()
    };
    let mut hash = Sha256::new();
    for name in files {
        let path = context.path(name);
        let read = |cause: io::Error| Error::Files {
            what: "read",
            path: path.clone(),
            cause,
        };
        hash.update(u64::try_from(name.len()).unwrap_or(u64::MAX).to_le_bytes());
        hash.update(name.as_bytes());
        let kind = fs::symlink_metadata(&path).map_err(read)?.file_type();
        if kind.is_symlink() {
            hash.update(b"link");
            let target = fs::read_link(&path).map_err(read)?;
            hash.update(target.as_os_str().as_encoded_bytes());
        } else if kind.is_file() {
            hash.update(b"file");
            let mut file = fs::File::open(&path).map_err(read)?;
            io::copy(&mut file, &mut hash).map_err(read)?;
        } else {
            // A folder (git lists a submodule as one): nothing of it goes into the hash.
            hash.update(b"dir");
        }
    }
    let digest = hash.finalize();
    let content: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(format!("{commit} {content}"))
}

/// What [`identity`] says for a checkout with no commit yet.
const NO_COMMIT: &str = "none";

/// Packs `files`, the list of them, the tree's `identity` and the sweep that removes
/// from the copy what the list does not name ([`sweep_script`]) into `tree.tgz` in
/// `scratch`, and answers where it is.
///
/// The list, the identity and the sweep are files of the checkout while `tar` reads
/// them, for it names its files from the checkout's root; they are taken away when the
/// tarball is made, or when that fails.
pub fn pack(
    context: &Context,
    files: &[String],
    identity: &str,
    scratch: &Path,
) -> Result<PathBuf, Error> {
    let names = scratch.join("files");
    let tarball = scratch.join("tree.tgz");
    let named: Vec<&str> = files
        .iter()
        .map(String::as_str)
        .chain([SYNCED, TREE, SWEEP])
        .collect();
    fs::write(&names, named.join("\0")).map_err(Error::files("write", &names))?;
    let tar = tar(context, scratch, &names);
    let made = {
        let _list = Listed::write(&context.root, files, identity)?;
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

/// The list of what goes over, the identity and the sweep, as files of the checkout:
/// taken away when it is dropped.
struct Listed(Vec<PathBuf>);

impl Listed {
    fn write(root: &Path, files: &[String], identity: &str) -> Result<Self, Error> {
        let mut listed = Self(Vec::new());
        let mut list = files.join("\n");
        list.push('\n');
        for (name, text) in [
            (SYNCED, list),
            (TREE, format!("{identity}\n")),
            (SWEEP, sweep_script()),
        ] {
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
