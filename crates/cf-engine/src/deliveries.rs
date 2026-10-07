//! What goes into a window and what comes back out (`src/core/deliveries.js`):
//! into a window, ready, `beginDelivery`, the hand-over, then the record
//! watched for the message's header to confirm it, try it again or fail it,
//! with one more Enter for a paste left unsent; at start, what was in flight
//! is settled. Out of a window, a worker's answer becomes its task's result.
//! It owns the record's delivery part; the window's part it only reads.

use std::rc::Rc;

use cf_base::js;
use cf_harness::contract::{Admission, Held, Observed, PaneHost, Readiness};
use cf_harness::records::{Options, Reading, Role};
use cf_ledger::{MessageView, NewNote, ParticipantView, ProjectView, TaskView};
use cf_proto::agents::Harness;
use cf_proto::trace::WindowEvent;
use serde_json::{json, Value};

use crate::delivery_text::{delivery_text, marker_of};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::{begin, caught, returning};
use crate::seams::EngineError;

/// An Enter, pressed once more for a paste its window did not send.
const ENTER: u8 = 13;
/// How long a paste may go unshown in the record before its window gets that Enter.
const ENTER_AGAIN_MS: i64 = 10_000;
/// How long the window must have printed nothing first: one being typed into,
/// or drawing, is left be.
const ENTER_AGAIN_QUIET_MS: f64 = 3_000.0;

/// The one message on its way into a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Delivering {
    pub(crate) message: i64,
    /// Its header's start, which proves it arrived (`markerOf`).
    pub(crate) marker: String,
    /// When it was handed over, on the engine's clock.
    pub(crate) since: i64,
    /// It is a launch's first message.
    pub(crate) launch: bool,
    /// It is the chief's first message: a handoff, most often.
    pub(crate) chief: bool,
    /// The harness's own queue took it.
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
        // claiming it): the ledger's own head of this queue is asked again, and
        // nothing is handed over unless it is this message. Nothing fails.
        let next = self.seams.ledger.borrow().next_delivery(record.id)?;
        if next.is_none_or(|next| next.id != message.id) {
            return Ok(());
        }
        let held = match ready {
            Readiness::Ready => None,
            Readiness::Held(held) => Some(held),
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
        let delivering = Delivering {
            message: message.id,
            marker: marker_of(message.id),
            since: self.now(),
            launch: false,
            chief: false,
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
        // on its way to settle when it went: the message goes again.
        if record.window.borrow().pane.as_ref() != Some(&pane) {
            return self.settle_failure(
                delivering,
                "its window closed while it was handed over",
                true,
            );
        }
        record.delivery.borrow_mut().delivering = Some(delivering);
        self.changed();
        Ok(())
    }

    /// Whether the message on its way shows in the window's record; it is
    /// confirmed if so. Only what the window was given counts: a tool's
    /// output that prints a header proves nothing arrived. One withdrawn on
    /// its way (its task cancelled, or taken back from the window) is waited
    /// for no longer, and stays withdrawn whether it shows or not.
    pub(crate) fn confirm_arrival(
        &self,
        record: &Record,
        observed: &Observed,
    ) -> Result<bool, EngineError> {
        let Some(delivering) = record.delivery.borrow().delivering.clone() else {
            return Ok(true);
        };
        let message = self.seams.ledger.borrow().message(delivering.message)?;
        if message.is_none_or(|found| found.state != "delivering") {
            record.delivery.borrow_mut().delivering = None;
            return Ok(true);
        }
        let arrived = observed
            .items()
            .iter()
            .find(|item| item.role == Role::User && item.text.contains(&delivering.marker));
        let Some(arrived) = arrived else {
            return Ok(false);
        };
        let receipt = json!({ "item": &*arrived.id });
        self.seams
            .ledger
            .borrow_mut()
            .confirm_delivery(delivering.message, Some(&receipt))?;
        record.delivery.borrow_mut().delivering = None;
        self.changed();
        Ok(true)
    }

    /// Watches the record for what is on its way: confirmed, waited for,
    /// pressed Enter for once, tried again or failed.
    pub(crate) async fn watch_arrival(
        self: &Rc<Self>,
        record: &Rc<Record>,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        if self.confirm_arrival(record, observed)? {
            return Ok(());
        }
        let Some(delivering) = record.delivery.borrow().delivering.clone() else {
            return Ok(());
        };
        let waited = self.now() - delivering.since;
        if delivering.launch {
            // The chief's window is the human's: however long it takes to
            // show its first message (the handoff), it is never closed for that.
            if delivering.chief || waited <= self.seams.limits.launch_ms {
                return Ok(());
            }
            // What the window shows is asked before it closes: the screen goes with it.
            let pane = record.window.borrow().pane.clone();
            let (shown, settles) = self.look_before_giving_up(record, pane.as_ref()).await;
            // An exit that came while the host answered settled the launch already.
            if !settles {
                return Ok(());
            }
            let (this, held) = (Rc::clone(self), Rc::clone(record));
            let closing = begin(&*self.seams.spawn, async move { this.retire(&held).await }).await;
            let because = "the window never showed its first message";
            self.settle_failure(
                delivering,
                &self.unstarted_because(record, because, &shown),
                false,
            )?;
            closing.await?;
            return Ok(());
        }
        if delivering.queued {
            return Ok(());
        }
        if waited <= self.seams.limits.arrival_ms {
            if waited > ENTER_AGAIN_MS && !delivering.entered_again {
                self.enter_again(record, delivering.message).await?;
            }
            return Ok(());
        }
        record.delivery.borrow_mut().delivering = None;
        // A paste the harness record never showed after the whole window did
        // not land: sending it again is how it reaches the reader. (A message
        // the harness queued itself waits for the record, or for the window
        // to close.)
        self.settle_failure(delivering, "the harness record never showed it", true)
    }

    /// One more Enter, for a paste its window has not sent: an Enter that
    /// came before the window had read the paste finds nothing to send, and
    /// the text waits in the input, where a second paste would only stack a
    /// copy beside it. Pressed once, and only into a window quiet for a
    /// while; the next look tries again.
    async fn enter_again(
        self: &Rc<Self>,
        record: &Rc<Record>,
        message: i64,
    ) -> Result<(), EngineError> {
        let Some(pane) = record.window.borrow().pane.clone() else {
            return Ok(());
        };
        self.entered_again(record, message, true);
        let body = json!({ "id": pane.id, "generation": pane.generation });
        // `.catch(() => null)` made a promise of its own to wait on, a turn
        // after the host's answer.
        let snapshot = returning(self.seams.host.request("pane.snapshot", body))
            .await
            .ok();
        let quiet = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.get("outputQuietMs"))
            .and_then(Value::as_f64);
        // What the human has typed since would go with it: they send both.
        let ready = snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.get("ok") == Some(&Value::Bool(true)))
            && !js::truthy(
                snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.get("unsent")),
            )
            && quiet.is_some_and(|quiet| quiet >= ENTER_AGAIN_QUIET_MS);
        if !ready {
            self.entered_again(record, message, false);
            return Ok(());
        }
        let input = json!({ "id": pane.id, "generation": pane.generation, "bytes": [ENTER] });
        // A key the host did not take is pressed again at the next look's
        // Enter, if any: `.catch(() => {})` made a promise of its own to wait on.
        let _ = returning(self.seams.host.request("pane.input", input)).await;
        self.trace_window(record, WindowEvent::EnterAgain { message })
    }

    /// Marks whether Enter was pressed again for `message`, while it is the one on its way.
    fn entered_again(&self, record: &Record, message: i64, pressed: bool) {
        if let Some(delivering) = record
            .delivery
            .borrow_mut()
            .delivering
            .as_mut()
            .filter(|delivering| delivering.message == message)
        {
            delivering.entered_again = pressed;
        }
    }

    /// A worker's answer to its task's latest message finishes the task.
    pub(crate) fn collect(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let task = self
            .seams
            .ledger
            .borrow()
            .active_task(participant.id, false)?;
        let Some(thread) = task.filter(|thread| thread.task.state == "working") else {
            return Ok(());
        };
        // The last message pasted into the window (one that only rode in a
        // paste, or that it read elsewhere, has no marker of its own).
        let latest = self
            .seams
            .ledger
            .borrow()
            .last_pasted(participant.id, thread.task.id)?;
        let Some(latest) = latest else {
            return Ok(());
        };
        let items = observed.items();
        let marker = marker_of(latest.id);
        let Some(start) = items.iter().position(|item| item.text.contains(&marker)) else {
            return Ok(());
        };
        if observed.failed {
            let because = format!("@{}'s harness reported a failure", participant.handle);
            return self.fail_task(project, &thread.task, &because);
        }
        if !observed.settled {
            return Ok(());
        }
        // The turn is over once its last message is complete (one that
        // ended in a tool call never is). The result is everything the
        // member wrote in it, in order; a tool's output is not the member's
        // words, and nor are the progress notes a harness marks as its
        // commentary: its final answer is the report.
        let written: Vec<_> = items[start + 1..]
            .iter()
            .filter(|item| item.role == Role::Assistant)
            .collect();
        if !written.last().is_some_and(|item| item.complete) {
            return Ok(());
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
        self.seams
            .ledger
            .borrow_mut()
            .record_result(project.id, thread.task.number, &body)?;
        self.changed();
        Ok(())
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

    /// Once, at start: what was on its way to a window when the previous
    /// process ended. A message whose header ConsensFlow's copy of the
    /// window shows had arrived, and so had one the harness's own record
    /// shows (the copy can lag the record, and a message the harness took
    /// must not go again). Any other goes back to its queue with its
    /// attempt, as does one whose record cannot be read.
    pub(crate) async fn settle_in_flight(self: &Rc<Self>) -> Result<(), EngineError> {
        // No door lives through a start: what one claimed is the ledger's again.
        self.seams.ledger.borrow_mut().release_all_claims()?;
        let messages = self.seams.ledger.borrow().in_flight()?;
        for message in messages {
            let marker = marker_of(message.id);
            let copied = self
                .seams
                .ledger
                .borrow()
                .copied_item_with(message.recipient_id, &marker)?;
            let item = match copied {
                Some(item) => Some(item),
                // An `async` method: its answer reaches this a turn after it was made.
                None => returning(self.recorded_item_with(message.recipient_id, &marker)).await?,
            };
            match item {
                None => {
                    self.seams.ledger.borrow_mut().retry_delivery(
                        message.id,
                        "the daemon stopped before it arrived",
                        true,
                    )?;
                }
                Some(item) => {
                    let receipt = json!({ "item": item });
                    self.seams
                        .ledger
                        .borrow_mut()
                        .confirm_delivery(message.id, Some(&receipt))?;
                }
            }
        }
        Ok(())
    }

    /// The first item a participant's harness recorded it was given that
    /// holds `marker`, read with no window open: none when none does, or
    /// when its record cannot be read.
    async fn recorded_item_with(
        &self,
        participant: i64,
        marker: &str,
    ) -> Result<Option<String>, EngineError> {
        let conversation = self
            .seams
            .ledger
            .borrow()
            .current_conversation(participant)?;
        let Some(conversation) = conversation else {
            return Ok(None);
        };
        let Some(session) = conversation
            .native_session
            .filter(|session| !session.is_empty())
        else {
            return Ok(None);
        };
        let Some(harness) = Harness::from_kind(&conversation.harness)
            .filter(|_| self.seams.adapters.adapter(&conversation.harness).is_some())
        else {
            return Ok(None);
        };
        // The adapter's `record` is an `async` function, and `.catch(() =>
        // null)` made a promise of its own to wait on.
        let reading = caught(
            self.seams
                .records
                .look(harness, &session, &Options::default()),
        )
        .await;
        let Reading::Known(record) = &*reading else {
            return Ok(None);
        };
        Ok(record
            .items
            .iter()
            .find(|item| item.role == Role::User && item.text.contains(marker))
            .map(|item| item.id.to_string()))
    }
}
