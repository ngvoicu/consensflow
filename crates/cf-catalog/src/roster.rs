//! The roster (`src/roster.js`): every catalog agent, exactly as the catalog
//! has it, and the agents defined by hand. `agents.json` keeps only the
//! latter, in full. A catalog agent is never edited or removed: a different
//! setting is a custom agent under a name of its own. Rows an older build
//! saved from the catalog are the catalog's again on read.
//!
//! An image agent is a Codex agent with the designer flag (`designer:
//! true`): its window is Codex on its own default model, whose image tool
//! draws.
//!
//! This is the roster's reads. Each reads the file afresh and creates
//! nothing: a file that cannot be read, or parsed, or is no agents file is
//! refused (`agents-file-unreadable`), never taken for an empty roster, and
//! never written over. The roster's writes wait for the daemon's flip, when
//! the Node build stops being the file's one writer.
//!
//! - `file`: the path and the reading;
//! - `document`: the document the file holds, stored and loaded;
//! - `agent_row`: a row in the shape the file keeps it;
//! - `rows`: the catalog's rows and the human's, as the app sees them;
//! - `views`: a row as the page and the CLI list it;
//! - `preferences`: what the human chose, and what it hides.

mod agent_row;
mod document;
mod file;
mod preferences;
mod rows;
#[cfg(test)]
mod testing;
mod views;

use std::path::PathBuf;

use cf_base::refusal::Refusal;
use cf_proto::agents::{AgentView, Preferences};

use crate::Catalog;
use document::load_document;
use preferences::{hides, preferences_of};

pub use agent_row::AgentRow;
pub use file::roster_path;

/// The roster in the file at a path, over the catalog it lists first.
#[derive(Debug, Clone)]
pub struct Roster<'a> {
    catalog: &'a Catalog,
    path: PathBuf,
}

impl<'a> Roster<'a> {
    pub fn new(catalog: &'a Catalog, path: PathBuf) -> Self {
        Self { catalog, path }
    }

    /// The row the launcher runs, in the stored shape (`kind`, `thinking`):
    /// the catalog entry, or the custom row, the first whose id is `name`
    /// with one leading `@` taken off. The runner and the packet builder
    /// speak that shape, so [`Roster::list`] would drop the fields they run
    /// on.
    pub fn agent_row(&self, name: &str) -> Result<Option<AgentRow>, Refusal> {
        let wanted = name.strip_prefix('@').unwrap_or(name);
        let document = load_document(&self.path)?;
        Ok(self
            .catalog
            .rows(&document)
            .into_iter()
            .find(|row| row.id() == Some(wanted)))
    }

    /// What the human chose about the roster.
    pub fn preferences(&self) -> Result<Preferences, Refusal> {
        Ok(preferences_of(&load_document(&self.path)?))
    }

    /// Every agent as the page and the CLI list it, the catalog's first; the
    /// ones the human keeps out of sight say `hidden`. A row whose work tier
    /// is none of the four fails the whole list.
    pub fn list(&self) -> Result<Vec<AgentView>, Refusal> {
        let document = load_document(&self.path)?;
        let preferences = preferences_of(&document);
        self.catalog
            .rows(&document)
            .iter()
            .map(|row| {
                let view = self.catalog.view(row)?;
                Ok(if hides(preferences, &view) {
                    AgentView {
                        hidden: true,
                        ..view
                    }
                } else {
                    view
                })
            })
            .collect()
    }
}
