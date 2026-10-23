//! What goes into a window and what comes back out: into a window, ready,
//! `beginDelivery`, the hand-over; its arrival is watched for by `arrival`, and
//! what it came to is settled here: tried again or failed. What was on its way
//! when the daemon or the window went is `in_flight`'s. Out of a window, a
//! worker's answer becomes its task's result. It owns the record's delivery
//! part; the window's part it only reads.

use std::rc::Rc;

use cf_base::js;
use cf_harness::contract::{Admission, Held, Observed, PaneHost, Readiness};
use cf_ledger::{is_told, GivenUp, MessageView, NewNote, ParticipantView, ProjectView, TaskView};
use cf_proto::trace::WindowEvent;

use crate::delivery_text::{delivery_text, marker_of};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::role_change::{answers_to_the_task, Prelude};
use crate::runtime::returning;
use crate::seams::EngineError;

/// The one message on its way into a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Delivering {
    /// It, whose paste proves it arrived (`is_paste_of`).
    pub(crate) message: i64,
    /// When it was handed over, on the engine's clock.
    pub(crate) since: i64,
    /// Since when the window has been at rest without the record showing it:
    /// from the hand-over, but for the turns the window was in since. None
    /// while it is in one, or its harness holds it (`arrival`).
    pub(crate) rested_since: Option<i64>,
    /// Since when its harness has not held it in a turn: the hand-over, where the
    /// harness did not hold it then, else the first look that found it held no longer
    /// after the last that did. None while the harness holds it (`arrival`).
    pub(crate) free_since: Option<i64>,
    /// It is a launch's first message.
    pub(crate) launch: bool,
    /// It is the chief's first message, a handoff most often, or what a launch put
    /// ahead of it (the replacement of the role): never given up for being slow.
    pub(crate) chief: bool,
    /// The harness's own queue took it, as it said at the hand-over or after
    /// (`Window::took`).
    pub(crate) queued: bool,
    /// Enter was pressed once more for it.
    pub(crate) entered_again: bool,
}

/// The record's delivery part.
#[derive(Debug, Default)]
pub(crate) struct DeliveryPart {
    pub(crate) delivering: Option<Delivering>,
    /// A message waits for what the human typed and has not sent.
    pub(crate) unsent: bool,
    /// The message a window not ready for a paste holds, told to the trace once.
    pub(crate) held: Option<i64>,
    /// What goes into the window before anything else of its queue, in this
    /// order and while it waits and may go: a launch that is to say a role
    /// first puts the replacement here, and the message it displaced after it
    /// ([`Dispatcher::next_in_line`]), and owes it until it is told
    /// ([`Dispatcher::owed`]). Each launch says it anew, and none that opened a
    /// window leaves it as it was.
    pub(crate) prelude: Option<Prelude>,
}

impl Dispatcher {
    /// Hands `message` to a window that is idle. A participant forgotten
    /// while its delivery waits (it left, or its project was deleted) is
    /// handed nothing, and what its harness did with the message settles
    /// nothing: its window goes with the record. A window closed while it
    /// got ready is handed nothing, and the message waits for the next.
    pub(crate) async fn deliver(
        self: &Rc<Self>,
        record: &Rc<Record>,
        message: MessageView,
    ) -> Result<(), EngineError> {
        let host: &dyn PaneHost = &*self.seams.host;
        let (Some(window), Some(pane)) = self.window_of(record) else {
            return Ok(());
        };
        let ready = window
            .ready(host, &pane)
            .await
            .map_err(|error| EngineError::said("not-ready", error))?;
        if self.forgotten(record) {
            return Ok(());
        }
        // Withdrawn, or overtaken, while the window got ready (its task cancelled
        // or paused meanwhile, words held for the human's approval, a door
        // claiming it): what is next of this queue is asked again, and nothing is
        // handed over unless it is this message. Nothing fails.
        let next = self.next_in_line(record)?;
        if next.is_none_or(|next| next.id != message.id) {
            return Ok(());
        }
        let held = match ready {
            Readiness::Ready => {
                self.came_up(record);
                None
            }
            Readiness::Held(held) => Some(held),
            // A window that never will be ready is a launch that did not come up.
            Readiness::Unfit(why) => return self.give_up_unfit(record, &message, &why).await,
        };
        // Held for what the human typed and has not sent: the board says so.
        record.delivery.borrow_mut().unsent = held == Some(Held::Unsent);
        if let Some(held) = held {
            // Said once per message, so a wait is in the trace, not a mystery.
            let first = record.delivery.borrow().held != Some(message.id);
            if first {
                record.delivery.borrow_mut().held = Some(message.id);
                self.trace_window(
                    record,
                    WindowEvent::DeliveryHeld {
                        message: message.id,
                        reason: format!("the window is not ready for a paste: {}", held.sentence()),
                    },
                )?;
            }
            return Ok(());
        }
        record.delivery.borrow_mut().held = None;
        let (Some(window), Some(pane)) = self.window_of(record) else {
            return Ok(());
        };
        // What is pasted is what the ledger says it is when delivery begins: the
        // message, and the rows it carries still waiting, which are the set from now.
        let begun = self.seams.ledger.borrow_mut().begin_delivery(message.id)?;
        if begun.message.kind == "task" {
            self.pay_with_words(record)?;
        }
        let text = delivery_text(&begun.message, &begun.carried);
        // A turn begins with this message: what the window's tree runs now came before it.
        self.mark_turn(record, &pane).await;
        let outcome = match returning(window.deliver(host, &pane, &text)).await {
            Ok(admission) => admission,
            // An adapter that failed never handed it over, and its error must show.
            Err(error) => Admission::Refused {
                reason: format!("the delivery failed: {error}"),
            },
        };
        if self.forgotten(record) {
            return Ok(());
        }
        let since = self.now();
        // The window was at rest when it was handed over, unless its harness holds what
        // it was given in the turn it is in (`arrival`).
        let free = (!window.held()).then_some(since);
        let delivering = Delivering {
            message: message.id,
            since,
            rested_since: free,
            free_since: free,
            launch: false,
            // What a chief's launch put ahead of its queue is its first messages.
            chief: self.begins(record, message.id),
            // The harness's own queue took it (a peer inbox, a broker, a
            // plugin): it shows when the harness gets to it, and sending it
            // again would only make a duplicate.
            queued: matches!(outcome, Admission::Admitted { queued: true }),
            entered_again: false,
        };
        if let Admission::Refused { reason } = &outcome {
            return self.settle_failure(delivering, reason, true);
        }
        // A window that closed while its harness took the message had nothing
        // on its way to settle when it went: the message goes again, unless the
        // harness's record shows it took it (`in_flight`).
        if record.window.borrow().pane.as_ref() != Some(&pane) {
            let this = Rc::clone(self);
            self.settle_gone(delivering, move |delivering| {
                this.settle_failure(
                    delivering,
                    "its window closed while it was handed over",
                    true,
                )
            })
            .await?;
            return Ok(());
        }
        record.delivery.borrow_mut().delivering = Some(delivering);
        self.changed();
        Ok(())
    }

    /// A worker's answer to its task's latest message finishes the task: the task
    /// as it is once its result is recorded, where it was. Whatever the turn left
    /// running is no part of it ([`Dispatcher::end_turn_work`]). An answer to the
    /// note that says the worker its role is none of the task's
    /// ([`answers_to_the_task`]): where nothing else answers, there is no result,
    /// and nothing the turn left running is ended. Nor while words told to the
    /// window before it was up are still on their way to it (the brief was in
    /// flight when they were told): they go in next, and the answer to them is
    /// the result.
    pub(crate) fn collect(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        observed: &Observed,
    ) -> Result<Option<TaskView>, EngineError> {
        let task = self
            .seams
            .ledger
            .borrow()
            .active_task(participant.id, false)?;
        let Some(thread) = task.filter(|thread| thread.task.state == "working") else {
            return Ok(None);
        };
        // The last message pasted into the window (one that only rode in a
        // paste, or that it read elsewhere, has no marker of its own).
        let latest = self
            .seams
            .ledger
            .borrow()
            .last_pasted(participant.id, thread.task.id)?;
        let Some(latest) = latest else {
            return Ok(None);
        };
        let items = observed.items();
        let marker = marker_of(latest.id);
        let Some(start) = items.iter().position(|item| item.text.contains(&marker)) else {
            return Ok(None);
        };
        if observed.failed {
            let because = format!("@{}'s harness reported a failure", participant.handle);
            self.fail_task(project, &thread.task, &because)?;
            return Ok(None);
        }
        if !observed.settled {
            return Ok(None);
        }
        let told = self
            .seams
            .ledger
            .borrow()
            .told_waiting(participant.id, thread.task.id)?;
        if told {
            return Ok(None);
        }
        // The turn is over once its last message is complete (one that
        // ended in a tool call never is). The result is everything the
        // member wrote in it, in order; a tool's output is not the member's
        // words, and nor are the progress notes a harness marks as its
        // commentary: its final answer is the report.
        let replacements = self.seams.ledger.borrow().role_notes_to(participant.id)?;
        let written = answers_to_the_task(&items[start + 1..], &replacements);
        if !written.last().is_some_and(|item| item.complete) {
            return Ok(None);
        }
        let words: Vec<&str> = written
            .iter()
            .filter(|item| !item.commentary)
            .map(|item| js::trim(&item.text))
            .filter(|text| !text.is_empty())
            .collect();
        let body = if words.is_empty() {
            "(the agent ended its turn without a written answer)".to_owned()
        } else {
            words.join("\n\n")
        };
        let moved =
            self.seams
                .ledger
                .borrow_mut()
                .record_result(project.id, thread.task.number, &body)?;
        self.changed();
        Ok(Some(moved.task))
    }

    /// What a window was to receive goes back to its queue with its attempt:
    /// the window went before it had the chance to land.
    pub(crate) fn give_back(
        &self,
        delivering: Delivering,
        because: &str,
    ) -> Result<(), EngineError> {
        let message = self.seams.ledger.borrow().message(delivering.message)?;
        if message.is_none_or(|found| found.state != "delivering") {
            return Ok(());
        }
        self.seams
            .ledger
            .borrow_mut()
            .retry_delivery(delivering.message, because, true)?;
        Ok(())
    }

    /// A delivery that did not arrive: tried again while attempts remain
    /// (`retry`), or given up.
    pub(crate) fn settle_failure(
        &self,
        delivering: Delivering,
        because: &str,
        retry: bool,
    ) -> Result<(), EngineError> {
        let message = self.seams.ledger.borrow().message(delivering.message)?;
        let Some(message) = message.filter(|found| found.state == "delivering") else {
            return Ok(());
        };
        if retry && message.attempts < i64::from(self.seams.limits.max_attempts) {
            self.seams
                .ledger
                .borrow_mut()
                .retry_delivery(message.id, because, false)?;
        } else {
            self.fail_delivery(&message, because)?;
        }
        self.changed();
        Ok(())
    }

    /// A message that will not be delivered. A brief fails its task, and
    /// whoever gave it hears why; so do words told to the window while its
    /// task waits on it (`is_told`): an answer the window gave without them is
    /// no result. Whoever waits on any other kind would wait forever, so the
    /// human hears, once, what it was, for whom and why. Words told to the window
    /// that the ledger kept for their task's resume did not fail: its requester
    /// hears why and which words wait (the ledger's `tell_kept`), and the human,
    /// once, as of a failure, that they wait for the resume ([`Dispatcher::tell_kept`]).
    /// What the ledger withdrew did not fail, and nobody hears of it. The ledger says
    /// which it did ([`GivenUp`]): no state is read for it.
    fn fail_delivery(&self, message: &MessageView, because: &str) -> Result<(), EngineError> {
        let (_, given_up) = self
            .seams
            .ledger
            .borrow_mut()
            .give_up_delivery(message.id, because)?;
        match given_up {
            GivenUp::Failed => {}
            GivenUp::Kept => return self.tell_kept(message, because),
            GivenUp::Withdrawn => return Ok(()),
        }
        let project = self.known_project(message.project_id)?;
        if let Some(number) = message.task_number {
            let task = self.seams.ledger.borrow().task(project.id, number)?;
            let failed = task.filter(|thread| {
                thread.task.state == "failed"
                    && (message.kind == "task" || is_told(message, thread.task.assignee.as_deref()))
            });
            if let Some(thread) = failed {
                let because = if message.kind == "task" {
                    because.to_owned()
                } else {
                    format!(
                        "what @{} told its window, m-{}, did not reach it ({because})",
                        message.sender.as_deref().unwrap_or("null"),
                        message.id
                    )
                };
                self.tell_requester(&project, &thread.task, &because)?;
                // A task the human gave: that note was theirs.
                if thread.task.requester == "human" {
                    return Ok(());
                }
            }
        }
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: message.task_number,
                body: format!(
                    "m-{}, {}, did not reach @{}: {because}.",
                    message.id,
                    described(message),
                    message.recipient
                ),
            },
        )?;
        Ok(())
    }

    /// Words told to a window, kept for their task's resume as their paste failed
    /// `because`: the human hears it once, as main told it of words that did not
    /// reach the window, which these do at the resume. A task the human gave was
    /// told of it already: that note was theirs.
    fn tell_kept(&self, message: &MessageView, because: &str) -> Result<(), EngineError> {
        let Some(number) = message.task_number else {
            return Ok(());
        };
        let project = self.known_project(message.project_id)?;
        let task = self.seams.ledger.borrow().task(project.id, number)?;
        if task.is_some_and(|thread| thread.task.requester == "human") {
            return Ok(());
        }
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: Some(number),
                body: format!(
                    "m-{}, {}, waits for T-{number}'s resume to reach @{}: {because}.",
                    message.id,
                    described(message),
                    message.recipient
                ),
            },
        )?;
        Ok(())
    }

    fn fail_task(
        &self,
        project: &ProjectView,
        task: &TaskView,
        because: &str,
    ) -> Result<(), EngineError> {
        self.seams
            .ledger
            .borrow_mut()
            .fail_task(project.id, task.number, because)?;
        self.tell_requester(project, task, because)
    }

    fn tell_requester(
        &self,
        project: &ProjectView,
        task: &TaskView,
        because: &str,
    ) -> Result<(), EngineError> {
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: task.requester.clone(),
                task: Some(task.number),
                body: format!(
                    "T-{0} failed: {because}. Reopen it with: cf task reopen T-{0} \"…\"",
                    task.number
                ),
            },
        )?;
        Ok(())
    }
}

/// What `message` is to the human: its kind, who wrote it, and its task
/// (`an answer from @chief on T-1`).
fn described(message: &MessageView) -> String {
    let kind = if message.kind == "answer" {
        "an answer".to_owned()
    } else {
        format!("a {}", message.kind)
    };
    let from = message
        .sender
        .as_ref()
        .map_or_else(|| "ConsensFlow".to_owned(), |sender| format!("@{sender}"));
    let on = message
        .task_number
        .map_or_else(String::new, |number| format!(" on T-{number}"));
    format!("{kind} from {from}{on}")
}
