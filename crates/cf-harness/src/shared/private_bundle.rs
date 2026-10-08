//! A bundle of ConsensFlow's own files, published where a harness loads them:
//! verified and immutable, never replacing code a live pane may have loaded. It
//! is made in a folder of its own, named by the hash of every file in it, and
//! moved into place whole. A folder already there is another process's or an
//! earlier run's, checked file by file against this build's and kept as it is.
//!
//! A generated file (OpenCode's `tui.json`) names the folder the bundle is
//! published in, so it is hashed as it reads with the parent folder in that
//! place: a build that writes it differently is a new bundle, never one that
//! differs from what is installed and refuses every launch.
//!
//! Every call is synchronous, as Node's are, and a failure is said in Node's
//! words.

use std::io;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::{
    make_folder, make_temporary_folder, read_file_sync, remove_all, rename, write_file, FileError,
    Mkdir,
};
use cf_base::home::config_root;
use cf_base::path;
use sha2::{Digest, Sha256};

/// A file of a bundle, by its name in the repository and its bytes.
type File<'a> = (&'a str, &'a [u8]);

/// A file a bundle makes, by its name and its bytes.
pub(crate) type Made = (String, Vec<u8>);

/// Publishes the bundle `kind` made of `files`, and the files `generated`
/// makes of the folder it is published in: the folder it is in, or why it
/// could not be made.
pub(crate) fn prepare_private_integration(
    env: &Env,
    kind: &str,
    files: &[File],
    generated: &dyn Fn(&str) -> Vec<Made>,
) -> Result<String, String> {
    let config = config_root(env).ok_or_else(|| "missing home in env".to_owned())?;
    let root = path::join(&[&config.to_string_lossy(), "extensions", kind]);
    let hashed = generated(&root);
    let hash = hash_of(files.iter().copied().chain(borrowed(&hashed)));
    let destination = path::join(&[&root, &hash]);
    let written = generated(&destination);
    let files: Vec<File> = files.iter().copied().chain(borrowed(&written)).collect();
    let mut temporary = None;
    let outcome = install(&root, &destination, kind, &files, &mut temporary);
    // Whatever came of it, the folder it was made in goes; and a failure to
    // remove it is what is said, as it is thrown from a `finally`.
    if let Some(temporary) = temporary {
        remove_all(&temporary).map_err(|failed| failed.to_string())?;
    }
    outcome
}

/// The SHA-256 of `files` in hex, each file's name and its bytes, each
/// followed by a NUL byte.
fn hash_of<'a>(files: impl Iterator<Item = File<'a>>) -> String {
    let mut hash = Sha256::new();
    for (name, bytes) in files {
        hash.update(name.as_bytes());
        hash.update([0]);
        hash.update(bytes);
        hash.update([0]);
    }
    format!("{:x}", hash.finalize())
}

/// The files made, as they are held beside the others.
fn borrowed(made: &[Made]) -> impl Iterator<Item = File<'_>> {
    made.iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
}

/// The bundle made if it is not there, and every file of it checked. The
/// folder it is made in is put in `temporary` as soon as it exists.
fn install(
    root: &str,
    destination: &str,
    kind: &str,
    files: &[File],
    temporary: &mut Option<PathBuf>,
) -> Result<String, String> {
    let said = |failed: FileError| failed.to_string();
    if !Path::new(destination).exists() {
        make_folder(Path::new(root), 0o700, Mkdir::Sync).map_err(said)?;
        let folder = make_temporary_folder(&path::join(&[root, ".install-"]), random_name)
            .map_err(said)?
            .to_string_lossy()
            .into_owned();
        temporary.replace(PathBuf::from(&folder));
        for (name, bytes) in files {
            let target = path::join(&[&folder, name]);
            let target = Path::new(&target);
            if let Some(above) = target.parent() {
                make_folder(above, 0o700, Mkdir::Sync).map_err(said)?;
            }
            write_file(target, bytes, 0o600).map_err(said)?;
        }
        publish(Path::new(&folder), Path::new(destination)).map_err(said)?;
    }
    for (name, bytes) in files {
        let found = read_file_sync(Path::new(&path::join(&[destination, name]))).map_err(said)?;
        if found != *bytes {
            return Err(format!(
                "Private {kind} integration differs from this build; existing files were preserved"
            ));
        }
    }
    Ok(destination.to_owned())
}

/// Moves the folder made into place. A folder already there, because another
/// process published first, is no failure: the files in it are checked next.
fn publish(made: &Path, destination: &Path) -> Result<(), FileError> {
    match rename(made, destination) {
        Err(failed) if !matches!(failed.code(), "ENOTEMPTY" | "EEXIST") => Err(failed),
        _ => Ok(()),
    }
}

/// The six bytes a temporary folder's name is made of, from the system's own
/// randomness and not the adapter's: `mkdtemp` draws its own.
fn random_name() -> io::Result<[u8; 6]> {
    let mut bytes = [0; 6];
    getrandom::fill(&mut bytes).map_err(|failed| {
        failed.raw_os_error().map_or_else(
            || io::Error::other(failed.to_string()),
            io::Error::from_raw_os_error,
        )
    })?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;
