//! What goes into a window and what comes back out: into a window, ready,
//! `beginDelivery`, the hand-over; its arrival is watched for by `arrival`, and
//! what it came to is settled here: tried again or failed. What was on its way
//! when the daemon or the window went is `in_flight`'s. Out of a window, a
//! worker's answer becomes its task's result. It owns the record's delivery
//! part; the window's part it only reads.

use std::rc::Rc;

use cf_base::js;
use cf_harness::contract::{Admission, Held, Observed, PaneHost, Readiness};
use cf_harness::records::Role;
use cf_ledger::{MessageView, NewNote, ParticipantView, ProjectView, TaskView};
use cf_proto::trace::WindowEvent;

use crate::delivery_text::{delivery_text, marker_of};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::role_change::{answers_to_the_task, Prelude};
use crate::runtime::returning;
use crate::seams::EngineError;
use crate::shown::{after_words, Shown};

/// How many of a participant's newest messages are read to tell whether a
/// failed turn was told already.
const TOLD_LOOKBACK: i64 = 50;

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
    /// and nothing the turn left running is ended.
    pub(crate) async fn collect(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        observed: &Observed,
    ) -> Result<Option<TaskView>, EngineError> {
        let task = self
            .seams
            .ledger
            .borrow()
            .active_task(participant.id, false)?;
        let Some(thread) = task.filter(|thread| thread.task.state == "working") else {
            if observed.failed {
                self.tell_failed_turn(project, participant, record, observed)
                    .await?;
            }
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
        let Some(pasted) = items.iter().position(|item| item.text.contains(&marker)) else {
            return Ok(None);
        };
        // The task's turn is the last one after its latest paste, which the last
        // user's words begin: a turn its window went on with by itself, after a
        // refusal whose words are no part of the answer (`went_on_by_itself`), a
        // word of the human's, a role told again. The answer is what the window
        // wrote from there, as turn-end's final run is (T-96).
        let turn = items[pasted + 1..]
            .iter()
            .rposition(|item| item.role == Role::User)
            .map_or(pasted + 1, |at| pasted + 1 + at);
        if observed.failed {
            let because = self.why_it_failed(participant, record, observed).await;
            // The task may have moved on while the window was looked at.
            let still = self
                .seams
                .ledger
                .borrow()
                .active_task(participant.id, false)?
                .is_some_and(|now| now.task.id == thread.task.id && now.task.state == "working");
            if still && !self.forgotten(record) {
                self.fail_task(project, &thread.task, &because)?;
            }
            return Ok(None);
        }
        if !observed.settled {
            return Ok(None);
        }
        // The turn is over once its last message is complete (one that
        // ended in a tool call never is). The result is everything the
        // member wrote in it, in order; a tool's output is not the member's
        // words, and nor are the progress notes a harness marks as its
        // commentary: its final answer is the report.
        let replacements = self.seams.ledger.borrow().role_notes_to(participant.id)?;
        let written = answers_to_the_task(&items[turn..], &replacements);
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

    /// A member's turn that failed while its task was not working (waiting on an
    /// answer, paused, done): whoever gave the task is told why, once, and the
    /// task stays as it is. Only a turn since the task's last message was pasted
    /// into the window is the task's; nothing is said where the window was given
    /// no task.
    async fn tell_failed_turn(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let Some(thread) = self.seams.ledger.borrow().last_task(participant.id)? else {
            return Ok(());
        };
        let latest = self
            .seams
            .ledger
            .borrow()
            .last_pasted(participant.id, thread.task.id)?;
        let Some(latest) = latest else {
            return Ok(());
        };
        let marker = marker_of(latest.id);
        if !observed
            .items()
            .iter()
            .any(|item| item.text.contains(&marker))
        {
            return Ok(());
        }
        let requester = &thread.task.requester;
        if self.told_of_failure(project, participant, requester)? {
            return Ok(());
        }
        let because = self.why_it_failed(participant, record, observed).await;
        // As the task is once the window was looked at.
        let task = self
            .seams
            .ledger
            .borrow()
            .task(project.id, thread.task.number)?;
        let Some(now) = task else {
            return Ok(());
        };
        let number = now.task.number;
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: requester.clone(),
                task: Some(number),
                body: format!(
                    "A turn of T-{number}'s window failed while T-{number} was {}, as it stays: {because}.",
                    now.task.state
                ),
            },
        )?;
        self.changed();
        Ok(())
    }

    /// The chief's turn that failed: the human is told why, once, on the board.
    /// Nothing else of the chief's changes.
    pub(crate) async fn tell_chief_failed(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        if self.told_of_failure(project, participant, "human")? {
            return Ok(());
        }
        let because = self.why_it_failed(participant, record, observed).await;
        if self.forgotten(record) {
            return Ok(());
        }
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: format!("The chief's turn failed: {because}."),
            },
        )?;
        self.changed();
        Ok(())
    }

    /// Whether `recipient` was told of a failed turn of `participant`'s since the
    /// window was last given a message: a failed turn stays the window's last
    /// word until the next message, look after look and across a restart, and is
    /// told once. Each such note, and a task's failure, says the harness reported
    /// a failure. One nobody can be told of counts as told.
    fn told_of_failure(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        recipient: &str,
    ) -> Result<bool, EngineError> {
        let Some(to) = project.participants.iter().find(|p| p.handle == recipient) else {
            return Ok(true);
        };
        let ledger = self.seams.ledger.borrow();
        let given = ledger
            .inbox(participant.id, TOLD_LOOKBACK)?
            .into_iter()
            .filter(|message| matches!(message.state.as_str(), "delivering" | "delivered" | "read"))
            .map(|message| message.id)
            .max()
            .unwrap_or(0);
        let said = format!("@{}'s harness reported a failure", participant.handle);
        Ok(ledger.inbox(to.id, TOLD_LOOKBACK)?.iter().any(|message| {
            message.id > given
                && message.kind == "note"
                && message.sender.is_none()
                && message.body.contains(&said)
        }))
    }

    /// Why a member's turn failed, as its note says it: in its harness's own
    /// words where its record has them, and otherwise in the last lines its
    /// window shows, which is where a harness that says nothing in its record
    /// says why (a crash, a model it does not know), or that it could not be read.
    /// Both are held to a quote's bounds ([`after_words`], [`Shown::after`]).
    async fn why_it_failed(
        &self,
        participant: &ParticipantView,
        record: &Record,
        observed: &Observed,
    ) -> String {
        let failed = format!("@{}'s harness reported a failure", participant.handle);
        if let Some(words) = observed.failure() {
            return after_words(&failed, words);
        }
        let pane = record.window.borrow().pane.clone();
        let shown = match pane {
            Some(pane) => self.look_at(&pane).await,
            None => Shown::default(),
        };
        // A host that refused the look, or answered it late, said nothing of the
        // screen: the note says so, not the bare words the field had.
        if shown.is_known() {
            shown.after(&failed)
        } else {
            format!("{failed}; its screen could not be read")
        }
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
    /// whoever gave it hears why. Whoever waits on any other kind would wait
    /// forever, so the human hears, once, what it was, for whom and why.
    fn fail_delivery(&self, message: &MessageView, because: &str) -> Result<(), EngineError> {
        self.seams
            .ledger
            .borrow_mut()
            .fail_delivery(message.id, because)?;
        let project = self.known_project(message.project_id)?;
        if let Some(number) = message.task_number.filter(|_| message.kind == "task") {
            let task = self.seams.ledger.borrow().task(project.id, number)?;
            if let Some(thread) = task.filter(|thread| thread.task.state == "failed") {
                self.tell_requester(&project, &thread.task, because)?;
                // A task the human gave: that note was theirs.
                if thread.task.requester == "human" {
                    return Ok(());
                }
            }
        }
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
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: message.task_number,
                body: format!(
                    "m-{}, {kind} from {from}{on}, did not reach @{}: {because}.",
                    message.id, message.recipient
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
