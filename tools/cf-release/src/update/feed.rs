//! The feed's entry for a build, `latest.json`: what installed apps read from
//! the rolling release of their channel to learn that a newer app is there, and
//! where to download it. Tauri's updater format, for the one platform the app
//! ships an update for. It is written as `JSON.stringify` wrote it, one line and
//! a line break, so that an entry is the same bytes whichever tool made it.

use std::fs;
use std::path::Path;

use cf_base::js;
use serde_json::{json, Value};

/// The most of release notes the feed carries.
const NOTES_LIMIT: usize = 64 * 1024;

/// Where the release's files are downloaded from: GitHub's, for this repository.
const DOWNLOADS: &str = "https://github.com/ngvoicu/consensflow/releases/download";

/// Why the feed's entry could not be made.
#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    #[error("could not read release notes: {0}")]
    NotesUnreadable(String),
    #[error("release notes exceed 64 KiB")]
    NotesTooLong,
    #[error("could not write {path}: {cause}")]
    Unwritable { path: String, cause: String },
}

/// What the entry says of a build.
pub struct Build<'a> {
    /// The release's version.
    pub version: &'a str,
    /// What changed, as the notes file said it.
    pub notes: &'a str,
    /// When it was published, as given.
    pub date: &'a str,
    /// The archive's file name, which the entry's URL ends with.
    pub archive: &'a str,
    /// The archive's signature.
    pub signature: &'a str,
}

/// The release notes in the file at `path`: its text without the blank around it.
pub fn notes(path: &Path) -> Result<String, FeedError> {
    let bytes = fs::read(path)
        .map_err(|cause| FeedError::NotesUnreadable(format!("{}: {cause}", path.display())))?;
    let text = String::from_utf8_lossy(&bytes);
    let notes = js::trim(&text);
    if notes.len() > NOTES_LIMIT {
        return Err(FeedError::NotesTooLong);
    }
    Ok(notes.to_string())
}

/// The entry for `build`, as the line `latest.json` holds. The URL is made here
/// from the version and the archive's name: nothing else can set it.
pub fn render(build: &Build) -> String {
    let entry: Value = json!({
        "version": build.version,
        "notes": build.notes,
        "pub_date": build.date,
        "platforms": {
            "darwin-aarch64": {
                "url": format!("{DOWNLOADS}/v{}/{}", build.version, build.archive),
                "signature": build.signature,
            },
        },
    });
    format!("{}\n", js::stringify(&entry))
}

/// Writes the entry to `path`, replacing what is there.
pub fn write(path: &Path, entry: &str) -> Result<(), FeedError> {
    fs::write(path, entry).map_err(|cause| FeedError::Unwritable {
        path: path.display().to_string(),
        cause: cause.to_string(),
    })
}

#[cfg(test)]
mod tests;
