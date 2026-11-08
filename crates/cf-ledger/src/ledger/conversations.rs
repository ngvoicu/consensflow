//! The ledger's conversations, their copies and the chief's history, as its
//! callers hold them.

use serde_json::Value;

use super::Ledger;
use crate::{
    conversations, ChiefConversation, ChiefOpenWork, ChiefSwitch, ConversationView, HistoryAsk,
    HistoryKey, HistoryRead, LastSwitch, LedgerError, MessageParties, ProjectView, TaskTranscript,
    TextPart,
};

impl Ledger {
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

    /// One read of participant `handle`'s history for the viewer: every
    /// conversation, headed, and the items from where `ask` says, a page's
    /// worth at most and no more text than `ask.scan`.
    pub fn read_history(
        &self,
        project_id: i64,
        handle: &str,
        ask: &HistoryAsk<'_>,
    ) -> Result<HistoryRead, LedgerError> {
        conversations::read_history(&self.store, project_id, handle, ask)
    }

    /// A part of one item's text, of participant `handle`'s history, the one
    /// at row `row` of conversation `conversation`, from character `from`.
    pub fn read_history_part(
        &self,
        project_id: i64,
        handle: &str,
        conversation: i64,
        row: i64,
        from: usize,
    ) -> Result<TextPart, LedgerError> {
        conversations::read_history_part(&self.store, project_id, handle, conversation, row, from)
    }

    /// Where the one item of participant `handle`'s history that may still be written
    /// stands: the newest of its conversation now (none where it has none).
    pub fn live_item(
        &self,
        project_id: i64,
        handle: &str,
    ) -> Result<Option<HistoryKey>, LedgerError> {
        conversations::live_item(&self.store, project_id, handle)
    }

    /// Message `id` as far as the history page needs it, to tell whether a
    /// delivery names one of a participant's: its project, recipient and
    /// sender. Its body is never read.
    pub fn message_parties(&self, id: i64) -> Result<Option<MessageParties>, LedgerError> {
        conversations::message_parties(&self.store, id)
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

    /// A chief that needs an edit runs on the model and effort its harness's record names,
    /// and needs none (T-1202).
    pub fn chief_ran_on(
        &mut self,
        participant_id: i64,
        model: &str,
        effort: Option<&str>,
    ) -> Result<(), LedgerError> {
        conversations::chief_ran_on(&mut self.store, participant_id, model, effort)
    }

    /// The chief's launch asked its harness for `model`; none for the harness's own default.
    pub fn chief_launched(
        &mut self,
        participant_id: i64,
        model: Option<&str>,
    ) -> Result<(), LedgerError> {
        conversations::chief_launched(&mut self.store, participant_id, model)
    }

    /// The chief's launch found `gone` gone: it runs on `model` from now on, and no reopen
    /// asks for `gone` again.
    pub fn chief_gone(
        &mut self,
        participant_id: i64,
        gone: &str,
        model: Option<&str>,
    ) -> Result<(), LedgerError> {
        conversations::chief_gone(&mut self.store, participant_id, gone, model)
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

    /// The role text a conversation was last given, none where it is not known.
    pub fn conversation_role_text(
        &self,
        conversation_id: i64,
    ) -> Result<Option<String>, LedgerError> {
        conversations::conversation_role_text(&self.store, conversation_id)
    }

    /// A conversation is given `text` as its role, which logs no event. What a
    /// conversation was told by a message is recorded with the message's own
    /// confirmation ([`Ledger::confirm_delivery_telling`]), not by this.
    pub fn set_conversation_role_text(
        &mut self,
        conversation_id: i64,
        text: &str,
    ) -> Result<(), LedgerError> {
        #[cfg(feature = "test-support")]
        self.watched("set_conversation_role_text", Some(conversation_id))?;
        conversations::set_conversation_role_text(&mut self.store, conversation_id, text)
    }

    /// Whether the host proved the program of the last window that ran on a
    /// conversation gone, which only then goes on with another task (`ownership` in
    /// the engine).
    pub fn program_gone(&self, conversation_id: i64) -> Result<bool, LedgerError> {
        conversations::program_gone(&self.store, conversation_id)
    }

    /// Whether the program of the last window that ran on a conversation is gone:
    /// `true` as the host proves it, `false` before a window opens on it. It logs no
    /// event.
    pub fn set_program_gone(
        &mut self,
        conversation_id: i64,
        gone: bool,
    ) -> Result<(), LedgerError> {
        conversations::set_program_gone(&mut self.store, conversation_id, gone)
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

    /// Every item of the participant's current conversation it was given that
    /// contains `text`, first copied at `since` or after ([`Ledger::now`]), in
    /// order: its id and its words.
    pub fn copied_items_since(
        &self,
        participant_id: i64,
        text: &str,
        since: &str,
    ) -> Result<Vec<(String, String)>, LedgerError> {
        conversations::copied_items_since(&self.store, participant_id, text, since)
    }

    /// The time now, as the ledger writes it (a transcript item's `copied_at`).
    pub fn now(&mut self) -> String {
        self.store.at()
    }

    /// Every item of the participant's current conversation it was given that contains
    /// `text`, in order: its id and its words.
    pub fn copied_items_with(
        &self,
        participant_id: i64,
        text: &str,
    ) -> Result<Vec<(String, String)>, LedgerError> {
        conversations::copied_items_with(&self.store, participant_id, text)
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
}
