//! Whether a message handed over to a window arrived: its harness's record is
//! watched for the message's paste, an item the message's header begins as the
//! harness records a paste, which confirms it; a header quoted in other words
//! proves nothing (`is_paste_of`). A paste the window may not have sent is given
//! one more Enter (a paste only: what a harness takes over a channel of its own
//! gets none, its record alone saying whether it arrived; an Enter withheld leaves
//! its paste in the input box, which the next paste empties first, `deliveries`),
//! and one the record never showed is pasted again, by this rule:
//!
//! - **Taken.** A harness that says its own queue has the message, at the hand-over
//!   (`Admission::Admitted`'s `queued`) or after it (`Window::took`: Devin's TUI put
//!   the paste to its agent), has it: the message is waited for until its record
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
//!
//! - **Unknown.** A read of the window that fails, or answers too late to say what
//!   the window does now ([`SLOW_READ_MS`]), is no look: what became of the paste
//!   since the last look that read the window is not known. Its rest begins again at
//!   the next look that reads the window at rest, and nothing is pressed or pasted
//!   again on it: a look whose record could not be read, or a snapshot for the Enter
//!   that failed or came late, or a host that says it is writing a paste still. Each
//!   is said in the daemon's log, at most once a minute for a message
//!   ([`UNREAD_TOLD_MS`]). Before a
//!   paste goes again, the window is read once more: the pane host must answer at
//!   once that nothing is going in, and that it wrote no paste into the window for a
//!   whole arrival window (one it held while it was slow is written when it catches
//!   up, and the record has had only that long to show it: its rest counts from
//!   then); and the record read after it must still not show it (one that does
//!   confirms it). Turn-end's rule for processes is the same
//!   (unreadable is unknown, and holds).
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
//! queue behind it, until the human stops the turn or closes the window. A turn that
//! begins between the window's readiness and the paste is not known to hold it until
//! a look reads its prompt; were it to end before the next look, and that look come a
//! whole arrival window after the hand-over and within seconds of its end, the paste
//! would go again while Devin had it queued. The one Enter is pressed whatever the
//! window shows (a quiet dialog may take it), and the screen is looked at apart from
//! the press, so a human's typing in between can meet it. A paste whose window can
//! no longer be read waits with no bound: while every look fails or its pane host
//! does not answer in time, no Enter is pressed, it is never pasted again, and no
//! attempt is counted. What ends that is a look that reads the window again (then
//! the arrival window of rest, or the half hour, as ever), the window's exit, the
//! message leaving its way (its task cancelled, the window taken back), or the
//! daemon's restart, which gives a message whose record cannot be read back to its
//! queue (`in_flight`).
//!
//! What a hand-over is and how it fails are `deliveries`'; the wait is the part of
//! the record's delivery part this owns.

use std::rc::Rc;

use cf_base::js;
use cf_harness::contract::{Input, Observed, Pane};
use cf_harness::records::{Reading, Role};
use cf_proto::trace::WindowEvent;
use serde_json::{json, Value};

use crate::deliveries::{Enter, Uncleared, Unread};
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
pub(crate) const ENTER_AGAIN_QUIET_MS: f64 = 3_000.0;
/// How long a read of the window may take and still say what the window does now:
/// a pane host answers a snapshot at once, so one that took longer stalled, and what
/// it says is as old as the stall.
const SLOW_READ_MS: i64 = 5_000;
/// Why a snapshot came to nothing.
const NO_ANSWER: &str = "its pane host did not answer in time";
/// How often at most the log says a message's window could not be read.
const UNREAD_TOLD_MS: i64 = 60_000;
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
    /// The rest began again, with no turn, at a look after one that could not read
    /// the window.
    read_again: bool,
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
        if delivering.queued {
            return Ok(());
        }
        // The harness's own word, once it has one: its agent has the message.
        if self.took(record) {
            self.queued(record);
            return Ok(());
        }
        // A look that could not read the record says nothing of the paste.
        if observed
            .reading
            .as_deref()
            .is_some_and(Reading::read_failed)
        {
            self.unknown(record, "its record could not be read");
            return Ok(());
        }
        let now = self.now();
        let waited = now - delivering.since;
        let Rest {
            rested,
            turned,
            read_again,
            free,
        } = self.rested(record, observed, self.held(record));
        let at_rest = observed.settled && observed.waiting.is_none();
        let overdue = at_rest && free.is_some_and(|free| now - free > RECONCILE_MS);
        if rested <= self.seams.limits.arrival_ms && !overdue {
            // Only a paste waits in an input box for an Enter: what a harness took
            // over its own channel is reconciled by its record alone, however
            // uncertain its hand-over (Astra's review of 2026-10-20, round 10).
            if waited > ENTER_AGAIN_MS
                && waited <= self.seams.limits.arrival_ms
                && !delivering.entered_again
                && self.pasted(record)
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
        if !self.read_once_more(record, delivering.message).await? {
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
        } else if read_again {
            format!(
                "the harness record never showed it in the {} s its window was at rest since it could be read again",
                self.seams.limits.arrival_ms / 1000
            )
        } else {
            "the harness record never showed it".to_owned()
        };
        self.settle_failure(delivering, &because, true)
    }

    /// Whether the window takes what it is handed pasted into its input box.
    fn pasted(&self, record: &Record) -> bool {
        matches!(record.window.borrow().input, Input::Pasted { .. })
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
    /// to show it. A look after one that could not read the window starts the count
    /// again too ([`Dispatcher::unknown`]). A look that finds the window in a turn starts the count
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
            delivering.turned = true;
            return Rest {
                rested: 0,
                turned: true,
                read_again: false,
                free: None,
            };
        }
        let free = Some(*delivering.free_since.get_or_insert(now));
        if observed.settled && observed.waiting.is_none() {
            let since = *delivering.rested_since.get_or_insert(now);
            Rest {
                rested: now - since,
                turned: delivering.turned,
                read_again: since != delivering.since,
                free,
            }
        } else {
            delivering.rested_since = None;
            delivering.turned = true;
            Rest {
                rested: 0,
                turned: true,
                read_again: false,
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
        // A host that cannot say what the window does now says nothing of the paste.
        let Some(snapshot) = self.snapshot_now(&pane).await else {
            self.entered_again(record, message, false);
            self.unknown(record, NO_ANSWER);
            return Ok(());
        };
        let snapshot = Some(snapshot);
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
        // Enter sends what waits in the input box: only as the ledger admits the
        // paste now, its project open and nothing of it withdrawn while the screen
        // was looked at (`deliveries`). Withheld, the paste stays in the box, and
        // the next paste into the window empties it first: else it would go in
        // beside it, and its Enter send both (both reviews of 2026-10-20, round 10).
        match self.enter_admitted(record, message)? {
            Enter::Go => {}
            Enter::Withheld => {
                self.entered_again(record, message, false);
                let since = self.seams.ledger.borrow_mut().now();
                let mut part = record.delivery.borrow_mut();
                let carried = part
                    .delivering
                    .as_ref()
                    .filter(|delivering| delivering.message == message)
                    .map(|delivering| delivering.carried.clone())
                    .unwrap_or_default();
                part.uncleared = Some(Uncleared {
                    pane,
                    message,
                    carried,
                    since,
                    unquiet_since: None,
                    unquiet_last: None,
                });
                return Ok(());
            }
            Enter::Arrived => return Ok(()),
        }
        let input = json!({ "id": pane.id, "generation": pane.generation, "bytes": [ENTER] });
        // A key the host did not take is pressed again at the next look's
        // Enter, if any: `.catch(() => {})` made a promise of its own to wait on.
        let _ = returning(self.seams.host.request("pane.input", input)).await;
        self.trace_window(record, WindowEvent::EnterAgain { message })
    }

    /// What became of the paste on its way since the window was last read is not
    /// known: a read of it failed, or came too late to say what it says. The rest the
    /// record has had to show it begins again at the next look that reads the window
    /// at rest.
    /// The daemon's log says so, `why` the read came to nothing, at most once in
    /// [`UNREAD_TOLD_MS`] for a message: which window, which message, and how many
    /// reads came to nothing in how long. Never what the window shows.
    pub(crate) fn unknown(&self, record: &Record, why: &str) {
        let now = self.now();
        let told = {
            let mut part = record.delivery.borrow_mut();
            let Some(delivering) = part.delivering.as_mut() else {
                return;
            };
            delivering.rested_since = None;
            let message = delivering.message;
            let unread = part
                .unread
                .take()
                .filter(|unread| unread.message == message)
                .unwrap_or(Unread {
                    message,
                    first: now,
                    reads: 0,
                    told: None,
                });
            let unread = part.unread.insert(Unread {
                reads: unread.reads + 1,
                ..unread
            });
            if unread.told.is_some_and(|told| now - told < UNREAD_TOLD_MS) {
                return;
            }
            unread.told = Some(now);
            (message, unread.reads, (now - unread.first) / 1000)
        };
        let (message, reads, secs) = told;
        let window = record.window.borrow().pane.as_ref().map_or_else(
            || "a window".to_owned(),
            |pane| format!("the window {}", pane.id),
        );
        self.seams.log.warn(&format!(
            "{window} could not be read ({why}): m-{message} waits, its rest begun again; \
             {reads} reads of it came to nothing in {secs} s"
        ));
    }

    /// The pane host's word on the window now: none where it failed, or came too late
    /// ([`SLOW_READ_MS`]) to say what the window does now.
    async fn snapshot_now(&self, pane: &Pane) -> Option<Value> {
        let asked = self.now();
        let body = json!({ "id": pane.id, "generation": pane.generation });
        // `.catch(() => null)` made a promise of its own to wait on, a turn
        // after the host's answer.
        let snapshot = returning(self.seams.host.request("pane.snapshot", body))
            .await
            .ok()?;
        let in_time = self.now() - asked <= SLOW_READ_MS;
        (in_time && snapshot.get("ok") == Some(&Value::Bool(true))).then_some(snapshot)
    }

    /// Reads the window once more before `message`, which its record never showed,
    /// is pasted again: whether it is to go. The pane host must say at once that no
    /// paste is going in, and the record, read after that, must still not show it:
    /// one that does confirms it. A read that fails or comes late is unknown, and so
    /// is a host still writing a paste: the rest begins again, and nothing goes. So
    /// does a window that is in a turn by then. One no longer on its way is done with.
    async fn read_once_more(
        self: &Rc<Self>,
        record: &Rc<Record>,
        message: i64,
    ) -> Result<bool, EngineError> {
        let (window, pane) = {
            let part = record.window.borrow();
            (part.window.clone(), part.pane.clone())
        };
        let (Some(window), Some(pane)) = (window, pane) else {
            return Ok(true);
        };
        let Some(snapshot) = self.snapshot_now(&pane).await else {
            self.unknown(record, NO_ANSWER);
            return Ok(false);
        };
        if js::truthy(snapshot.get("pasteInFlight")) {
            self.unknown(record, "its pane host is writing a paste into it still");
            return Ok(false);
        }
        // A paste the host held while it was slow, written only now: the record has
        // its whole time from then.
        let pasted = snapshot.get("pasteQuietMs").and_then(Value::as_f64);
        if let Some(pasted) = pasted.filter(|&pasted| pasted < self.seams.limits.arrival_ms as f64)
        {
            let written = self.now() - pasted as i64;
            if let Some(delivering) = record.delivery.borrow_mut().delivering.as_mut() {
                delivering.rested_since = Some(written);
            }
            self.seams.log.warn(&format!(
                "the window {} was written a paste {} ms ago, as m-{message} was to go again: \
                 its record has its whole time from then",
                pane.id, pasted as i64
            ));
            return Ok(false);
        }
        let Ok(looked) = returning(window.observe()).await else {
            self.unknown(record, "a look at it failed");
            return Ok(false);
        };
        let on_its_way = record
            .delivery
            .borrow()
            .delivering
            .as_ref()
            .is_some_and(|delivering| delivering.message == message);
        if self.forgotten(record) || !on_its_way {
            return Ok(false);
        }
        if looked.reading.as_deref().is_some_and(Reading::read_failed) {
            self.unknown(record, "its record could not be read");
            return Ok(false);
        }
        if self.confirm_arrival(record, &looked)? {
            return Ok(false);
        }
        if !(looked.settled && looked.waiting.is_none()) {
            if let Some(delivering) = record.delivery.borrow_mut().delivering.as_mut() {
                delivering.rested_since = None;
                delivering.turned = true;
            }
            return Ok(false);
        }
        Ok(true)
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
