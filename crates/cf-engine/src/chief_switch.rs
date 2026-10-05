//! The human's Switch chief (`src/core/chief-switch.js`): now, or after the
//! chief's turn, with a note first asking it where things stand when the
//! human wants one. It takes a last look and copy, gives back what was in
//! flight, closes the old window as the engine's own, switches the chief in
//! the ledger and launches the new one with the handoff. It also picks a
//! chief's first message, and owns the record's pending switch.

use std::rc::Rc;

use cf_base::refusal::Refusal;
use cf_harness::contract::Observed;
use cf_harness::records::Role;
use cf_ledger::{
    ChiefSwitch, ConversationView, MessageView, NewNote, ParticipantView, ProjectView,
};
use serde_json::Value;

use crate::deliveries::Delivering;
use crate::dispatcher::Dispatcher;
use crate::handoff::{handoff_text, history_pages, is_handoff, last_words, Handoff};
use crate::record::Record;
use crate::seams::EngineError;
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

    /// A switch the human asked for after the chief's turn, with a note that
    /// first asks the chief where things stand when they want one: it waits
    /// in the record ([`Dispatcher::await_switch`]).
    pub(crate) fn after_turn(
        &self,
        project: i64,
        record: &Record,
        to: SwitchTo,
        note: bool,
    ) -> Result<(), EngineError> {
        // A switch asked again replaces the one waiting, with a note not yet sent.
        let replaced = record
            .pending_switch
            .borrow()
            .as_ref()
            .and_then(|pending| pending.note);
        if let Some(replaced) = replaced {
            let waiting = self.seams.ledger.borrow().message(replaced)?;
            if waiting.is_some_and(|waiting| waiting.state == "queued") {
                self.seams
                    .ledger
                    .borrow_mut()
                    .cancel_message(replaced, "the human asked for the switch again")?;
            }
        }
        let asked = if note {
            let body = format!(
                "The human is moving this project's chief to {} ({}) once you answer. Write down where things stand, for the chief after you: what you and the human decided, what you promised, what you were about to do, and what is unresolved. Do not start anything new.",
                to.harness, to.agent
            );
            let sent = self.seams.ledger.borrow_mut().note(
                project,
                &NewNote {
                    from: None,
                    to: "chief".to_owned(),
                    task: None,
                    body,
                },
            )?;
            Some(sent.id)
        } else {
            None
        };
        *record.pending_switch.borrow_mut() = Some(PendingSwitch { to, note: asked });
        self.changed();
        Ok(())
    }

    /// A switch the human asked for after the chief's turn: once the turn is
    /// over (and the note asking where things stand came and was answered,
    /// when they asked for one), the chief goes. Until then nothing else is
    /// delivered to it, so its turn can end.
    pub(crate) async fn await_switch(
        self: &Rc<Self>,
        project: &ProjectView,
        chief: &ParticipantView,
        record: &Rc<Record>,
        observed: &Observed,
        idle: bool,
    ) -> Result<(), EngineError> {
        if !idle {
            return Ok(());
        }
        let Some(pending) = record.pending_switch.borrow().clone() else {
            return Ok(());
        };
        let asked = match pending.note {
            Some(note) => self.seams.ledger.borrow().message(note)?,
            None => None,
        };
        if let Some(asked) = asked {
            if asked.state == "queued" {
                let (this, held) = (Rc::clone(self), Rc::clone(record));
                self.act(record, async move { this.deliver(&held, asked).await })
                    .await;
                return Ok(());
            }
            if asked.state == "delivered" {
                let shown = asked.receipt.get("item").and_then(Value::as_str);
                let items = observed.items();
                let at = items
                    .iter()
                    .position(|item| shown.is_some_and(|shown| &*item.id == shown));
                let answered = at.is_some_and(|at| {
                    items[at + 1..]
                        .iter()
                        .any(|item| item.role == Role::Assistant && item.complete)
                });
                if !answered {
                    return Ok(());
                }
            }
        }
        self.perform_switch(project, chief, record, pending.to)
            .await
    }

    /// The switch itself, holding the chief's turn: one last look at the old
    /// window (its words become history; a delivery whose header shows there
    /// arrived), what it was still receiving goes back to the queue with its
    /// attempt, the window closes without suspending the project, the ledger
    /// moves the chief, and the new window opens with the handoff. A project
    /// deleted on the way (its chief forgotten) stops it there: its rows are
    /// gone, so there is no delivery to confirm and no chief to move, and its
    /// old window closes with the record.
    pub(crate) async fn perform_switch(
        self: &Rc<Self>,
        project: &ProjectView,
        chief: &ParticipantView,
        record: &Rc<Record>,
        to: SwitchTo,
    ) -> Result<(), EngineError> {
        let asked = record
            .pending_switch
            .borrow_mut()
            .take()
            .and_then(|pending| pending.note);
        let mut cut = false;
        let pane = record.window.borrow().pane.clone();
        if let Some(pane) = pane {
            let observed = self.observe(chief, record).await.ok();
            if self.forgotten(record) {
                return Ok(());
            }
            if let Some(observed) = &observed {
                self.copy(chief, record, observed)?;
                if record.delivery.borrow().delivering.is_some() {
                    self.confirm_arrival(record, observed)?;
                }
                cut = !observed.settled;
            }
            let delivering = record.delivery.borrow_mut().delivering.take();
            if let Some(delivering) = delivering {
                self.give_back(delivering, "the chief was switched before it arrived")?;
            }
            if !self.close_own(record, &pane).await? {
                // The old window would not close: the switch waits, as one
                // asked for after a turn does, and the chief's next step
                // tries it again.
                *record.pending_switch.borrow_mut() = Some(PendingSwitch { to, note: asked });
                self.changed();
                return Ok(());
            }
        }
        if self.forgotten(record) {
            return Ok(());
        }
        // A handoff still on its way is an earlier switch's: this one writes
        // its own. The note asking the old chief where things stand was for
        // it alone, however the switch came (now, or the chief out of quota).
        let pending = self.seams.ledger.borrow().pending(chief.id)?;
        for message in pending {
            if is_handoff(&message) {
                self.seams
                    .ledger
                    .borrow_mut()
                    .cancel_message(message.id, "the chief was switched again")?;
            } else if Some(message.id) == asked {
                self.seams
                    .ledger
                    .borrow_mut()
                    .cancel_message(message.id, "the chief was switched before it came")?;
            }
        }
        self.seams.ledger.borrow_mut().switch_chief(
            project.id,
            &ChiefSwitch {
                harness: to.harness,
                agent: to.agent,
                cut,
            },
        )?;
        {
            let mut quota = record.quota.borrow_mut();
            quota.reported = None;
            quota.low_until = None;
        }
        *record.copied.borrow_mut() = None;
        {
            let mut window = record.window.borrow_mut();
            window.interrupted = None;
            window.relaunch = None;
        }
        record.delivery.borrow_mut().held = None;
        self.changed();
        let current = self.known_project(project.id)?;
        if current.state != "open" {
            return Ok(());
        }
        let Some(moved) = current
            .participants
            .iter()
            .find(|participant| participant.role == "chief")
            .cloned()
        else {
            return Ok(());
        };
        let (this, held) = (Rc::clone(self), Rc::clone(record));
        self.act(record, async move {
            this.launch(&held, &current, &moved, None).await
        })
        .await;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
