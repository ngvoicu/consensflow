//! The file written (`STALE_FIELDS` and `saveDocument`, `src/roster.js`):
//! whole or not at all, a write cut short leaving the previous file, as
//! `JSON.stringify(document, null, 2)` writes it with a line break after.

use std::io;
use std::path::Path;

use cf_base::file::{errno_name, write_whole};
use cf_base::js;
use cf_base::json::{nesting, DEEPEST};
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
///
/// A document nested deeper than this build reads back (a description of
/// 125 levels, under the file's three) is refused, and nothing is written:
/// Node wrote it, and read it back, but here every read after would refuse
/// the file.
pub(crate) fn save_document(path: &Path, document: &mut Document) -> Result<(), Refusal> {
    for row in document.agents_mut() {
        for field in STALE_FIELDS {
            row.remove(field);
        }
    }
    let value = document.to_value();
    if nesting(&value) > DEEPEST {
        return Err(Refusal::new(
            "agents-file-too-deep",
            format!(
                "the agents file would nest deeper than ConsensFlow reads it back ({DEEPEST} levels): nothing was saved"
            ),
        ));
    }
    let text = format!("{}\n", js::stringify_indented(&value, 2));
    write_whole(path, text.as_bytes()).map_err(|error| unwritable(&error))
}

/// What a failed write says: the errno line Node's error carried, in the
/// system's own words after its name. Node said it with the temporary's or
/// the folder's path; the words are the platform's, which only the name
/// promises.
fn unwritable(error: &io::Error) -> Refusal {
    let said = match errno_name(error) {
        Some(name) => format!("{name}: {error}"),
        None => error.to_string(),
    };
    Refusal::new("agents-file-unwritable", said)
}

#[cfg(test)]
mod tests;
