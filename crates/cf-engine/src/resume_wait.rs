//! What a task's resume words wait for, and the chief's one notice when that
//! wait outlasts a paid stop. This reports existing delivery state; it does
//! not decide when a window may take input.

use cf_ledger::{MessageView, NewNote, ParticipantView, ProjectView, TaskView};
use serde_json::{json, Value};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::windows::ActivityState;

const RESUME_WAIT_NOTICE_MS: i64 = 2 * 60_000;

fn notice_head(number: i64, message: i64) -> String {
    format!("T-{number}'s resume words (m-{message})")
}

impl Dispatcher {
    /// A prior notice for these very resume words, including across a restart.
    pub(crate) fn resume_wait_was_told(
        &self,
        message: &MessageView,
        recipient: i64,
    ) -> Result<bool, EngineError> {
        let Some(number) = message.task_number else {
            return Ok(false);
        };
        let head = notice_head(number, message.id);
        Ok(self
            .seams
            .ledger
            .borrow()
            .note_since(recipient, message.id, &head)?
            .is_some())
    }

    /// The task and its resume message while the words have not begun delivery.
    fn waiting_resume(
        &self,
        participant: i64,
    ) -> Result<Option<(TaskView, MessageView)>, EngineError> {
        let ledger = self.seams.ledger.borrow();
        let Some(thread) = ledger.task_in_hand(participant)? else {
            return Ok(None);
        };
        let Some(words) = ledger.resume_words_not_in(thread.task.project_id, thread.task.number)?
        else {
            return Ok(None);
        };
        Ok(ledger.message(words)?.map(|message| (thread.task, message)))
    }

    /// A short reason from the existing gate, window activity or delivery hold.
    fn resume_wait_reason(&self, message: &MessageView) -> Result<String, EngineError> {
        if message.state == "gated" {
            return Ok("needs human approval".to_owned());
        }
        let Some(record) = self.record(message.recipient_id) else {
            return Ok("window not open".to_owned());
        };
        let activity = self.activity(message.recipient_id);
        let reason = match activity.state {
            ActivityState::Working => "window shows working".to_owned(),
            ActivityState::Starting => "window starting".to_owned(),
            ActivityState::Closed => "window not open".to_owned(),
            ActivityState::Waiting => {
                let delivery = record.delivery.borrow();
                if delivery
                    .for_human
                    .as_deref()
                    .is_some_and(|reason| Some(reason) == activity.reason.as_deref())
                {
                    "window needs the human".to_owned()
                } else {
                    "window's dialog waits for an answer".to_owned()
                }
            }
            ActivityState::Out => match activity.reason.as_deref() {
                Some(reason) if reason.starts_with("model busy") => "member's model is busy",
                Some(reason) if reason.starts_with("out of quota") => "member out of quota",
                Some(reason)
                    if reason.starts_with("moved") || reason.starts_with("model moved") =>
                {
                    "member's model moved"
                }
                _ => "member unavailable",
            }
            .to_owned(),
            ActivityState::Unknown => "window state unknown".to_owned(),
            ActivityState::Idle => {
                let held = record
                    .delivery
                    .borrow()
                    .held
                    .clone()
                    .filter(|held| held.message == message.id);
                if let Some(held) = held {
                    if held.unsent {
                        "window has unsent text".to_owned()
                    } else {
                        format!("window: {}", held.reason)
                    }
                } else if self
                    .next_in_line(&record)?
                    .is_some_and(|next| next.id != message.id)
                {
                    "another message is ahead of them".to_owned()
                } else {
                    "window settling".to_owned()
                }
            }
        };
        Ok(reason)
    }

    /// The resumed task on this lane and what its words wait for, while they
    /// are queued or gated. A delivered or withdrawn resume is not shown.
    pub fn resume_wait(&self, participant: i64) -> Result<Option<Value>, EngineError> {
        let Some((task, message)) = self.waiting_resume(participant)? else {
            return Ok(None);
        };
        Ok(Some(json!({
            "task": task.number,
            "reason": self.resume_wait_reason(&message)?,
        })))
    }

    /// Tell the chief once when resume words still wait two minutes after this
    /// task's stop was paid. The first look gives even a late resume its ordinary
    /// chance to go in before a notice; the next look uses the stop's clock.
    pub(crate) fn tell_resume_wait(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
    ) -> Result<(), EngineError> {
        let pending = if project.state == "open" {
            self.waiting_resume(participant.id)?
        } else {
            None
        };
        let Some((task, message)) = pending else {
            record.delivery.borrow_mut().resume_wait_seen = None;
            return Ok(());
        };
        let seen = {
            let mut delivery = record.delivery.borrow_mut();
            let seen = delivery.resume_wait_seen == Some(message.id);
            delivery.resume_wait_seen = Some(message.id);
            seen
        };
        if !seen {
            return Ok(());
        }
        let Some(stop) = self.seams.ledger.borrow().stop_of(participant.id)? else {
            return Ok(());
        };
        let paid = {
            let window = record.window.borrow();
            (stop.seq > 0
                && stop.task_id == task.id
                && window.stopped == Some((stop.task_id, stop.seq)))
            .then_some(window.stop_paid_at)
            .flatten()
        };
        let Some(paid) = paid else {
            return Ok(());
        };
        if self.now().saturating_sub(paid) < RESUME_WAIT_NOTICE_MS {
            return Ok(());
        }
        let Some(chief) = project
            .participants
            .iter()
            .find(|member| member.role == "chief")
        else {
            return Ok(());
        };
        if self.resume_wait_was_told(&message, chief.id)? {
            return Ok(());
        }
        let head = notice_head(task.number, message.id);
        let reason = self.resume_wait_reason(&message)?;
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: chief.handle.clone(),
                task: Some(task.number),
                body: format!(
                    "{head} were still waiting for @{} when checked: {}.",
                    participant.handle,
                    reason.trim_end_matches('.')
                ),
            },
        )?;
        Ok(())
    }
}
