//! What goes into a window and what comes back out: into a window, ready,
//! `beginDelivery`, the hand-over; its arrival is watched for by `arrival`, and
//! what it came to is settled here: tried again or failed. What was on its way
//! when the daemon or the window went is `in_flight`'s; what a worker's answer
//! comes to, out of a window, is `turn_end`'s. It owns the record's delivery
//! part; the window's part it only reads.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cf_base::js;
use cf_harness::contract::{Admission, Held, HostError, Input, Pane, PaneHost, Readiness, Work};
use cf_ledger::{is_told, GivenUp, HandOver, MessageView, NewNote, ProjectView, TaskView};
use cf_proto::trace::WindowEvent;
use serde_json::{json, Value};

use crate::arrival::ENTER_AGAIN_QUIET_MS;
use crate::delivery_text::{delivery_text, is_paste_of, marker_of};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::role_change::Prelude;
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
    /// A look found the window in a turn since the hand-over, or its harness holding
    /// it (`arrival`): the rest counted is since then.
    pub(crate) turned: bool,
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
    /// The rows its paste was begun with, which an Enter pressed once more for it
    /// sends with it (`arrival`). None for a launch's first message, which is
    /// never pressed for.
    pub(crate) carried: Vec<i64>,
}

/// The reads of a window that came to nothing while a message was on its way
/// (`arrival`'s `unknown`), as its log line counts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unread {
    pub(crate) message: i64,
    /// When the first came to nothing.
    pub(crate) first: i64,
    pub(crate) reads: u32,
    /// When the log last said so.
    pub(crate) told: Option<i64>,
}

/// The record's delivery part.
#[derive(Debug, Default)]
pub(crate) struct DeliveryPart {
    pub(crate) delivering: Option<Delivering>,
    /// The reads that came to nothing for the message on its way, or the last.
    pub(crate) unread: Option<Unread>,
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
    /// A paste whose Enter was withheld (`arrival`), which the window's input box
    /// may hold: nothing is pasted into that window until the host has taken the
    /// keys that empty it ([`Dispatcher::clear_box`]), unless the window's record
    /// shows the human sent it. A window opened since has a box of its own, which
    /// holds nothing of it.
    pub(crate) uncleared: Option<Uncleared>,
    /// The pane whose input box the engine emptied, and nothing went in since:
    /// each paste is told so until one goes in ([`PaneHost::cleared`]), and its
    /// adapter presses nothing more to empty it.
    pub(crate) emptied: Option<Pane>,
}

/// A paste whose Enter was withheld, left in its window's input box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Uncleared {
    pub(crate) pane: Pane,
    /// The message it was the paste of.
    pub(crate) message: i64,
    /// The rows it was begun with.
    pub(crate) carried: Vec<i64>,
    /// When its Enter was withheld, on the ledger's clock ([`cf_ledger::Ledger::now`]):
    /// only what the window's record was given from then on can be its paste, sent
    /// by the human.
    pub(crate) since: String,
    /// Since when, on the engine's clock, the window has not been quiet for the key
    /// that empties its box: none while it was, at the last ask.
    pub(crate) unquiet_since: Option<i64>,
    /// When a look last found it not quiet: a gap longer than the arrival window
    /// since (a hold for what the human typed, a closed project, another message
    /// first, a turn) starts the count again (Calliope's review of 2026-10-21,
    /// round 13).
    pub(crate) unquiet_last: Option<i64>,
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
        // Nor does another task's message go into a window that has a task's work
        // (`ownership`): the step closes it first.
        let participant = self.participant_of(record)?;
        if let Some(participant) = &participant {
            if self.barred(participant, record, &message).is_some() {
                return Ok(());
            }
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
        // Held for what the human typed and has not sent: the board says so. A box
        // the engine emptied holds the human's words now, which an interrupt of ours
        // may put back there: it is no longer known empty (Calliope's review of
        // 2026-10-21, round 12).
        record.delivery.borrow_mut().unsent = held == Some(Held::Unsent);
        if held == Some(Held::Unsent) {
            record.delivery.borrow_mut().emptied = None;
        }
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
        // would go in beside it, to be sent with it (both reviews of 2026-10-20,
        // round 10).
        let stale = record
            .delivery
            .borrow()
            .uncleared
            .clone()
            .filter(|stale| stale.pane == pane);
        if let Some(stale) = &stale {
            // The human may have sent it from there: the window's record shows it
            // given since its Enter was withheld, and it arrived. Nothing of it is
            // pasted again, and nothing is pressed into a box emptied so (Calliope's
            // review of 2026-10-21, round 11). It is confirmed in one write, and the
            // mark goes only once the ledger has it: a write that failed leaves it,
            // for the next pass to ask again (the same, round 12).
            // An item the paste begins, as only a paste proves it arrived
            // (`is_paste_of`): a header quoted in other words proves nothing.
            let copied = self
                .seams
                .ledger
                .borrow()
                .copied_items_since(record.id, &marker_of(stale.message), &stale.since)?
                .into_iter()
                .find(|(_, text)| is_paste_of(text, stale.message, None))
                .map(|(item, _)| item);
            if let Some(item) = copied {
                let receipt = json!({ "item": item });
                let found = self.seams.ledger.borrow().message(stale.message)?;
                let tells = match &found {
                    Some(found) => self.tells_a_role(found)?,
                    None => None,
                };
                let confirmed = self.seams.ledger.borrow_mut().confirm_sent_by_hand(
                    stale.message,
                    &stale.carried,
                    &receipt,
                    tells == Some(true),
                )?;
                record.delivery.borrow_mut().uncleared = None;
                if let Some(confirmed) = confirmed.filter(|_| tells == Some(false)) {
                    self.told_in_part(&confirmed)?;
                }
                self.changed();
                return Ok(());
            }
            // The key that empties it goes only into a window that printed nothing
            // for a while and holds nothing the human typed since it was read ready:
            // a second Control-C in a row ends Claude Code (the same review, round 11).
            let quiet = self.may_press(&pane).await;
            if self.forgotten(record)
                || !self.in_open_project(message.project_id)?
                || self
                    .next_in_line(record)?
                    .is_none_or(|next| next.id != message.id)
            {
                return Ok(());
            }
            let now = self.now();
            let unquiet_since = {
                let mut part = record.delivery.borrow_mut();
                let Some(stale) = part.uncleared.as_mut().filter(|stale| stale.pane == pane) else {
                    return Ok(());
                };
                if quiet {
                    stale.unquiet_since = None;
                    stale.unquiet_last = None;
                    None
                } else {
                    let arrival_ms = self.seams.limits.arrival_ms;
                    if stale
                        .unquiet_last
                        .is_some_and(|last| now - last > arrival_ms)
                    {
                        stale.unquiet_since = None;
                    }
                    stale.unquiet_last = Some(now);
                    Some(*stale.unquiet_since.get_or_insert(now))
                }
            };
            if let Some(since) = unquiet_since {
                // A window that never goes quiet would hold its queue for good: once
                // the wait outlasts the arrival window, the delivery is begun and this
                // attempt fails, as a key the host did not take does, and the last
                // tells the human (the same review, round 12).
                if now - since <= self.seams.limits.arrival_ms {
                    return self.held_once(
                        record,
                        message.id,
                        "its input box may hold a paste whose Enter was withheld, and the window is not quiet, or holds what you typed, for the key that empties it",
                    );
                }
                if let Some(stale) = record.delivery.borrow_mut().uncleared.as_mut() {
                    stale.unquiet_since = None;
                    stale.unquiet_last = None;
                }
                let begun = self.seams.ledger.borrow_mut().begin_delivery(message.id)?;
                let carried = begun.carried.iter().map(|row| row.id).collect();
                let delivering = self.delivering(record, message.id, carried);
                return self.settle_failure(
                    delivering,
                    "its window never went quiet for the key that empties its input box of a paste whose Enter was withheld",
                    true,
                );
            }
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
        // A member's window that held no task's work holds this one's from now on.
        let member = participant.is_some_and(|participant| participant.role != "chief");
        if let Some(number) = begun.message.task_number.filter(|_| member) {
            record.window.borrow_mut().owner.get_or_insert(number);
        }
        let text = delivery_text(&begun.message, &begun.carried);
        let carried: Vec<i64> = begun.carried.iter().map(|row| row.id).collect();
        // The box is emptied once the delivery is begun, so that a paste follows
        // the key; a key the host did not take spends the attempt, as a refused
        // paste does, and the last fails the delivery (the same review).
        if stale.as_ref().is_some_and(|stale| stale.pane == pane)
            && !self.clear_box(record, &pane).await
        {
            let delivering = self.delivering(record, message.id, carried);
            return self.settle_failure(
                delivering,
                "its window's input box could not be emptied of a paste whose Enter was withheld",
                true,
            );
        }
        // A turn begins with this message: what the window's tree runs now came before it,
        // and so did the rest the window was seen at, as every monitor of the turn will.
        self.mark_turn(record, &pane).await;
        record.held.borrow_mut().rests.pasted();
        // The adapter asks the ledger at its hand-over whether the project is still
        // open and nothing of the paste was withdrawn since it was begun.
        let cleared = record.delivery.borrow().emptied.as_ref() == Some(&pane);
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
        // Something went into the box, or may have: it is no longer known empty.
        if !matches!(outcome, Admission::Refused { .. }) {
            record.delivery.borrow_mut().emptied = None;
        }
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
        let mut delivering = self.delivering(record, message.id, carried);
        let since = delivering.since;
        // The window was at rest when it was handed over, unless its harness holds what
        // it was given in the turn it is in (`arrival`).
        let free = (!window.held()).then_some(since);
        delivering.rested_since = free;
        delivering.free_since = free;
        delivering.turned = free.is_none();
        // The harness's own queue took it (a peer inbox, a broker, a plugin): it
        // shows when the harness gets to it, and sending it again would only make
        // a duplicate.
        delivering.queued = matches!(outcome, Admission::Admitted { queued: true });
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

    /// `message`, begun with the rows `carried`, as it goes into `record`'s window
    /// now: handed over at this moment, the window at rest.
    fn delivering(&self, record: &Record, message: i64, carried: Vec<i64>) -> Delivering {
        let since = self.now();
        Delivering {
            message,
            since,
            rested_since: Some(since),
            free_since: Some(since),
            turned: false,
            launch: false,
            // What a chief's launch put ahead of its queue is its first messages.
            chief: self.begins(record, message),
            queued: false,
            entered_again: false,
            carried,
        }
    }

    /// A delivery waits this pass, `reason` said in the trace once per message.
    fn held_once(&self, record: &Record, message: i64, reason: &str) -> Result<(), EngineError> {
        if record.delivery.borrow().held == Some(message) {
            return Ok(());
        }
        record.delivery.borrow_mut().held = Some(message);
        self.trace_window(
            record,
            WindowEvent::DeliveryHeld {
                message,
                reason: reason.to_owned(),
            },
        )
    }

    /// Whether a key may be pressed into `pane` now: its screen says it printed
    /// nothing for [`ENTER_AGAIN_QUIET_MS`] and holds nothing the human typed.
    async fn may_press(&self, pane: &Pane) -> bool {
        let body = json!({ "id": pane.id, "generation": pane.generation });
        let snapshot = returning(self.seams.host.request("pane.snapshot", body))
            .await
            .ok();
        snapshot.is_some_and(|snapshot| {
            snapshot.get("ok") == Some(&Value::Bool(true))
                && !js::truthy(snapshot.get("unsent"))
                && snapshot
                    .get("outputQuietMs")
                    .and_then(Value::as_f64)
                    .is_some_and(|quiet| quiet >= ENTER_AGAIN_QUIET_MS)
        })
    }

    /// Empties `pane`'s input box of a paste whose Enter was withheld
    /// ([`DeliveryPart::uncleared`]), with the keys of its harness
    /// ([`Input::Pasted`]): whether the host took them, which is when the box
    /// counts as empty, and stays so until something goes in
    /// ([`DeliveryPart::emptied`]).
    pub(crate) async fn clear_box(&self, record: &Record, pane: &Pane) -> bool {
        let Input::Pasted { clear } = record.window.borrow().input else {
            return true;
        };
        let body = json!({ "id": pane.id, "generation": pane.generation, "bytes": clear });
        let answer = returning(self.seams.host.request("pane.input", body)).await;
        let taken = answer.is_ok_and(|answer| answer.get("ok") == Some(&Value::Bool(true)));
        if taken {
            let mut part = record.delivery.borrow_mut();
            if part
                .uncleared
                .as_ref()
                .is_some_and(|stale| &stale.pane == pane)
            {
                part.uncleared = None;
            }
            part.emptied = Some(pane.clone());
        }
        taken
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
    /// whoever gave it hears why; so do words told to the window while its
    /// task waits on it (`is_told`): an answer the window gave without them is
    /// no result, and whoever told them hears too (T-437). Whoever waits on any other kind would wait forever, so the
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
                // Whoever told the words that failed hears as the requester does:
                // the human hears below, unless the task was theirs.
                if let Some(teller) = message.sender.as_deref().filter(|teller| {
                    message.kind != "task" && *teller != thread.task.requester && *teller != "human"
                }) {
                    self.tell_failed(&project, &thread.task, teller, &because)?;
                }
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

    pub(crate) fn fail_task(
        &self,
        project: &ProjectView,
        task: &TaskView,
        because: &str,
    ) -> Result<(), EngineError> {
        // The ledger tells the requester, with the words that went (T-437).
        self.seams
            .ledger
            .borrow_mut()
            .give_up_task(project.id, task.number, because)?;
        Ok(())
    }

    fn tell_requester(
        &self,
        project: &ProjectView,
        task: &TaskView,
        because: &str,
    ) -> Result<(), EngineError> {
        self.tell_failed(project, task, &task.requester, because)
    }

    /// `to` hears that `task` failed, `because`, and how to reopen it.
    fn tell_failed(
        &self,
        project: &ProjectView,
        task: &TaskView,
        to: &str,
        because: &str,
    ) -> Result<(), EngineError> {
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: to.to_owned(),
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
