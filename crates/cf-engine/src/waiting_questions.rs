//! A question received by the chief must not be forgotten while it waits.

use cf_base::time;
use cf_ledger::{NewNote, ProjectView};

use crate::dispatcher::Dispatcher;
use crate::seams::EngineError;

const FIRST_REMINDER_MS: i64 = 30 * 60_000;
const REPEAT_REMINDER_MS: i64 = 2 * 60 * 60_000;

impl Dispatcher {
    pub(crate) fn remind_waiting_questions(
        &self,
        project: &ProjectView,
    ) -> Result<(), EngineError> {
        let waits = self.seams.ledger.borrow().waiting_questions(project.id)?;
        for wait in waits {
            let Some(since) = time::parse(&wait.received_at) else {
                continue;
            };
            let now = self.now();
            let due = if let Some(prior) = &wait.reminder {
                if matches!(prior.state.as_str(), "queued" | "gated" | "delivering") {
                    continue;
                }
                let Some(when) = prior
                    .delivered_at
                    .as_deref()
                    .and_then(time::parse)
                    .or_else(|| time::parse(&prior.created_at))
                else {
                    continue;
                };
                when + REPEAT_REMINDER_MS
            } else {
                since + FIRST_REMINDER_MS
            };
            if now < due {
                continue;
            }
            let minutes = (now - since) / 60_000;
            let age = if minutes < 120 {
                format!("{minutes} minutes")
            } else {
                format!("{} h {} min", minutes / 60, minutes % 60)
            };
            let subject = wait.task.map_or_else(
                || format!("@{}'s", wait.sender),
                |number| format!("T-{number}'s"),
            );
            let Some(chief) = project
                .participants
                .iter()
                .find(|p| p.id == wait.recipient_id)
            else {
                continue;
            };
            let cf = self.seams.roles.cf(chief);
            let next = if let Some(number) = wait.held_task {
                format!("If only the person can, ask them at the top of your reply, and don't keep the member waiting: finish the task with `{cf} task finish T-{number}`, and once they answer, give the next step as a new task with their answer.")
            } else {
                "If only the person can, ask them at the top of your reply, and don't keep the member waiting; once they answer, give the next step as a new task with their answer.".to_owned()
            };
            let note = NewNote {
                from: None,
                to: wait.recipient,
                task: wait.task,
                body: format!(
                    "{subject} question m-{} has waited {age}. Answer it. {next}",
                    wait.id
                ),
            };
            let sent = self.seams.ledger.borrow_mut().remind_question(
                project.id,
                wait.id,
                wait.reminder.as_ref().map(|m| m.id),
                &note,
            )?;
            if sent.is_some() {
                self.changed();
            }
        }
        Ok(())
    }
}
