//! The ledger's projects, as its callers hold them (`index.js`, projects).

use super::Ledger;
use crate::{projects, DeletedProject, EventView, LedgerError, NewProject, ProjectView};

impl Ledger {
    /// A project with its human, its chief and its staff.
    pub fn create_project(&mut self, request: &NewProject) -> Result<ProjectView, LedgerError> {
        projects::create_project(&mut self.store, request)
    }

    /// A project and the participants still in it; none for an id no project has.
    pub fn project(&self, id: i64) -> Result<Option<ProjectView>, LedgerError> {
        projects::project(&self.store, id)
    }

    /// Every project, oldest first.
    pub fn projects(&self) -> Result<Vec<ProjectView>, LedgerError> {
        #[cfg(feature = "test-support")]
        self.watched("projects", None)?;
        projects::projects(&self.store)
    }

    /// Opens or suspends a project by hand: `open` or `suspended`.
    pub fn set_project_state(&mut self, id: i64, state: &str) -> Result<ProjectView, LedgerError> {
        projects::set_project_state(&mut self.store, id, state)
    }

    /// Deletes a closed project and everything in it.
    pub fn delete_project(&mut self, id: i64) -> Result<DeletedProject, LedgerError> {
        projects::delete_project(&mut self.store, id)
    }

    /// Whether the human approves each message between two agents.
    pub fn set_gate(&mut self, id: i64, gate: bool) -> Result<ProjectView, LedgerError> {
        projects::set_gate(&mut self.store, id, gate)
    }

    /// At daemon start: every open project is suspended, marked to come back by itself.
    pub fn suspend_for_restart(&mut self) -> Result<Vec<ProjectView>, LedgerError> {
        projects::suspend_for_restart(&mut self.store)
    }

    /// A project's resume on start has been tried: the mark goes.
    pub fn forget_resume(&mut self, id: i64) -> Result<(), LedgerError> {
        projects::forget_resume(&mut self.store, id)
    }

    /// A project's events after the one numbered `after`, oldest first, at most `limit` (Node's defaults: 0 and 500).
    pub fn events(
        &self,
        project_id: i64,
        after: i64,
        limit: i64,
    ) -> Result<Vec<EventView>, LedgerError> {
        projects::events(&self.store, project_id, after, limit)
    }
}
