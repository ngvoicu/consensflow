//! The file folded into the shape it keeps now (`normalizeRoster`,
//! `src/roster.js`), at the daemon's start: a copy of a catalog entry goes
//! (the catalog has it), and so does stored display data; an image agent on
//! the `image` harness is a Codex agent that designs.

use std::path::Path;

use cf_base::js;
use cf_base::refusal::Refusal;
use serde_json::Value;

use super::document::{load_document, stored_document};
use super::save::{save_document, STALE_FIELDS};
use crate::Catalog;

impl Catalog {
    /// Folds the file at `path`, and says whether it changed: the file as
    /// stored, and as folded, compared as `JSON.stringify` writes each, so a
    /// file an older build left without its version or list, and nothing
    /// else to fold, is not written.
    pub(crate) fn normalize(&self, path: &Path) -> Result<bool, Refusal> {
        let before = js::stringify(&Value::Object(stored_document(path)?));
        let mut document = load_document(path)?;
        // The stale fields go from every row as it is folded, so a row that
        // held one makes the file change.
        let rows = document.agents_mut();
        for row in rows.iter_mut() {
            for field in STALE_FIELDS {
                row.remove(field);
            }
        }
        rows.retain(|row| self.entry_of(row).is_none());
        if js::stringify(&document.to_value()) == before {
            return Ok(false);
        }
        save_document(path, &mut document)?;
        Ok(true)
    }
}
