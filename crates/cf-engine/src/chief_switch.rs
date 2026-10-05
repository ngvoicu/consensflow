//! The human's Switch chief (`src/core/chief-switch.js`): now, or after the
//! chief's turn, with a note first asking it where things stand when the
//! human wants one. It takes a last look and copy, gives back what was in
//! flight, closes the old window as the engine's own, switches the chief in
//! the ledger and launches the new one with the handoff. It also picks a
//! chief's first message, and owns the record's pending switch.
//!
//! Landing C freezes what the dispatcher asks of it; a worker ports it.

use std::rc::Rc;

use cf_base::refusal::Refusal;
use cf_harness::contract::Observed;
use cf_ledger::{ConversationView, MessageView, NewNote, ParticipantView, ProjectView};

use crate::deliveries::Delivering;
use crate::dispatcher::Dispatcher;
use crate::handoff::{handoff_text, history_pages, is_handoff, last_words, Handoff};
use crate::record::Record;
use crate::seams::EngineError;
use crate::windows::not_ported;
use cf_proto::ledger::SwitchedFrom;

/// The chief a switch is to: a saved agent on its harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchTo {
    pub harness: String,
    pub agent: String,
}

/// A switch waiting for the end of the chief's turn, and the note it sent
/// first, if the human wanted one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingSwitch {
    pub(crate) to: SwitchTo,
    pub(crate) note: Option<i64>,
}

impl Dispatcher {
    /// A chief's first message. A handoff still on its way goes first: the
    /// window it was written for never came up, or closed before showing
    /// it. A chief that starts fresh with earlier conversations behind it
    /// (the human switched it) gets a handoff. Either way, what was queued
    /// for it waits for its next turn.
    pub(crate) fn chief_first(
        &self,
        project: &ProjectView,
        chief: &ParticipantView,
        conversation: Option<&ConversationView>,
        message: Option<MessageView>,
    ) -> Result<Option<MessageView>, EngineError> {
        let pending = self.seams.ledger.borrow().pending(chief.id)?;
        if let Some(handoff) = pending
            .into_iter()
            .find(|pending| is_handoff(pending) && pending.state == "queued")
        {
            return Ok(Some(handoff));
        }
        if conversation.is_none() {
            return Ok(self.handoff(project, chief)?.or(message));
        }
        Ok(message)
    }

    /// The handoff a chief that starts fresh is given, written for it and
    /// queued: what it takes over, the human's last words, and how many
    /// pages `cf history` has. Built from the ledger alone, so it needs no
    /// turn of the old chief's; none for a project's first chief.
    pub(crate) fn handoff(
        &self,
        project: &ProjectView,
        chief: &ParticipantView,
    ) -> Result<Option<MessageView>, EngineError> {
        let history = self.seams.ledger.borrow().chief_history(project.id)?;
        let Some(latest) = history.last() else {
            return Ok(None);
        };
        if history
            .iter()
            .all(|conversation| conversation.items.is_empty())
        {
            return Ok(None);
        }
        let switched = self.seams.ledger.borrow().last_switch(project.id)?;
        let from = switched.as_ref().map_or_else(
            || SwitchedFrom {
                harness: latest.conversation.harness.clone(),
                agent: None,
            },
            |switched| switched.from.clone(),
        );
        let open = self.seams.ledger.borrow().chief_open_work(project.id)?;
        // The page count is cf history's: a line names only this project's messages.
        let ledger = &self.seams.ledger;
        let message = |id: i64| -> Result<Option<MessageView>, Refusal> {
            let found = ledger
                .borrow()
                .message(id)
                .map_err(|error| EngineError::Ledger(error).refusal())?;
            Ok(found.filter(|found| found.project_id == project.id))
        };
        let last = last_words(&history);
        let pages = history_pages(&history, &message)?;
        let body = handoff_text(&Handoff {
            from: &from,
            to: chief,
            open: &open,
            last: last.as_ref(),
            cut: switched.as_ref().is_some_and(|switched| switched.cut),
            pages,
        });
        let note = self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "chief".to_owned(),
                task: None,
                body,
            },
        )?;
        Ok(Some(note))
    }

    /// A chief whose agent is gone does not open: the human hears why, and
    /// what it was to receive waits for the chief they switch in, its
    /// attempt given back.
    pub(crate) fn chief_without_agent(
        &self,
        project: &ProjectView,
        chief: &ParticipantView,
        delivering: Option<Delivering>,
    ) -> Result<(), EngineError> {
        let agent = chief.agent.as_deref().unwrap_or("null");
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: format!(
                    "The chief runs on {agent}, which is no longer among your agents: add it back under Agents, or switch the chief."
                ),
            },
        )?;
        if let Some(delivering) = delivering {
            self.give_back(
                delivering,
                &format!("{agent} is no longer among your agents"),
            )?;
        }
        self.changed();
        Ok(())
    }

    /// A switch the human asked for after the chief's turn; with `note`,
    /// the chief is asked first where things stand.
    pub(crate) fn after_turn(
        &self,
        _project: i64,
        _record: &Record,
        _to: SwitchTo,
        _note: bool,
    ) -> Result<(), EngineError> {
        Err(not_ported("a switch after the chief's turn"))
    }

    /// A switch waiting for the chief's turn goes once the turn ends.
    pub(crate) async fn await_switch(
        self: &Rc<Self>,
        _project: &ProjectView,
        _chief: &ParticipantView,
        _record: &Rc<Record>,
        _observed: &Observed,
        _idle: bool,
    ) -> Result<(), EngineError> {
        Err(not_ported("a switch waiting for the chief's turn"))
    }

    /// The switch: the old window closed, the chief switched, the new one launched.
    pub(crate) async fn perform_switch(
        self: &Rc<Self>,
        _project: &ProjectView,
        _chief: &ParticipantView,
        _record: &Rc<Record>,
        _to: SwitchTo,
    ) -> Result<(), EngineError> {
        Err(not_ported("Switch chief"))
    }
}
