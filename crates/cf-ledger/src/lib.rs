//! The ledger: the one durable record of every project, participant, task
//! and inbox message, in `<home>/consensflow.db`. The board and every inbox
//! are views of it. A port of `src/ledger/`, file for file; its rules are
//! the long comment in `index.js`, each with a test, and every ledger the
//! Node suite opened is a trace this crate replays (`tests/replay.rs`).
//!
//! The daemon is the only process that opens the file. The connection runs
//! in SQLite's exclusive locking mode and takes the write lock when it opens,
//! so the lock is the instance lock: a second opener, in this process or
//! another, is refused with `ledger-locked`, and the operating system
//! releases the lock when the holder closes or dies. Every operation is one
//! transaction: it validates, writes and logs, or it is refused and writes
//! nothing. This crate never reads the environment and never logs: the file
//! and the clock are arguments, and every refusal has a stable code.

#![forbid(unsafe_code)]

mod conversations;
/// The ledger's vocabulary: the words its records hold, their limits, and
/// the parsers a caller reading JSON (the API, the page) checks a request with.
pub mod model;
mod projects;
mod queue;
mod schema;
mod staff;
mod store;
mod views;

use std::path::Path;
use std::time::Duration;

use cf_base::time::{Clock, SystemClock};
use rusqlite::{Connection, ErrorCode};
use serde_json::Value;

pub use cf_proto::ledger::{
    Candidate, ChiefConversation, ChiefOpenWork, ConversationView, DeletedProject, EventView,
    LastSwitch, MemberView, MessageView, ParticipantView, ProjectView, RemovedMember, StaffMember,
    TaskTranscript, TierChange,
};
pub use conversations::{ChiefSwitch, TRANSCRIPT_ITEM_MAX};
pub use model::LedgerError;
pub use projects::{NewChief, NewMember, NewProject};
pub use schema::{migrate, MIGRATIONS, SCHEMA_VERSION};

use store::Store;

/// An event as it is logged: when, of which project, what kind, and its data.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub at: String,
    pub project: i64,
    pub kind: String,
    pub data: Value,
}

/// What a ledger is given: the time, and someone told every event as it is logged.
pub struct Options {
    pub clock: Box<dyn Clock>,
    pub trace: Box<dyn FnMut(&Event)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            clock: Box::new(SystemClock),
            trace: Box::new(|_| {}),
        }
    }
}

/// The ledger of a home, open.
pub struct Ledger {
    store: Store,
}

/// Opens the ledger at `file`, made or brought to this build's schema: the
/// one instance that holds it. Another holder refuses it (`ledger-locked`),
/// as does a file that is no ledger (`ledger-unreadable`) and one a newer
/// build wrote (`ledger-newer`).
pub fn open_ledger(file: &Path, options: Options) -> Result<Ledger, LedgerError> {
    let db = Connection::open(file)?;
    // SQLite's own default waits five seconds for a lock: a second instance
    // is refused at once instead. Node set both of these when it opened.
    db.busy_timeout(Duration::ZERO)?;
    db.execute_batch("PRAGMA foreign_keys = ON")?;
    let ready = (|| {
        db.execute_batch("PRAGMA locking_mode = EXCLUSIVE")?;
        // In exclusive mode the first write lock is kept until the connection
        // closes; taking it here is what makes this connection the only one.
        db.execute_batch("BEGIN EXCLUSIVE; COMMIT")?;
        db.execute_batch("PRAGMA journal_mode = WAL")?;
        db.execute_batch("PRAGMA synchronous = FULL")?;
        migrate(&db, &MIGRATIONS)
    })();
    if let Err(cause) = ready {
        drop(db);
        return Err(opening_refusal(file, cause));
    }
    Ok(Ledger {
        store: Store::new(db, options.clock, options.trace),
    })
}

/// What an opening that failed tells: a held file, or one that is no ledger,
/// said as the ledger says it; anything else as it came.
fn opening_refusal(file: &Path, cause: LedgerError) -> LedgerError {
    let LedgerError::Sqlite(rusqlite::Error::SqliteFailure(failure, message)) = &cause else {
        return cause;
    };
    match failure.code {
        ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => LedgerError::refused_with(
            "ledger-locked",
            format!("another ConsensFlow has {} open", file.display()),
            409,
        ),
        ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => LedgerError::refused_with(
            "ledger-unreadable",
            format!(
                "{} is not a readable ConsensFlow ledger: {}",
                file.display(),
                message.clone().unwrap_or_else(|| failure.to_string())
            ),
            409,
        ),
        _ => cause,
    }
}

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

    /// A member joins the staff, or rejoins it in the roles, harness,
    /// designer flag and tier given now.
    pub fn add_member(
        &mut self,
        project_id: i64,
        member: &NewMember,
    ) -> Result<ParticipantView, LedgerError> {
        staff::add_member(&mut self.store, project_id, member)
    }

    /// Each active member's tier becomes its saved agent's now (`tier_of`
    /// answers it, or none for an agent the roster no longer has): the members that changed.
    pub fn refresh_member_tiers(
        &mut self,
        tier_of: impl FnMut(&str) -> Option<String>,
    ) -> Result<Vec<TierChange>, LedgerError> {
        staff::refresh_member_tiers(&mut self.store, tier_of)
    }

    /// A member's roles change in place; a role it gains must fit its agent.
    pub fn set_roles<S: AsRef<str>>(
        &mut self,
        project_id: i64,
        handle: &str,
        roles: &[S],
    ) -> Result<ParticipantView, LedgerError> {
        staff::set_roles(&mut self.store, project_id, handle, roles)
    }

    /// A member leaves the staff, its open tasks cancelled.
    pub fn remove_member(
        &mut self,
        project_id: i64,
        handle: &str,
    ) -> Result<RemovedMember, LedgerError> {
        staff::remove_member(&mut self.store, project_id, handle)
    }

    /// The members of the newest project that has any: the staff a new project starts from.
    pub fn last_staff(&self) -> Result<Vec<StaffMember>, LedgerError> {
        staff::last_staff(&self.store)
    }

    /// Whether a participant has a task on its hands, paused work included.
    pub fn holds_work(&self, participant_id: i64) -> Result<bool, LedgerError> {
        staff::holds_work(&self.store, participant_id)
    }

    /// The active members an open task may go to, with what the daemon ranks them by.
    pub fn candidates(&self, project_id: i64, number: i64) -> Result<Vec<Candidate>, LedgerError> {
        staff::candidates(&self.store, project_id, number)
    }

    /// The active members of one role, in join order.
    pub fn members(&self, project_id: i64, role: &str) -> Result<Vec<MemberView>, LedgerError> {
        staff::members(&self.store, project_id, Some(role))
    }

    /// The human ends a session that holds no work, `by` saying who did.
    pub fn end_session(
        &mut self,
        project_id: i64,
        handle: &str,
        by: &str,
    ) -> Result<ProjectView, LedgerError> {
        staff::end_session(&mut self.store, project_id, handle, by)
    }

    /// A member out of quota takes no work until `until`, an ISO time.
    pub fn mark_out(
        &mut self,
        participant_id: i64,
        until: &str,
        reason: &str,
    ) -> Result<ParticipantView, LedgerError> {
        staff::mark_out(&mut self.store, participant_id, until, reason)
    }

    /// A member out of quota is back before its reset; the tasks held for it go on.
    pub fn mark_back(
        &mut self,
        participant_id: i64,
        because: &str,
    ) -> Result<ParticipantView, LedgerError> {
        staff::mark_back(&mut self.store, participant_id, because)
    }

    /// A participant's new native conversation; the one before it ends.
    pub fn start_conversation(
        &mut self,
        participant_id: i64,
        harness: &str,
    ) -> Result<ConversationView, LedgerError> {
        conversations::start_conversation(&mut self.store, participant_id, harness)
    }

    /// The harness's own session a conversation runs in.
    pub fn bind_conversation(
        &mut self,
        conversation_id: i64,
        native_session: &str,
    ) -> Result<ConversationView, LedgerError> {
        conversations::bind_conversation(&mut self.store, conversation_id, native_session)
    }

    /// Copies a window's items as its harness recorded them, the first at
    /// position `from`: how many rows changed.
    pub fn copy_transcript(
        &mut self,
        conversation_id: i64,
        items: &[Value],
        from: i64,
    ) -> Result<usize, LedgerError> {
        conversations::copy_transcript(&mut self.store, conversation_id, items, from)
    }

    /// What the windows that had a task wrote, the last `limit` items (all with none).
    pub fn transcript(
        &self,
        project_id: i64,
        number: i64,
        limit: Option<usize>,
    ) -> Result<TaskTranscript, LedgerError> {
        conversations::transcript(&self.store, project_id, number, limit)
    }

    /// The chief's earlier conversations, oldest first, with their items.
    pub fn chief_history(&self, project_id: i64) -> Result<Vec<ChiefConversation>, LedgerError> {
        conversations::chief_history(&self.store, project_id)
    }

    /// What waits on the chief now, for a chief that takes over.
    pub fn chief_open_work(&self, project_id: i64) -> Result<ChiefOpenWork, LedgerError> {
        conversations::chief_open_work(&self.store, project_id)
    }

    /// The human's Switch chief.
    pub fn switch_chief(
        &mut self,
        project_id: i64,
        switch: &ChiefSwitch,
    ) -> Result<ProjectView, LedgerError> {
        conversations::switch_chief(&mut self.store, project_id, switch)
    }

    /// The chief read its history: which page, or what it searched for.
    pub fn history_read(
        &mut self,
        project_id: i64,
        page: i64,
        find: Option<&str>,
        tools: bool,
    ) -> Result<(), LedgerError> {
        conversations::history_read(&mut self.store, project_id, page, find, tools)
    }

    /// The project's latest Switch chief; none before any.
    pub fn last_switch(&self, project_id: i64) -> Result<Option<LastSwitch>, LedgerError> {
        conversations::last_switch(&self.store, project_id)
    }

    /// A conversation ends; one that has ended already, or none, stays as it is.
    pub fn end_conversation(
        &mut self,
        conversation_id: i64,
    ) -> Result<Option<ConversationView>, LedgerError> {
        conversations::end_conversation(&mut self.store, conversation_id)
    }

    /// A participant's conversation now, if one has not ended.
    pub fn current_conversation(
        &self,
        participant_id: i64,
    ) -> Result<Option<ConversationView>, LedgerError> {
        conversations::current_conversation(&self.store, participant_id)
    }

    /// The first item of the participant's current conversation it was given that contains `text`.
    pub fn copied_item_with(
        &self,
        participant_id: i64,
        text: &str,
    ) -> Result<Option<String>, LedgerError> {
        conversations::copied_item_with(&self.store, participant_id, text)
    }

    /// A window the human switched to another conversation: the participant's is that one now.
    pub fn follow_conversation(
        &mut self,
        participant_id: i64,
        harness: &str,
        native_session: &str,
    ) -> Result<ConversationView, LedgerError> {
        conversations::follow_conversation(&mut self.store, participant_id, harness, native_session)
    }

    /// One message, or none.
    pub fn message(&self, id: i64) -> Result<Option<MessageView>, LedgerError> {
        self.store.message(id)
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

    /// SQLite's own consistency check of the whole file: `ok` when sound.
    pub fn integrity(&mut self) -> Result<String, LedgerError> {
        Ok(self
            .store
            .db
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?)
    }

    /// Closes the file; the lock goes with it.
    pub fn close(self) -> Result<(), LedgerError> {
        self.store.db.close().map_err(|(_, cause)| cause.into())
    }
}
