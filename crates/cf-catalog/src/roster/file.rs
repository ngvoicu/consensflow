//! Where the roster is and how its file is read (`rosterPath`, `readRoster`
//! and `unreadable`, `src/roster.js`). Only a missing file is an empty
//! roster: one that cannot be read or parsed is said to whoever reads it,
//! and nothing is written over it, since the next write would erase every
//! agent in it.

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::{errno_name, is_missing};
use cf_base::home::{config_root, normalized};
use cf_base::json::from_slice_exact;
use cf_base::refusal::Refusal;
use serde_json::{Map, Value};

/// The human's agents file, inside ConsensFlow's folder, joined as Node's
/// `path.join` joins it: a `..` in `CONSENSFLOW_HOME` takes off the folder
/// before it by the text alone, so `/base/missing/..` is `/base`. None when
/// the environment names no folder to keep it in.
pub fn roster_path(env: &Env) -> Option<PathBuf> {
    config_root(env).map(|root| normalized(&root.join("agents.json")))
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
    // mark, which `JSON.parse` then refuses: so does this. JSON a value here
    // cannot hold as Node would (a lone surrogate's escape, a number past a
    // double's range, nesting past 128 levels) is refused as JSON this build
    // cannot read, stricter than Node and kept on purpose: the next write
    // would otherwise change the file, and the file is left as it is.
    let parsed = from_slice_exact(&bytes).map_err(|_| unreadable(path, "is not valid JSON"))?;
    match parsed {
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
    // access denied.
    if path.is_dir() {
        return Err(Unread::Code("EISDIR".to_owned()));
    }
    fs::read(path).map_err(|error| {
        if is_missing(&error) {
            Unread::Missing
        } else {
            Unread::Code(errno_name(&error).map_or_else(|| error.to_string(), str::to_owned))
        }
    })
}

#[cfg(test)]
mod tests;
