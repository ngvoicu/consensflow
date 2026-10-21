//! What goes into a window and what comes back out: into a window, ready,
//! `beginDelivery`, the hand-over; its arrival is watched for by `arrival`, and
//! what it came to is settled here: tried again or failed, and at start, what
//! was in flight. Out of a window, a worker's answer becomes its task's result.
//! It owns the record's delivery part; the window's part it only reads.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cf_base::js;
use cf_harness::contract::{
    Admission, Held, HostError, Input, Observed, Pane, PaneHost, Readiness, Work,
};
use cf_harness::records::{Options, Reading, Role};
use cf_ledger::{HandOver, MessageView, NewNote, ParticipantView, ProjectView, TaskView};
use cf_proto::agents::Harness;
use cf_proto::trace::WindowEvent;
use serde_json::{json, Value};

use crate::delivery_text::{delivery_text, marker_of};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::role_change::{answers_to_the_task, Prelude};
use crate::runtime::{caught, returning};
use crate::seams::EngineError;

/// The one message on its way into a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Delivering {
    pub(crate) message: i64,
    /// Its header's start, which proves it arrived (`markerOf`).
    pub(crate) marker: String,
    /// When it was handed over, on the engine's clock.
    pub(crate) since: i64,
    /// Since when the window has been at rest without the record showing it:
    /// from the hand-over, but for the turns the window was in since. None
    /// while it is in one (`arrival`).
    pub(crate) rested_since: Option<i64>,
    /// It is a launch's first message.
    pub(crate) launch: bool,
    /// It is the chief's first message, a handoff most often, or what a launch put
    /// ahead of it (the replacement of the role): never given up for being slow.
    pub(crate) chief: bool,
    /// The harness's own queue took it.
    pub(crate) queued: bool,
    /// Enter was pressed once more for it.
    pub(crate) entered_again: bool,
    /// The rows its paste was begun with, which an Enter pressed once more for it
    /// sends with it (`arrival`). None for a launch's first message, which is
    /// never pressed for.
    pub(crate) carried: Vec<i64>,
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
    /// The pane whose input box may hold a paste whose Enter was withheld
    /// (`arrival`): nothing is pasted into it until the host has taken the keys
    /// that empty it ([`Dispatcher::clear_box`]). A window opened since has a box
    /// of its own, which holds nothing of it.
    pub(crate) uncleared: Option<Pane>,
}

/// What the Enter pressed once more for a paste does, as the ledger says now
/// ([`Dispatcher::enter_admitted`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Enter {
    /// It goes: the paste is admitted as it was at its hand-over.
    Go,
    /// No key goes, and the paste went back to its queue: what it held was
    /// withdrawn since, or its project closed. It stays in the input box.
    Withheld,
    /// No key goes: the window has the paste, and it is taken for arrived.
    Arrived,
}

/// The pane host as a window is handed it for a hand-over (a delivery, or the
/// first message a window posts once it is up): the host's own requests, and the
/// engine's word whether the window may be handed the text now
/// ([`PaneHost::admits`]). Every adapter asks it at the moment of its hand-over,
/// after every wait of its own (Claude's keys that clear its input box, the pane's
/// claim Pi's rename waits for, OpenCode's wait for its server, the connection a
/// send over loopback waits for): a Close that comes during any of them hands the
/// window nothing (both reviews of 2026-10-16, rounds 4 and 5). Of a message
/// begun, the ledger answers, in the write that marks it handed if it goes
/// ([`cf_ledger::Ledger::admit`]): it goes while its project is open and nothing
/// of its paste was withdrawn since it was begun (a continuation whose task moved
/// on, round 7), and from then on a reader of a closed project reads its text. An
/// Enter that sends what waits in an input box, a paste handed already, is asked
/// for as its hand-over was, and a row of it its window has since says it arrived
/// ([`Admitting::again`]).
pub(crate) struct Admitting<'a> {
    dispatcher: &'a Dispatcher,
    /// The window's project.
    project: i64,
    /// The message it hands over, and the rows its paste carries, where it hands
    /// over one begun.
    handing: Option<(i64, Vec<i64>)>,
    /// What it hands over is a paste handed over already, which an Enter sends.
    again: bool,
    /// The engine emptied the window's input box just before the hand-over.
    cleared: bool,
    /// A row of the paste its window has since, which says the window took it.
    received: Cell<Option<i64>>,
    /// It said no, or could not tell: what was to go in did not.
    refused: Cell<bool>,
    /// It said no because what was to go in was withdrawn since it was begun.
    withdrawn: Cell<bool>,
    /// Why it could not tell, where the ledger could not be read.
    failed: RefCell<Option<EngineError>>,
}

impl<'a> Admitting<'a> {
    /// The host for a window of `project`.
    pub(crate) fn new(dispatcher: &'a Dispatcher, project: i64) -> Self {
        Self {
            dispatcher,
            project,
            handing: None,
            again: false,
            cleared: false,
            received: Cell::new(None),
            refused: Cell::new(false),
            withdrawn: Cell::new(false),
            failed: RefCell::new(None),
        }
    }

    /// The host for the hand-over of `message`, begun with the rows `carried`.
    pub(crate) fn handing(mut self, message: i64, carried: Vec<i64>) -> Self {
        self.handing = Some((message, carried));
        self
    }

    /// The host for a paste handed over already, which an Enter sends: a row of it
    /// its window has since is no row withdrawn.
    pub(crate) fn again(mut self) -> Self {
        self.again = true;
        self
    }

    /// The host for a hand-over the engine emptied the input box for just before.
    pub(crate) fn cleared(mut self, cleared: bool) -> Self {
        self.cleared = cleared;
        self
    }

    /// Whether it turned a hand-over away: then nothing of it went in, and why it
    /// could not tell, if the ledger failed it.
    pub(crate) fn turned_away(&self) -> Option<Option<EngineError>> {
        self.refused.get().then(|| self.failed.borrow_mut().take())
    }

    /// Why what was to go in went back to its queue, where it turned it away.
    pub(crate) fn because(&self) -> &'static str {
        if self.withdrawn.get() {
            "what its paste held was withdrawn before it was handed over"
        } else {
            "its project was closed before it was handed over"
        }
    }

    /// Whether the window may be handed the text now, as the ledger says.
    fn may(&self) -> Result<bool, EngineError> {
        let Some((message, carried)) = &self.handing else {
            return self.dispatcher.in_open_project(self.project);
        };
        if self.again {
            let received = self
                .dispatcher
                .seams
                .ledger
                .borrow()
                .received_of(*message, carried)?;
            if received.is_some() {
                self.received.set(received);
                return Ok(false);
            }
        }
        let handed = self
            .dispatcher
            .seams
            .ledger
            .borrow_mut()
            .admit(*message, carried)?;
        self.withdrawn.set(handed == HandOver::Withdrawn);
        Ok(handed == HandOver::Admitted)
    }
}

impl PaneHost for Admitting<'_> {
    fn request<'b>(&'b self, op: &'b str, body: Value) -> Work<'b, Result<Value, HostError>> {
        self.dispatcher.seams.host.request(op, body)
    }

    fn cleared(&self) -> bool {
        self.cleared
    }

    fn admits(&self) -> bool {
        match self.may() {
            Ok(true) => return true,
            Ok(false) => {}
            Err(cause) => *self.failed.borrow_mut() = Some(cause),
        }
        self.refused.set(true);
        false
    }
}

impl Dispatcher {
    /// Whether `project` is open now, as a delivery asks after each wait: a
    /// closed project's window is handed nothing.
    pub(crate) fn in_open_project(&self, project: i64) -> Result<bool, EngineError> {
        Ok(self
            .project_now(project)?
            .is_some_and(|project| project.state == "open"))
    }

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
        // A project closed while the window got ready gives it nothing: the message
        // waits, queued, for the Resume (both reviews of 2026-10-16, round 3).
        if !self.in_open_project(message.project_id)? {
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
        // A paste whose Enter was withheld waits in the input box, where this one
        // would go in beside it, to be sent with it: the box is emptied first, and
        // nothing goes in until the host has taken the keys (both reviews of
        // 2026-10-20, round 10). What was asked before is asked again after it.
        let cleared = match self.clear_box(record, &pane).await {
            None => false,
            Some(true) => true,
            Some(false) => {
                let first = record.delivery.borrow().held != Some(message.id);
                if first {
                    record.delivery.borrow_mut().held = Some(message.id);
                    self.trace_window(
                        record,
                        WindowEvent::DeliveryHeld {
                            message: message.id,
                            reason: "its input box may hold a paste whose Enter was withheld, and the keys that empty it were not taken".to_owned(),
                        },
                    )?;
                }
                return Ok(());
            }
        };
        if cleared
            && (self.forgotten(record)
                || !self.in_open_project(message.project_id)?
                || self
                    .next_in_line(record)?
                    .is_none_or(|next| next.id != message.id))
        {
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
        // The adapter asks the ledger at its hand-over whether the project is still
        // open and nothing of the paste was withdrawn since it was begun.
        let carried: Vec<i64> = begun.carried.iter().map(|row| row.id).collect();
        let admitting = Admitting::new(self, message.project_id)
            .handing(message.id, carried.clone())
            .cleared(cleared);
        let outcome = match returning(window.deliver(&admitting, &pane, &text)).await {
            Ok(admission) => admission,
            // An adapter that failed never handed it over, and its error must show.
            Err(error) => Admission::Refused {
                reason: format!("the delivery failed: {error}"),
            },
        };
        if self.forgotten(record) {
            return Ok(());
        }
        // Turned away before the hand-over: nothing went in, and the message waits
        // for the Resume, or goes again without what was withdrawn of its paste. One
        // withdrawn itself stays so.
        if let Some(failed) = admitting.turned_away() {
            self.give_back(message.id, admitting.because())?;
            return failed.map_or(Ok(()), Err);
        }
        let since = self.now();
        let delivering = Delivering {
            message: message.id,
            marker: marker_of(message.id),
            since,
            // The window was at rest when it was handed over.
            rested_since: Some(since),
            launch: false,
            // What a chief's launch put ahead of its queue is its first messages.
            chief: self.begins(record, message.id),
            // The harness's own queue took it (a peer inbox, a broker, a
            // plugin): it shows when the harness gets to it, and sending it
            // again would only make a duplicate.
            queued: matches!(outcome, Admission::Admitted { queued: true }),
            entered_again: false,
            carried,
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

    /// A worker's answer to its task's latest message finishes the task: the task
    /// as it is once its result is recorded, where it was. Whatever the turn left
    /// running is no part of it ([`Dispatcher::end_turn_work`]). An answer to the
    /// note that says the worker its role is none of the task's
    /// ([`answers_to_the_task`]): where nothing else answers, there is no result,
    /// and nothing the turn left running is ended.
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

    /// What one more Enter for `message`, the paste on its way into `record`'s
    /// window, does after the look at its screen: the Enter sends it, so it goes
    /// only as the paste's hand-over went, as the ledger admits it now, with the
    /// rows the paste was begun with (`cf_ledger`'s `admit`). Its project closed
    /// since, or what it held withdrawn since (a continuation whose task was
    /// paused or cancelled while the screen was looked at, or a row of its paste),
    /// no key goes: it goes back to its queue, to go again without what was
    /// withdrawn, or once its project is open, as a paste turned away at its
    /// hand-over does (both reviews of 2026-10-17, round 9). The paste stays in the
    /// input box, which the next paste empties first (`arrival`). A row of the
    /// paste its window has since (read, or delivered) says the window took it: no
    /// key goes, and it is confirmed with that row for its receipt (Sol's review of
    /// 2026-10-20, round 10: the words were taken for withdrawn and went twice).
    pub(crate) fn enter_admitted(
        &self,
        record: &Record,
        message: i64,
    ) -> Result<Enter, EngineError> {
        let carried = record
            .delivery
            .borrow()
            .delivering
            .as_ref()
            .filter(|delivering| delivering.message == message)
            .map(|delivering| delivering.carried.clone())
            .unwrap_or_default();
        let found = self.seams.ledger.borrow().message(message)?;
        let project = found.as_ref().map_or(0, |found| found.project_id);
        let admitting = Admitting::new(self, project)
            .handing(message, carried)
            .again();
        if admitting.admits() {
            return Ok(Enter::Go);
        }
        if let Some(row) = admitting.received.get() {
            if let Some(found) = found.filter(|found| found.state == "delivering") {
                self.confirm_delivered(&found, &json!({ "read": row }))?;
            }
            record.delivery.borrow_mut().delivering = None;
            self.changed();
            return Ok(Enter::Arrived);
        }
        if let Some(Some(failed)) = admitting.turned_away() {
            return Err(failed);
        }
        let because = if admitting.withdrawn.get() {
            admitting.because()
        } else {
            "its project was closed before its Enter was pressed"
        };
        self.give_back(message, because)?;
        Ok(Enter::Withheld)
    }

    /// Empties `pane`'s input box of a paste whose Enter was withheld
    /// ([`DeliveryPart::uncleared`]), with the keys of its harness
    /// ([`Input::Pasted`]): whether the host took them, which is when the box
    /// counts as empty. None where nothing waits there.
    pub(crate) async fn clear_box(&self, record: &Record, pane: &Pane) -> Option<bool> {
        if record.delivery.borrow().uncleared.as_ref() != Some(pane) {
            return None;
        }
        let Input::Pasted { clear } = record.window.borrow().input else {
            return None;
        };
        let body = json!({ "id": pane.id, "generation": pane.generation, "bytes": clear });
        let answer = returning(self.seams.host.request("pane.input", body)).await;
        let taken = answer.is_ok_and(|answer| answer.get("ok") == Some(&Value::Bool(true)));
        let mut part = record.delivery.borrow_mut();
        if taken && part.uncleared.as_ref() == Some(pane) {
            part.uncleared = None;
        }
        Some(taken)
    }

    /// What a window was to receive goes back to its queue with its attempt:
    /// the window went before it had the chance to land, or was handed nothing of
    /// it, its project closed first, and it waits for the Resume. One withdrawn
    /// meanwhile stays so; a pause note whose task was cancelled on its way says
    /// its warning again (the ledger's `retry_delivery`; both reviews of
    /// 2026-10-16, rounds 4 and 5).
    pub(crate) fn give_back(&self, message: i64, because: &str) -> Result<(), EngineError> {
        let found = self.seams.ledger.borrow().message(message)?;
        if found.is_none_or(|found| found.state != "delivering") {
            return Ok(());
        }
        self.seams
            .ledger
            .borrow_mut()
            .retry_delivery(message, because, true)?;
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
        let given_up = self
            .seams
            .ledger
            .borrow_mut()
            .fail_delivery(message.id, because)?;
        // A pause note whose task was cancelled on its way was withdrawn instead,
        // and its warning is said again: nothing failed.
        if given_up.state != "failed" {
            return Ok(());
        }
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
                    self.confirm_delivered(&message, &json!({ "item": item }))?;
                }
            }
        }
        Ok(())
    }

    /// The first item a participant's harness recorded it was given that
    /// holds `marker`, read with no window open: none when none does, or
    /// when its record cannot be read.
    pub(crate) async fn recorded_item_with(
        &self,
        participant: i64,
        marker: &str,
    ) -> Result<Option<String>, EngineError> {
        let Some((harness, session)) = self.readable(participant)? else {
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

    /// Whether a participant's harness's record of its current conversation,
    /// read with no window open, ends in a turn with no end (the one the app's
    /// quit cut, or a Close), and none where it cannot tell: the record cannot,
    /// or there is none to read.
    pub(crate) async fn turn_cut(&self, participant: i64) -> Result<Option<bool>, EngineError> {
        let Some((harness, session)) = self.readable(participant)? else {
            return Ok(None);
        };
        Ok(self.seams.records.turn_cut(harness, &session).await)
    }

    /// The harness and the session a participant's current conversation is on,
    /// where its record can be read with no window open: none when it has no
    /// conversation, no session yet, or a harness no adapter opens.
    fn readable(&self, participant: i64) -> Result<Option<(Harness, String)>, EngineError> {
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
        Ok(Harness::from_kind(&conversation.harness)
            .filter(|_| self.seams.adapters.adapter(&conversation.harness).is_some())
            .map(|harness| (harness, session)))
    }
}
