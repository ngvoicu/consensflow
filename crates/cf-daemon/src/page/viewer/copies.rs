//! The copies the browser is given of an HTML file of a specs folder: what the
//! door read and checked, written into a folder of the daemon's own, so the
//! browser opens those bytes and no path of the project's folder, which may
//! name something else by the time the browser reads it.
//!
//! Each copy is a folder of its own under `tmp/specs-open` in ConsensFlow's
//! home, by a name no other has, made for the user alone where the system has
//! modes; its files are written new, never over one that is there. A copy is
//! kept a day ([`KEPT_FOR`]), for the browser may read its assets late or be
//! reloaded, and is removed at the first opening after.
//!
//! ConsensFlow's home is trusted as a project's folder is: a link on its way
//! there is the human's. Below it, `tmp` and `specs-open` are each made where
//! they are not there and opened in the folder above as themselves, and held:
//! one that is a link, or on Windows any reparse point, is refused, in words,
//! and nothing is removed or written there (an agent may have left one, to
//! have the sweep remove where it leads). What the copies' folder holds is
//! read, removed and written through that held folder alone, and a link in it
//! is removed as a link, never followed.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_base::time::{Clock, SystemClock};

use super::folder::ForBrowser;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix::Folder;
#[cfg(windows)]
use windows::Folder;

/// How long a copy is kept: it is removed at the first opening after.
pub(super) const KEPT_FOR: Duration = Duration::from_secs(24 * 60 * 60);

/// The letters a copy's folder is named with.
const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// Why a folder on the copies' way was not taken.
enum Refused {
    /// A link, or on Windows any reparse point.
    Link,
    NotFolder,
    Io(io::Error),
}

impl From<io::Error> for Refused {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// A name the copies' folder holds, as its folder lists it, not following it:
/// whether it is a folder (a link to one is not), and when it was last written,
/// in milliseconds since 1970, where that could be read.
struct Seen {
    folder: bool,
    written: Option<i64>,
}

/// Writes `read` into a new folder of the copies' folder in ConsensFlow's home
/// `home`, the copies older than `kept_for` removed first: the path of its
/// HTML file, the first of its files.
pub(super) fn place(home: &Path, read: &ForBrowser, kept_for: Duration) -> Result<PathBuf, String> {
    if read.files.is_empty() {
        return Err("nothing was read for the browser".to_owned());
    }
    let home = Folder::home(home).map_err(|error| failed(home, &error))?;
    let tmp = way(&home, "tmp")?;
    let root = way(&tmp, "specs-open")?;
    sweep(&root, kept_for);
    let copy = made(&root)?;
    let mut opened = None;
    for (at, bytes) in &read.files {
        let Some((file, folders)) = at.split_last() else {
            continue;
        };
        let mut held: Vec<Folder> = Vec::with_capacity(folders.len());
        for name in folders {
            let next = way(held.last().unwrap_or(&copy), name)?;
            held.push(next);
        }
        let folder = held.last().unwrap_or(&copy);
        let written = folder.path().join(file);
        folder
            .write_new(file, bytes)
            .map_err(|error| failed(&written, &error))?;
        opened.get_or_insert(written);
    }
    opened.ok_or_else(|| "nothing was read for the browser".to_owned())
}

/// The folder `name` in `above`, made where it is not there, opened as itself:
/// refused, in words, where it is a link or no folder.
fn way(above: &Folder, name: &str) -> Result<Folder, String> {
    above.child(name).map_err(|refused| {
        let path = above.path().join(name);
        match refused {
            Refused::Link => format!(
                "no copy for the browser was made: {} is a link, and no copy is made where a \
                 link leads",
                path.display()
            ),
            Refused::NotFolder => format!(
                "no copy for the browser was made: {} is no folder",
                path.display()
            ),
            Refused::Io(error) => failed(&path, &error),
        }
    })
}

fn failed(path: &Path, error: &io::Error) -> String {
    format!(
        "the copy for the browser could not be written at {}: {error}",
        path.display()
    )
}

/// A folder of `root` by a name no other has, made new.
fn made(root: &Folder) -> Result<Folder, String> {
    loop {
        let mut random = [0_u8; 12];
        getrandom::fill(&mut random)
            .map_err(|error| failed(root.path(), &io::Error::other(error)))?;
        let name: String = random
            .iter()
            .map(|byte| char::from(ALPHABET[usize::from(*byte) % ALPHABET.len()]))
            .collect();
        match root.new_child(&name) {
            Ok(Some(copy)) => return Ok(copy),
            Ok(None) => {}
            Err(Refused::Io(error)) => return Err(failed(&root.path().join(&name), &error)),
            // Made just now, by a name no other had: a link there is no copy of ours.
            Err(_) => {
                return Err(format!(
                    "no copy for the browser was made: {} changed as it was made",
                    root.path().join(&name).display()
                ))
            }
        }
    }
}

/// Removes what `root` holds that was last written `kept_for` ago or longer,
/// as far as it can: a copy that cannot be removed now is removed at a later
/// opening.
fn sweep(root: &Folder, kept_for: Duration) {
    let Ok(names) = root.names() else {
        return;
    };
    let now = SystemClock.now_ms();
    let kept_for = i64::try_from(kept_for.as_millis()).unwrap_or(i64::MAX);
    for (name, seen) in names {
        if seen
            .written
            .is_some_and(|written| now.saturating_sub(written) >= kept_for)
        {
            root.remove(&name, &seen);
        }
    }
}

#[cfg(test)]
mod tests;
