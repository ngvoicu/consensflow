//! The file written (`STALE_FIELDS` and `saveDocument`, `src/roster.js`):
//! whole or not at all, a write cut short leaving the previous file, as
//! `JSON.stringify(document, null, 2)` writes it with a line break after.

use std::path::Path;

use cf_base::file::{write_whole, FileError};
use cf_base::js;
use cf_base::refusal::Refusal;

use super::document::Document;

/// Display data older builds wrote into the file: recomputed on read now,
/// never stored again.
pub(super) const STALE_FIELDS: [&str; 5] = [
    "skillsPolicy",
    "skillPaths",
    "skills",
    "skillPath",
    "profile",
];

/// Writes `document` to `path`, every row without the stale fields first.
pub(crate) fn save_document(path: &Path, document: &mut Document) -> Result<(), Refusal> {
    for row in document.agents_mut() {
        for field in STALE_FIELDS {
            row.remove(field);
        }
    }
    let text = format!("{}\n", js::stringify_indented(&document.to_value(), 2));
    write_whole(path, text.as_bytes()).map_err(unwritable)
}

/// What a failed write says: Node's own message for the call that failed,
/// word for word, as the API answers `{error: cause.message}` and the page
/// shows that text: `EACCES: permission denied, open
/// '/home/me/.consensflow/agents.json.4242.tmp'`. The path is the
/// temporary's, or the folder's for a folder that could not be made.
fn unwritable(error: FileError) -> Refusal {
    Refusal::new("agents-file-unwritable", error.to_string())
}

#[cfg(test)]
mod tests;
