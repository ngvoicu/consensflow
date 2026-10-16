//! Whether a message handed over to a window arrived: its harness's record is
//! watched for the message's paste, an item the message's header begins as the
//! harness records a paste, which confirms it; a header quoted in other words
//! proves nothing (`is_paste_of`). A paste the window may
//! not have sent is given one more Enter, and one the record never showed is
//! pasted again. Two things keep a record that is late from being taken for
//! lacking, and the paste from being a second copy of one the window has (a
//! Devin chief, 2026-10-13: each of two notes twice). A harness may say at the
//! hand-over that its own queue holds the message (`Admission::Admitted`'s
//! `queued`), and it is then waited for as one the harness queued itself is. And a
//! window in a turn may hold what it is given and write it to its record when it
//! gets to it, so only the window's rest counts towards the record's time: a record
//! gets a whole arrival window of rest, from the hand-over or from the end of the
//! last turn the window went into since. A window whose harness says the paste is in
//! a turn it is in still (`Window::held`: one it went into, or the one the harness
//! began with it) is in that turn, however at rest it reads (a Devin turn whose
//! agent says nothing for minutes: a result of 2026-10-15 was pasted twice into
//! one), and the paste is never pasted again while it is, however long the turn:
//! the human can stop it. That word lasts the turn it is for, and no longer (the
//! reviews of 2026-10-16: Devin's word that its queue had the message, which the
//! human's words quoting a header Devin had dropped gave, kept it waited for for
//! good). However often the window goes into other turns, a paste the record has not shown
//! within [`RECONCILE_MS`] of its hand-over is pasted again at the window's next
//! rest, so unrelated turns never hold the queue behind it for good. What a chief's
//! launch put ahead of its queue (the role it says first, and the message that
//! role displaced) is its first messages, as its handoff is on its command line:
//! waited for as those are, and never pasted again for being slow
//! (`Delivering::chief`).
//!
//! Limits (Astraeus, 2026-10-14). A harness's word at the hand-over that its queue
//! holds the message (OpenCode's, Pi's) is not withdrawn: one that then refuses it
//! leaves the message waited for until the window closes. A paste held in a turn
//! that never ends waits with it, and the window's queue behind it, until the human
//! stops the turn or closes the window (the reviews of 2026-10-16: pasted again past
//! half an hour into the turn that held it, both ran when it ended). The one Enter is
//! pressed whatever the window shows, as it was before (a quiet dialog may take
//! it), and the screen is looked at apart from the press, so a human's typing in
//! between can meet it.
//!
//! What a hand-over is and how it fails are `deliveries`'; the wait is the part of
//! the record's delivery part this owns.

use std::rc::Rc;

use cf_base::js;
use cf_harness::contract::Observed;
use cf_harness::records::Role;
use cf_proto::trace::WindowEvent;
use serde_json::{json, Value};

use crate::delivery_text::is_paste_of;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::{begin, returning};
use crate::seams::EngineError;

/// An Enter, pressed once more for a paste its window did not send.
const ENTER: u8 = 13;
/// How long a paste may go unshown in the record before its window gets that Enter.
const ENTER_AGAIN_MS: i64 = 10_000;
/// How long the window must have printed nothing first: one being typed into,
/// or drawing, is left be.
const ENTER_AGAIN_QUIET_MS: f64 = 3_000.0;
/// How long after its hand-over a paste the record never showed is pasted again
/// at the window's next rest, however its rest was cut by turns: by turns other
/// than one its harness held it in (`Delivering::held`).
const RECONCILE_MS: i64 = 30 * 60_000;

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
        if delivering.queued {
            return Ok(());
        }
        let waited = self.now() - delivering.since;
        let held = self.held(record);
        if held {
            self.held_in_a_turn(record);
        }
        let (rested, turned) = self.rested(record, observed, held);
        let at_rest = observed.settled && observed.waiting.is_none();
        // A paste its harness holds in a turn goes again never while it does, at any
        // age, and the half hour's shortcut is none once it did: the turn's end gives
        // its record a whole arrival window (the reviews of 2026-10-16: past the half
        // hour, a second copy went into the turn that held the first).
        let overdue = waited > RECONCILE_MS && at_rest && !held && !delivering.held;
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
            format!(
                "the harness record never showed it in the {} min since it was handed over",
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

    /// Whether the window's harness says the message on its way is in a turn it is
    /// in still.
    fn held(&self, record: &Record) -> bool {
        self.window_of(record).0.is_some_and(|window| window.held())
    }

    /// The message on its way went into a turn its harness held it in.
    fn held_in_a_turn(&self, record: &Record) {
        if let Some(delivering) = record.delivery.borrow_mut().delivering.as_mut() {
            delivering.held = true;
        }
    }

    /// How long the window has been at rest since the message on its way was
    /// handed over, or since its last turn ended: the time its record has had
    /// to show it. A look that finds the window in a turn starts the count
    /// again, whatever the harness does with what came in meanwhile: it may
    /// hold it until it runs it, which is the record's wait, not its failure. A
    /// window waiting on a dialog is in its turn still, and so is one whose harness
    /// `held` the message in the turn it went into. And whether a turn was found
    /// since the hand-over.
    fn rested(&self, record: &Record, observed: &Observed, held: bool) -> (i64, bool) {
        let now = self.now();
        let mut part = record.delivery.borrow_mut();
        let Some(delivering) = part.delivering.as_mut() else {
            return (0, false);
        };
        if observed.settled && observed.waiting.is_none() && !held {
            let since = *delivering.rested_since.get_or_insert(now);
            (now - since, since != delivering.since)
        } else {
            delivering.rested_since = None;
            (0, true)
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
