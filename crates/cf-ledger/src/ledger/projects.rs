//! The ledger's projects, as its callers hold them.

use super::Ledger;
use crate::{
    meeting, projects, specs, DeletedProject, EventView, LedgerError, MessageView, NewProject,
    ProjectView, SpecsChange,
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

    /// The human starts (`on`) or stops a meeting with the project's chief: while
    /// one is on, what is queued for the chief is held (3.0.7 chief-meeting).
    pub fn set_meeting(&mut self, id: i64, on: bool) -> Result<ProjectView, LedgerError> {
        meeting::set_meeting(&mut self.store, id, on)
    }

    /// The messages the project's meeting holds, in the order they will go; none
    /// while no meeting is on.
    pub fn held_by_meeting(&self, id: i64) -> Result<Vec<MessageView>, LedgerError> {
        meeting::held(&self.store, id)
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

    /// Writes the rules the human gives a project's staff (plain text, at most
    /// 8,000 bytes and 150 lines), or takes them away: none, or blank text. They
    /// reach each window of the project when it next starts.
    pub fn set_staff_rules(
        &mut self,
        id: i64,
        rules: Option<&str>,
    ) -> Result<ProjectView, LedgerError> {
        projects::set_staff_rules(&mut self.store, id, rules)
    }

    /// Chooses the size, in tokens, at which the project's Claude member windows
    /// compact (100,000 to 1,000,000), or takes the choice away: none is Claude's
    /// own point. It reaches each window when it next starts.
    pub fn set_claude_compaction(
        &mut self,
        id: i64,
        window: Option<u32>,
    ) -> Result<ProjectView, LedgerError> {
        projects::set_claude_compaction(&mut self.store, id, window)
    }

    /// What the project says of specs: whether they are on, the folder and the
    /// Specmint variant, all three at once. A chief whose window is open is told at its
    /// next rest, and any other takes it at its next launch.
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
