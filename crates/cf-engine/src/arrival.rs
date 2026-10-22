//! Whether a message handed over to a window arrived: its harness's record is
//! watched for the message's paste, an item the message's header begins as the
//! harness records a paste, which confirms it; a header quoted in other words
//! proves nothing (`is_paste_of`). A paste the window may not have sent is given
//! one more Enter, and one the record never showed is pasted again, by this rule:
//!
//! - **Taken.** A harness that says its own queue has the message, at the hand-over
//!   (`Admission::Admitted`'s `queued`) or after it (`Window::took`: Devin's store
//!   has the paste's history row, written at its Enter), has it: the message is waited for until its record
//!   shows it or the window closes, and never pasted again.
//! - **Held.** While the harness says the paste is in a turn it is in still
//!   (`Window::held`: the turn it went into, or the one the harness began with it),
//!   it is waited for, however long the turn and however at rest the window reads.
//!   The harness is asked at the hand-over too: a paste that went into a turn is
//!   held from then, whenever the next look comes.
//! - **Free.** Otherwise the paste's time runs from when it was last found free:
//!   the hand-over, where the harness did not hold it then, else the first look that
//!   found it held no longer. It is pasted again once the record has had a whole
//!   arrival window of the window's rest since then (a look that finds the window in
//!   a turn starts that again: the turn may hold what it was given), or at the
//!   window's first rest past [`RECONCILE_MS`] since then, however turns cut its
//!   rest, so no turns hold the queue behind it for good.
//! - **Left.** A paste whose window the human switched to another conversation
//!   (`/new`, `/resume`, `/clear`) before its record showed it went into the one left
//!   (`Delivering::session`): that record, read with no window open, confirms it, or
//!   gives it back to its queue, with its attempt, once it has read at rest without it
//!   for a whole arrival window (a record that cannot be read is at rest, as a look
//!   reads one), or has read a turn at work, unchanged, for [`RECONCILE_MS`]: the
//!   switch cut it, unless its harness said it took the paste. The harness's word on
//!   its queue was that conversation's.
//!
//! So a paste the window did not drop goes once: a turn that holds it ends, Devin
//! runs it within seconds, and the first look after reads its prompt (taken), before
//! any arrival window of rest or half hour from the first free look can pass. And a
//! paste the window dropped goes again within half an hour of the first look after
//! the turn that held it, however the looks fall (`tests/dispatcher/schedules.rs`
//! explores both over generated schedules of looks, turns and sleeps). What a
//! chief's launch put ahead of its queue (the role it says first, and the message
//! that role displaced) is its first messages, as its handoff is on its command
//! line: waited for as those are, and never pasted again for being slow
//! (`Delivering::chief`).
//!
//! Limits. That a paste the window did not drop goes once rests on the arrival
//! window being longer than the seconds Devin takes to begin a paste it queued, once
//! the turn it waited behind ends (the schedules' model gives it 5 s, not measured):
//! a Devin that waited longer, or a shorter arrival window, could see it pasted again
//! before its prompt is read. A harness's word that its queue has the message is not
//! withdrawn: one that then loses it leaves the message waited for until the window
//! closes. A paste held in a turn that never ends waits with it, and the window's
//! queue behind it, until the human stops the turn or closes the window. The one Enter is pressed whatever the
//! window shows (a quiet dialog may take it), and the screen is looked at apart from
//! the press, so a human's typing in between can meet it. What the daemon's stop leaves
//! on its way is settled at the start by the record alone (`in_flight`): one it does not
//! show goes again, so a paste a window held queued when the daemon stopped, and ran
//! later, is read twice. Where Devin's store says its TUI took it, the paste that goes
//! again says in its header that it may be there already.
//!
//! What a hand-over is and how it fails are `deliveries`'; the wait is the part of
//! the record's delivery part this owns.

use std::rc::Rc;

use cf_base::js;
use cf_harness::contract::Observed;
use cf_harness::records::{Reading, Role, Settlement};
use cf_proto::trace::WindowEvent;
use serde_json::{json, Value};

use crate::deliveries::Delivering;
use crate::delivery_text::is_paste_of;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::{begin, returning};
use crate::seams::EngineError;
use crate::trust::Trusting;

/// An Enter, pressed once more for a paste its window did not send.
const ENTER: u8 = 13;
/// How long a paste may go unshown in the record before its window gets that Enter.
const ENTER_AGAIN_MS: i64 = 10_000;
/// How long the window must have printed nothing first: one being typed into,
/// or drawing, is left be.
const ENTER_AGAIN_QUIET_MS: f64 = 3_000.0;
/// How long after it was last found free (`Delivering::free_since`) a paste the
/// record never showed is pasted again at the window's next rest, however its rest
/// was cut by turns.
const RECONCILE_MS: i64 = 30 * 60_000;

/// What a look found of the window's rest since the message on its way was handed over.
#[derive(Debug, Default)]
struct Rest {
    /// How long the window has been at rest since the hand-over or since its last turn.
    rested: i64,
    /// A turn was found since the hand-over.
    turned: bool,
    /// Since when the paste has been free (`Delivering::free_since`): none while the
    /// harness holds it.
    free: Option<i64>,
}

impl Dispatcher {
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
        let Some(message) = message.filter(|found| found.state == "delivering") else {
            record.delivery.borrow_mut().delivering = None;
            return Ok(true);
        };
        // Only a launch's first message goes in behind the brief its window received.
        let first = if delivering.launch {
            self.brief_given_first(&message)?
        } else {
            None
        };
        let arrived = observed.items().iter().find(|item| {
            item.role == Role::User && is_paste_of(&item.text, message.id, first.as_deref())
        });
        let Some(arrived) = arrived else {
            return Ok(false);
        };
        // A paste the window's harness queued behind a turn that the app's Stop then
        // cancelled is in its record all the same, and delivered: Devin keeps it in the
        // conversation, unanswered and never run, and its agent reads it in the next
        // turn (T-102, 3 of 3 runs on Devin 3000.11.92). The Stop is a pause, and every
        // resume sends words, so the next turn comes for a task that goes on; giving it
        // back would put it in the conversation twice.
        let receipt = json!({ "item": &*arrived.id });
        self.confirm_delivered(&message, &receipt)?;
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
        let launching = record
            .delivery
            .borrow()
            .delivering
            .as_ref()
            .is_some_and(|delivering| delivering.launch);
        if self.confirm_arrival(record, observed)? {
            // A launch's first message went in with the window, so the turn
            // it began is told by the record: its header showing is the
            // harness's own start of it.
            let pane = record.window.borrow().pane.clone();
            if let Some(pane) = pane.filter(|_| launching) {
                self.mark_turn(record, &pane).await;
            }
            return Ok(());
        }
        let Some(delivering) = record.delivery.borrow().delivering.clone() else {
            return Ok(());
        };
        if delivering.launch {
            // A first message waits as long as its window asks whether to trust its
            // folder, and has its whole time once it is past that (`trust`).
            if record.window.borrow().trust != Trusting::Past {
                return Ok(());
            }
            let waited = self.now() - delivering.since;
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
        // The human switched the window away from the conversation it was pasted into:
        // that conversation's record says what came of it, the queued one's too.
        if let Some(left) = self.left(record, &delivering)? {
            return self.watch_left(record, delivering, &left).await;
        }
        if delivering.queued {
            return Ok(());
        }
        // The harness's own word, once it has one: its agent has the message.
        if self.took(record) {
            self.queued(record);
            return Ok(());
        }
        let now = self.now();
        let waited = now - delivering.since;
        let Rest {
            rested,
            turned,
            free,
        } = self.rested(record, observed, self.held(record));
        let at_rest = observed.settled && observed.waiting.is_none();
        let overdue = at_rest && free.is_some_and(|free| now - free > RECONCILE_MS);
        if rested <= self.seams.limits.arrival_ms && !overdue {
            if waited > ENTER_AGAIN_MS
                && waited <= self.seams.limits.arrival_ms
                && !delivering.entered_again
            {
                self.enter_again(record, delivering.message).await?;
            }
            return Ok(());
        }
        // What went into the chief's window as its first messages (the role its
        // launch said, the handoff after it) is waited for as a first message is.
        if delivering.chief {
            return Ok(());
        }
        record.delivery.borrow_mut().delivering = None;
        // A paste the harness record never showed after the whole window, the
        // window at rest all of it, did not land: sending it again is how it
        // reaches the reader. (A message the harness queued itself waits for
        // the record, or for the window to close.)
        let because = if overdue && rested <= self.seams.limits.arrival_ms {
            let since = if free == Some(delivering.since) {
                "since it was handed over"
            } else {
                "since the turn that held it ended"
            };
            format!(
                "the harness record never showed it in the {} min {since}",
                RECONCILE_MS / 60_000
            )
        } else if turned {
            format!(
                "the harness record never showed it in the {} s its window was at rest since its last turn",
                self.seams.limits.arrival_ms / 1000
            )
        } else {
            "the harness record never showed it".to_owned()
        };
        self.settle_failure(delivering, &because, true)
    }

    /// The conversation the message on its way was pasted into, where the window is on
    /// another now (the human ran `/new` or `/resume` in it, or `/clear`).
    fn left(
        &self,
        record: &Record,
        delivering: &Delivering,
    ) -> Result<Option<String>, EngineError> {
        let Some(pasted) = &delivering.session else {
            return Ok(None);
        };
        let now = self
            .seams
            .ledger
            .borrow()
            .current_conversation(record.id)?
            .and_then(|conversation| conversation.native_session);
        Ok(now.filter(|now| now != pasted).map(|_| pasted.clone()))
    }

    /// The message on its way went into the conversation `left`, which its window
    /// shows no longer: no look at the window reads its record again, and the harness's
    /// word of its own queue was of that conversation. Its record, read with no window
    /// open, confirms it where it shows it; one that has read at rest without it for a
    /// whole arrival window gives it back to its queue, with its attempt, for the
    /// conversation the window shows now (the reviews of 2026-10-18: a paste the
    /// harness had queued was waited for in the new conversation's record, which never
    /// shows it, and everything behind it waited for good). A record that cannot be
    /// read is at rest, with nothing in it, as a look at a window reads one (the review
    /// of c2e29f2d: Devin reads a session it holds no row of as unknown, and a paste into
    /// a conversation the human left before it ran waited for good). One that reads a
    /// turn at work is waited for, but no longer than [`RECONCILE_MS`] of reading the
    /// same: a turn the switch cut, which no end will close. Unless its harness said it
    /// took the paste (`queued`): Devin's `/new` cuts no turn, and runs the paste queued
    /// behind it in the conversation left once the turn ends, however long its step (the
    /// review of 6c8fc595), so its record is waited for as on the window's own path.
    async fn watch_left(
        &self,
        record: &Record,
        delivering: Delivering,
        left: &str,
    ) -> Result<(), EngineError> {
        let reading = self.record_left(record.id, left).await?;
        // The window may have gone, and another message gone on its way, while the
        // record was read: this one is settled no more here.
        let this = |record: &Record| {
            record
                .delivery
                .borrow()
                .delivering
                .as_ref()
                .is_some_and(|now| now.message == delivering.message)
        };
        if !this(record) {
            return Ok(());
        }
        let Some(message) = self.seams.ledger.borrow().message(delivering.message)? else {
            return Ok(());
        };
        if message.state != "delivering" {
            return Ok(());
        }
        let found = match reading.as_deref() {
            Some(Reading::Known(found)) => Some(found),
            _ => None,
        };
        let arrived = found.and_then(|found| {
            found
                .items
                .iter()
                .find(|item| item.role == Role::User && is_paste_of(&item.text, message.id, None))
        });
        if let Some(arrived) = arrived {
            self.confirm_delivered(&message, &json!({ "item": &*arrived.id }))?;
            record.delivery.borrow_mut().delivering = None;
            self.changed();
            return Ok(());
        }
        let at_rest = found.is_none_or(|found| {
            found.settlement == Settlement::Settled
                || (found.settlement != Settlement::InFlight
                    && found.items.is_empty()
                    && !found.in_flight)
        });
        let items = found.map_or(0, |found| found.items.len());
        let now = self.now();
        let (rested, at_work) = {
            let mut part = record.delivery.borrow_mut();
            let held = part.left_at_work;
            let Some(on_its_way) = part.delivering.as_mut() else {
                return Ok(());
            };
            if at_rest {
                let rested = now - *on_its_way.rested_since.get_or_insert(now);
                part.left_at_work = None;
                (rested, 0)
            } else {
                on_its_way.rested_since = None;
                let since = match held {
                    Some((of, seen, since)) if of == delivering.message && seen == items => since,
                    _ => now,
                };
                part.left_at_work = Some((delivering.message, items, since));
                (0, now - since)
            }
        };
        let because = if rested > self.seams.limits.arrival_ms {
            "its window was switched to another conversation, whose record never showed it"
        } else if at_work > RECONCILE_MS && !delivering.queued {
            "its window was switched to another conversation, whose record read a turn at work, unchanged, and never showed it"
        } else {
            return Ok(());
        };
        record.delivery.borrow_mut().delivering = None;
        self.give_back(delivering, because)?;
        self.changed();
        Ok(())
    }

    /// Whether the window's harness says its agent has the message on its way.
    fn took(&self, record: &Record) -> bool {
        self.window_of(record).0.is_some_and(|window| window.took())
    }

    /// The message on its way is the harness's own queue's from now on.
    fn queued(&self, record: &Record) {
        if let Some(delivering) = record.delivery.borrow_mut().delivering.as_mut() {
            delivering.queued = true;
        }
    }

    /// Whether the window's harness says the message on its way is in a turn it is
    /// in still.
    fn held(&self, record: &Record) -> bool {
        self.window_of(record).0.is_some_and(|window| window.held())
    }

    /// How long the window has been at rest since the message on its way was
    /// handed over, or since its last turn ended: the time its record has had
    /// to show it. A look that finds the window in a turn starts the count
    /// again, whatever the harness does with what came in meanwhile: it may
    /// hold it until it runs it, which is the record's wait, not its failure. A
    /// window waiting on a dialog is in its turn still. One whose harness `held` the
    /// message is in the turn it went into, and the paste is not free: its time
    /// starts again at the first look that finds it held no longer. And whether a
    /// turn was found since the hand-over.
    fn rested(&self, record: &Record, observed: &Observed, held: bool) -> Rest {
        let now = self.now();
        let mut part = record.delivery.borrow_mut();
        let Some(delivering) = part.delivering.as_mut() else {
            return Rest::default();
        };
        if held {
            delivering.rested_since = None;
            delivering.free_since = None;
            return Rest {
                rested: 0,
                turned: true,
                free: None,
            };
        }
        let free = Some(*delivering.free_since.get_or_insert(now));
        if observed.settled && observed.waiting.is_none() {
            let since = *delivering.rested_since.get_or_insert(now);
            Rest {
                rested: now - since,
                turned: since != delivering.since,
                free,
            }
        } else {
            delivering.rested_since = None;
            Rest {
                rested: 0,
                turned: true,
                free,
            }
        }
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
}
