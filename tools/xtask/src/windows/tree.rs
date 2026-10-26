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
//! is at and a hash of every file that goes over. The machine holds what it unpacked to
//! that hash (the sweep) and says the identity once it has, so that this program can tell
//! whether what ran there is what it sent. And it holds the run script, which is what the
//! run is asked for (the steps, the variables, the command).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};

use super::script::{sweep_script, RUN, SWEEP, SYNCED, TREE};
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
/// has no commit yet), and the first 16 hex digits of a SHA-256 over what goes over: for
/// each file, in the order `files` has them, the length of its name (8 bytes, little
/// endian), its name, its kind and, for a file, its bytes (a link or a folder, which git
/// can list, is `other` and has no bytes). Two checkouts at the same commit with other
/// changes made are two trees; so are two at different commits whose changes make the
/// same files.
///
/// The hash is one the machine can make of what it unpacked (the sweep does, and
/// holds the files to it), so it is made of what a Windows file system keeps: names, bytes,
/// and whether a thing is a file.
pub fn identity(context: &Context, files: &[String]) -> Result<String, Error> {
    let head =
        Invocation::new("git", &context.root).args(["rev-parse", "--verify", "--quiet", "HEAD"]);
    let answered = process::capture(&head, &context.env)?;
    let commit = if answered.code == 0 {
        answered.stdout.trim().to_owned()
    } else {
        NO_COMMIT.to_owned()
    };
    Ok(format!("{commit} {}", content(context, files)?))
}

/// The hash in [`identity`]: the first 16 hex digits of the SHA-256 over `files` as they
/// are under the checkout's root. It is the hash the sweep makes on the machine of what it
/// unpacked.
pub fn content(context: &Context, files: &[String]) -> Result<String, Error> {
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
        if fs::symlink_metadata(&path)
            .map_err(read)?
            .file_type()
            .is_file()
        {
            hash.update(b"file");
            let mut file = fs::File::open(&path).map_err(read)?;
            io::copy(&mut file, &mut hash).map_err(read)?;
        } else {
            hash.update(b"other");
        }
    }
    let digest = hash.finalize();
    Ok(digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// What [`identity`] says for a checkout with no commit yet.
const NO_COMMIT: &str = "none";

/// Packs `files`, the list of them, the tree's `identity`, the sweep that removes from
/// the copy what the list does not name ([`sweep_script`]) and the `run` script into
/// `tree.tgz` in `scratch`, and answers where it is.
///
/// The four of them are written in a folder of `scratch`, the run's own, and never in the
/// checkout: runs of one tree to the two machines (or two runs to one) pack the same
/// checkout at once, and files of its root were each other's to write over and to take away
/// (T-664, 2026-10-26: a run was sent another run's script, and one upload failed on a name
/// that was gone). `tar` packs the tree from the checkout's root and then appends the four
/// from their folder, in two steps because no one `tar` line reads two folders the same on
/// the systems the runner runs on (bsdtar reads a list of names from the last folder named);
/// the archive is compressed here, which `gzip` would do where it is on the PATH, and it is
/// not on zeewin's.
pub fn pack(
    context: &Context,
    files: &[String],
    identity: &str,
    run: &str,
    scratch: &Path,
) -> Result<PathBuf, Error> {
    let names = scratch.join("files");
    let state = scratch.join("state");
    let archive = scratch.join(ARCHIVE);
    let tarball = scratch.join("tree.tgz");
    fs::create_dir_all(&state).map_err(Error::files("make", &state))?;
    let mut list = files.join("\n");
    list.push('\n');
    let held = [
        (SYNCED, list),
        (TREE, format!("{identity}\n")),
        (SWEEP, sweep_script()),
        (RUN, run.to_owned()),
    ];
    for (name, text) in &held {
        let path = state.join(name);
        fs::write(&path, text).map_err(Error::files("write", &path))?;
    }
    fs::write(&names, files.join("\0")).map_err(Error::files("write", &names))?;
    for step in [
        tar(context, scratch, &names),
        append(&state, held.iter().map(|(name, _)| *name)),
    ] {
        let made = process::capture(&step, &context.env)?;
        if made.code != 0 {
            return Err(Error::step(&step, made.code, &made.stderr));
        }
    }
    gzip(&archive, &tarball)?;
    Ok(tarball)
}

/// The archive `tar` makes in the scratch folder, before it is compressed.
const ARCHIVE: &str = "tree.tar";

/// The `tar` that packs the names `names` holds (a file of them, each ended by a
/// NUL), from the checkout's root, into `tree.tar` in `scratch`. It runs in
/// `scratch` and names the archive by its bare name: the GNU tar Git for Windows
/// puts first on a PATH takes an archive named `C:\…` for a host to reach.
fn tar(context: &Context, scratch: &Path, names: &Path) -> Invocation {
    Invocation::new("tar", scratch)
        .args(["cf", ARCHIVE, "-C"])
        .arg(&context.root)
        .args(["--null", "-T"])
        .arg(names)
        // macOS's tar leaves out its ._ resource files.
        .var("COPYFILE_DISABLE", "1")
}

/// The `tar` that appends the files `held` to the archive, run in the folder `state` that holds
/// them and naming the archive, beside it, as `../tree.tar`: the folder is never an option's value,
/// which a GNU tar (Git for Windows's, first on the PATH of GitHub's runners) reads for its
/// backslash escapes and takes a drive letter in for a host. The folder of a run begins with the
/// run's name, which can begin with a digit (`\0000b009`, a NUL to it: the case of the lock).
fn append<'a>(state: &Path, held: impl Iterator<Item = &'a str>) -> Invocation {
    Invocation::new("tar", state)
        .args(["rf", "../tree.tar"])
        .args(held)
        .var("COPYFILE_DISABLE", "1")
}

/// `archive` compressed as `tarball`, as `tar czf` writes one, and `archive` taken away.
fn gzip(archive: &Path, tarball: &Path) -> Result<(), Error> {
    let compress = |cause| Error::Files {
        what: "compress",
        path: archive.to_path_buf(),
        cause,
    };
    let mut from = fs::File::open(archive).map_err(compress)?;
    let to = fs::File::create(tarball).map_err(compress)?;
    let mut to = GzEncoder::new(to, Compression::default());
    io::copy(&mut from, &mut to).map_err(compress)?;
    to.finish().map_err(compress)?;
    fs::remove_file(archive).map_err(compress)
}

#[cfg(test)]
mod tests;
