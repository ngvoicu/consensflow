//! The ledger's projects, as its callers hold them.

use super::Ledger;
use crate::{
    projects, specs, DeletedProject, EventView, LedgerError, NewProject, ProjectView, SpecsChange,
};

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

    /// Gives a project another name, with the rules a name has when the project
    /// is opened. The folder it is in stays.
    pub fn rename_project(&mut self, id: i64, name: &str) -> Result<ProjectView, LedgerError> {
        projects::rename_project(&mut self.store, id, name)
    }

    /// Writes the line the human says of a project under its name (one line of
    /// plain text, at most 200 characters), or takes it away: none, or blank text.
    pub fn describe_project(
        &mut self,
        id: i64,
        description: Option<&str>,
    ) -> Result<ProjectView, LedgerError> {
        projects::describe_project(&mut self.store, id, description)
    }

    /// What the project says of specs: whether they are on, the folder and the
    /// Specmint variant, all three at once. It applies at the chief's next launch.
    pub fn set_specs(&mut self, id: i64, change: &SpecsChange) -> Result<ProjectView, LedgerError> {
        specs::set_specs(&mut self.store, id, change)
    }

    /// At daemon start: every open project is suspended, for the human to resume.
    pub fn suspend_for_restart(&mut self) -> Result<Vec<ProjectView>, LedgerError> {
        projects::suspend_for_restart(&mut self.store)
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
