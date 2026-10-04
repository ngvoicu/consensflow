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
//! Each read reads the file afresh and creates nothing: a file that cannot
//! be read, or parsed, or is no agents file is refused
//! (`agents-file-unreadable`), never taken for an empty roster, and never
//! written over. Each write reads it the same way first, and writes it whole
//! or not at all. Nothing in the shipped app writes through here before the
//! daemon's flip: until then the Node build is the file's one writer.
//!
//! - `file`: the path and the reading;
//! - `document`: the document the file holds, stored and loaded;
//! - `agent_row`: a row in the shape the file keeps it;
//! - `rows`: the catalog's rows and the human's, as the app sees them;
//! - `views`: a row as the page and the CLI list it;
//! - `preferences`: what the human chose, what it hides, and its setting;
//! - `save`: the file written;
//! - `normalize`: the file folded into the shape it keeps now;
//! - `add` and `edit`: an agent defined by hand, added, edited or removed.

mod add;
mod agent_row;
mod document;
mod edit;
mod file;
mod normalize;
mod preferences;
mod rows;
mod save;
#[cfg(test)]
mod testing;
mod views;

use std::path::PathBuf;

use cf_base::refusal::Refusal;
use cf_base::time::Clock;
use cf_proto::agents::{AgentView, Preferences};
use serde_json::{Map, Value};

use crate::Catalog;
use document::load_document;
use preferences::{hides, preferences_of, set_preferences};

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

    /// Sets what the human chose, from a request's `patch` (none, `null`,
    /// a number or a flag naming nothing; a list or a text naming the
    /// indices JavaScript gives it), and answers the choices as kept: the
    /// file is written even when nothing changes, and made when there is none.
    pub fn set_preferences(&self, patch: Option<&Value>) -> Result<Preferences, Refusal> {
        set_preferences(&self.path, patch)
    }

    /// Folds what older builds wrote into the shape the file keeps now: a
    /// copy of a catalog entry goes, so does stored display data, and an
    /// image agent on the `image` harness is a Codex agent that designs.
    /// Says whether the file changed; one that did not is not written.
    pub fn normalize(&self) -> Result<bool, Refusal> {
        self.catalog.normalize(&self.path)
    }

    /// Adds an agent defined by hand, as `input` names it (`name`,
    /// `harness`, `model`, and `designer`, `effort`, `description`,
    /// `workTier` when it sets them), and answers it as the page lists it.
    pub fn add(
        &self,
        input: &Map<String, Value>,
        clock: &mut dyn Clock,
    ) -> Result<AgentView, Refusal> {
        self.catalog.add(&self.path, input, clock)
    }

    /// Edits an agent defined by hand in place: the fields `patch` names,
    /// `null` taking a tier or an effort off. A catalog agent is the
    /// catalog's, and stays as the catalog has it.
    pub fn edit(
        &self,
        name: &str,
        patch: &Map<String, Value>,
        clock: &mut dyn Clock,
    ) -> Result<AgentView, Refusal> {
        self.catalog.edit(&self.path, name, patch, clock)
    }

    /// Removes an agent defined by hand; a catalog agent is the catalog's.
    pub fn remove(&self, name: &str) -> Result<(), Refusal> {
        self.catalog.remove(&self.path, name)
    }
}
