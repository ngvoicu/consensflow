//! The ledger's conversations, their copies and the chief's history, as its
//! callers hold them.

use serde_json::Value;

use super::Ledger;
use crate::{
    conversations, ChiefConversation, ChiefOpenWork, ChiefSwitch, ConversationView, HistoryAsk,
    HistoryRead, LastSwitch, LedgerError, ProjectView, TaskTranscript, TextPart,
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

    /// A part of one item's text, of participant `handle`'s history, from character `from`.
    pub fn read_history_part(
        &self,
        project_id: i64,
        handle: &str,
        conversation: i64,
        item: &str,
        from: usize,
    ) -> Result<TextPart, LedgerError> {
        conversations::read_history_part(&self.store, project_id, handle, conversation, item, from)
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
}
