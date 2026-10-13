//! Whether a message handed over to a window arrived: its harness's record is
//! watched for the message's header, which confirms it. A paste the window may
//! not have sent is given one more Enter, and one the record never showed is
//! pasted again, unless its window went into a turn since: a window in a turn
//! took what it was given into it, or holds it (a harness may keep what comes in
//! meanwhile and write it to its record when it gets to it), so its record is
//! late, not lacking, and a message pasted again would be a second copy of one
//! the window has (a Devin chief, 2026-10-13: each of two notes twice). Such a
//! message is waited for as one the harness queued itself is. What a hand-over
//! is and how it fails are `deliveries`'; the wait is the part of the record's
//! delivery part this owns.

use std::rc::Rc;

use cf_base::js;
use cf_harness::contract::Observed;
use cf_harness::records::Role;
use cf_proto::trace::WindowEvent;
use serde_json::{json, Value};

use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::runtime::{begin, returning};
use crate::seams::EngineError;

/// An Enter, pressed once more for a paste its window did not send.
const ENTER: u8 = 13;
/// How long a paste may go unshown in the record before its window, if it is at
/// rest, gets that Enter.
const ENTER_AGAIN_MS: i64 = 10_000;
/// How long the window must have printed nothing first: one being typed into,
/// or drawing, is left be.
const ENTER_AGAIN_QUIET_MS: f64 = 3_000.0;

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
        // A window in a turn took the message into it, or holds it, whatever its
        // record shows; one waiting on a dialog of its own is in its turn still.
        if !(observed.settled && observed.waiting.is_none()) {
            self.took_a_turn(record);
            return Ok(());
        }
        let waited = self.now() - delivering.since;
        if delivering.turned || waited <= self.seams.limits.arrival_ms {
            if waited > ENTER_AGAIN_MS && !delivering.entered_again {
                self.enter_again(record, delivering.message).await?;
            }
            return Ok(());
        }
        record.delivery.borrow_mut().delivering = None;
        // A paste the harness record never showed after the whole window, its
        // window at rest all of it, did not land: sending it again is how it
        // reaches the reader. (A message the harness queued itself waits for
        // the record, or for the window to close; so does one the window went
        // into a turn after.)
        self.settle_failure(delivering, "the harness record never showed it", true)
    }

    /// A look found the window in a turn: the message on its way is waited for
    /// from now on, however long its record is, and an Enter pressed for it
    /// before (which a turn may have ignored) is spent no longer.
    fn took_a_turn(&self, record: &Record) {
        if let Some(delivering) = record.delivery.borrow_mut().delivering.as_mut() {
            delivering.turned = true;
            delivering.entered_again = false;
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
