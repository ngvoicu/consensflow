//! Where the roster is and how its file is read (`rosterPath`, `readRoster`
//! and `unreadable`, `src/roster.js`). Only a missing file is an empty
//! roster: one that cannot be read or parsed is said to whoever reads it,
//! and nothing is written over it, since the next write would erase every
//! agent in it.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::home::config_root;
use cf_base::json::{from_slice_lossy, js_order};
use cf_base::refusal::Refusal;
use serde_json::{Map, Value};

/// The human's agents file, inside ConsensFlow's folder. None when the
/// environment names no folder to keep it in.
pub fn roster_path(env: &Env) -> Option<PathBuf> {
    config_root(env).map(|root| root.join("agents.json"))
}

/// The file as the human left it: the object it holds, its keys in the order
/// JavaScript enumerates them; none when there is no file. Reading creates
/// nothing.
pub(crate) fn read_roster(path: &Path) -> Result<Option<Map<String, Value>>, Refusal> {
    let bytes = match read_bytes(path) {
        Ok(bytes) => bytes,
        Err(Unread::Missing) => return Ok(None),
        Err(Unread::Code(code)) => {
            return Err(unreadable(path, &format!("cannot be read ({code})")))
        }
    };
    // Node reads bytes that are no UTF-8 as U+FFFD, and keeps a byte order
    // mark, which `JSON.parse` then refuses: so does this.
    let parsed = from_slice_lossy(&bytes).map_err(|_| unreadable(path, "is not valid JSON"))?;
    match js_order(parsed) {
        Value::Object(fields) => Ok(Some(fields)),
        _ => Err(unreadable(path, "is not an agents file")),
    }
}

/// The refusal that says the file at `path` is not one the roster can use:
/// `why` is the end of the sentence's first half. A caller reads the same
/// sentence for a row of the wrong shape.
pub(crate) fn unreadable(path: &Path, why: &str) -> Refusal {
    Refusal::new(
        "agents-file-unreadable",
        format!(
            "Your agents file {} {why}: fix it or move it away. ConsensFlow left it as it is.",
            path.display()
        ),
    )
}

/// Why a file gave no bytes.
enum Unread {
    /// There is no such file: the roster is empty.
    Missing,
    /// What Node prints for it: the errno's name (`EACCES`), else the system's words.
    Code(String),
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, Unread> {
    // Node says EISDIR for a directory on every system. Unix opens one and
    // refuses to read it with the same word; Windows refuses to open it, as
    // "access denied".
    if path.is_dir() {
        return Err(Unread::Code("EISDIR".to_owned()));
    }
    fs::read(path).map_err(|error| match error.kind() {
        ErrorKind::NotFound => Unread::Missing,
        _ => Unread::Code(code_of(&error)),
    })
}

/// `error.code ?? error.message`: the errno's name when it has one, else
/// the system's own words.
fn code_of(error: &io::Error) -> String {
    errno_name(error).map_or_else(|| error.to_string(), str::to_owned)
}

/// The name of the errno behind `error`, as Node's `error.code` has it: the
/// ones a file the human can reach fails with. The numbers differ between
/// the systems, so the names come from the constants. Windows has no errno,
/// and its codes are other numbers: it says the system's words.
#[cfg(unix)]
fn errno_name(error: &io::Error) -> Option<&'static str> {
    Some(match error.raw_os_error()? {
        libc::EACCES => "EACCES",
        libc::EPERM => "EPERM",
        libc::EISDIR => "EISDIR",
        libc::ENOTDIR => "ENOTDIR",
        libc::ELOOP => "ELOOP",
        libc::ENAMETOOLONG => "ENAMETOOLONG",
        libc::EIO => "EIO",
        libc::EMFILE => "EMFILE",
        libc::ENFILE => "ENFILE",
        libc::ENOMEM => "ENOMEM",
        libc::EBUSY => "EBUSY",
        libc::ENXIO => "ENXIO",
        libc::ENODEV => "ENODEV",
        libc::EINVAL => "EINVAL",
        libc::EOVERFLOW => "EOVERFLOW",
        libc::ETIMEDOUT => "ETIMEDOUT",
        libc::ESTALE => "ESTALE",
        libc::EAGAIN => "EAGAIN",
        _ => return None,
    })
}

#[cfg(not(unix))]
fn errno_name(_error: &io::Error) -> Option<&'static str> {
    None
}

#[cfg(test)]
mod tests;
