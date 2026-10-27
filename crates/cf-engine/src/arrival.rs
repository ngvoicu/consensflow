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
//! queue behind it, until the human stops the turn or closes the window. The one Enter is pressed whatever the
//! window shows (a quiet dialog may take it), and the screen is looked at apart from
//! the press, so a human's typing in between can meet it. What the daemon's stop leaves
//! on its way is settled at the start by the record alone (`in_flight`): one it does not
//! show goes again, so a paste a window held queued when the daemon stopped, and ran
//! later, is read twice. Where Devin's store says its TUI took it, the paste that goes
//! again says in its header that it may be there already.
//! A paste whose window can no longer be read waits with no bound: while every look fails
//! or its pane host does not answer in time, no Enter is pressed, it is never pasted
//! again, and no attempt is counted. What ends that is a look that reads the window
//! again (then the arrival window of rest, or the half hour, as ever), the window's
//! exit, the message leaving its way (its task cancelled, the window taken back), or
//! the daemon's restart, which gives a message whose record cannot be read back to its
//! queue (`in_flight`).
//!
//! What a hand-over is and how it fails are `deliveries`'; the wait is the part of
//! the record's delivery part this owns.

use std::rc::Rc;

use cf_base::js;
use cf_harness::contract::{Input, InputBox, Observed, Pane};
use cf_harness::records::{Reading, Role, Settlement};
use cf_ledger::NewNote;
use cf_proto::ledger::MessageView;
use cf_proto::trace::WindowEvent;
use serde_json::{json, Value};

use crate::deliveries::{Delivering, Enter, Uncleared, Unread};
use crate::delivery_text::{header_shape, is_paste_of, marker_of};
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

/// What the looks at a window read while a message was on its way, for its retry or
/// failure to say (`look`, 3.0.7 chief-meeting CMT-08, T-760's F5): every retry
/// classifies itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Watched {
    message: i64,
    /// When the last look at it came, on the engine's clock.
    last: i64,
    /// The longest time between two looks at it: a loaded machine's.
    longest_pass: i64,
    /// How long the window's screen had printed nothing, as the pane host said before
    /// it was to go again.
    output_quiet_ms: Option<i64>,
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
        self.watched(record, &delivering);
        if delivering.launch {
            // A first message waits as long as its window asks one of the questions its
            // harness asks as it starts (its folder's trust, its folder's grants, the
            // project's new MCP servers), and has its whole time once it is past them
            // (`trust`). One still owed to a window that has neither asked nor shown its
            // prompt spends its time meanwhile: a window that shows neither is never ready
            // for it.
            let trust = record.window.borrow().trust;
            if trust.asks() || (trust == Trusting::Looking && delivering.owed.is_none()) {
                return Ok(());
            }
            let waited = self.now() - delivering.since;
            // One handed over by a paste once its window was up is pressed Enter for
            // once more, as any paste is; one its window read as it opened never is.
            if delivering.late
                && !delivering.entered_again
                && waited > ENTER_AGAIN_MS
                && self.pasted(record)
            {
                return self.enter_again(record, delivering.message).await;
            }
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
            // A window delivered its first message once up that never took it was
            // never ready for it (`hand_first`).
            let because = if delivering.owed.is_some() {
                "the window was never ready for its first message"
            } else {
                "the window never showed its first message"
            };
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
        // A record that holds the message's header in an item the proof cannot match
        // has it, unproven: it is never pasted again (CMT-23).
        if self.arrived_unproven(record, delivering.message)? {
            return Ok(());
        }
        // Nor does a paste go again that is still there for the window: on its way to
        // its record, in its input box, or on its screen (CMT-24).
        if self.paste_still_there(record, delivering.message).await? {
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
        let look = self.arrival_look(record, observed, delivering.message, rested);
        self.settle_failure_looked(delivering, &because, true, Some(&look))
    }

    /// Whether the paste of `message`, which the record never showed in its whole wait,
    /// is still there for the window (3.0.7 chief-meeting, CMT-24, T-760's F4: on a loaded
    /// Mac, m-3093 and m-4273 had no record of their paste for 177 s and 196 s, and were
    /// pasted again). It is where the window's screen printed within the last
    /// [`ENTER_AGAIN_QUIET_MS`] (a window that draws is at work, or taking the paste in), and
    /// where its input box holds it: then an Enter is pressed once more, as the hand-over's
    /// is ([`Dispatcher::enter_again`], whose guards keep the human's own words from going
    /// with it). Either way its rest begins again, and nothing is pasted: a paste goes
    /// again only where it is gone from the screen as from the record. Only a paste is
    /// asked of: what a harness took through a channel of its own is its record's.
    async fn paste_still_there(
        self: &Rc<Self>,
        record: &Rc<Record>,
        message: i64,
    ) -> Result<bool, EngineError> {
        if !self.pasted(record) {
            return Ok(false);
        }
        let (window, pane) = {
            let part = record.window.borrow();
            (part.window.clone(), part.pane.clone())
        };
        let (Some(window), Some(pane)) = (window, pane) else {
            return Ok(false);
        };
        let printing = record
            .delivery
            .borrow()
            .watched
            .filter(|watched| watched.message == message)
            .and_then(|watched| watched.output_quiet_ms)
            .is_some_and(|quiet| (quiet as f64) < ENTER_AGAIN_QUIET_MS);
        let boxed = if printing {
            None
        } else {
            returning(window.input_box(&*self.seams.host, &pane)).await
        };
        let on_its_way = record
            .delivery
            .borrow()
            .delivering
            .as_ref()
            .is_some_and(|delivering| delivering.message == message);
        if self.forgotten(record) || !on_its_way {
            return Ok(true);
        }
        let held = boxed == Some(InputBox::Holds);
        if !(printing || held) {
            return Ok(false);
        }
        if held {
            self.enter_again(record, message).await?;
        }
        let now = self.now();
        if let Some(delivering) = record
            .delivery
            .borrow_mut()
            .delivering
            .as_mut()
            .filter(|delivering| delivering.message == message)
        {
            // A whole wait from now, the half hour's too: the next ask comes no sooner.
            delivering.rested_since = Some(now);
            delivering.free_since = Some(now);
        }
        self.seams.log.warn(&format!(
            "m-{message} was to go again, its record never having shown it: its window {}; it waits once more, and is not pasted again",
            if held {
                "holds it in its input box, and Enter was pressed once more"
            } else {
                "printed on its screen within the last seconds"
            }
        ));
        Ok(true)
    }

    /// A look at the window while `delivering` is on its way: the time since the last
    /// one, or since its hand-over, is a pass, the longest of which its retry says.
    fn watched(&self, record: &Record, delivering: &Delivering) {
        let (now, message) = (self.now(), delivering.message);
        let mut part = record.delivery.borrow_mut();
        let watched = part
            .watched
            .filter(|watched| watched.message == message)
            .map_or(
                Watched {
                    message,
                    last: now,
                    longest_pass: now - delivering.since,
                    output_quiet_ms: None,
                },
                |watched| Watched {
                    last: now,
                    longest_pass: watched.longest_pass.max(now - watched.last),
                    ..watched
                },
            );
        part.watched = Some(watched);
    }

    /// What the looks read of the window as `message` is to go again, which its retry
    /// or failure says (CMT-08): how many of the record's items hold its header and
    /// where the first of them has it, the record's last item, how long the window
    /// rested, the longest pass and how long its screen printed nothing.
    fn arrival_look(&self, record: &Record, observed: &Observed, message: i64, rested: i64) -> Value {
        let marker = marker_of(message);
        let items = observed.items();
        let holding: Vec<_> = items
            .iter()
            .filter(|item| item.text.contains(&marker))
            .collect();
        let watched = record
            .delivery
            .borrow()
            .watched
            .filter(|watched| watched.message == message);
        json!({
            "markers": holding.len(),
            "first": holding.first().map(|item| json!({
                "role": item.role,
                "shape": header_shape(&item.text, message),
            })),
            "lastItem": items.last().map(|item| &*item.id),
            "restMs": rested,
            "longestPassMs": watched.map(|watched| watched.longest_pass),
            "outputQuietMs": watched.and_then(|watched| watched.output_quiet_ms),
        })
    }

    /// Whether `message`, on its way, arrived unproven (3.0.7 chief-meeting, CMT-23): a
    /// user item its window's record was given after its hand-over holds its header, and
    /// the proof (`is_paste_of`) does not take it for its paste. It is then taken for
    /// arrived, never pasted again and never said not to have reached its window: its
    /// shape is logged for filing (`delivery.unproven`), and whoever can act on it is
    /// told once. A human who quoted the header there meanwhile is covered by that note.
    fn arrived_unproven(&self, record: &Record, message: i64) -> Result<bool, EngineError> {
        let found = {
            let ledger = self.seams.ledger.borrow();
            let Some(handed) = ledger.handed_at(message)? else {
                return Ok(false);
            };
            ledger
                .copied_items_since(record.id, &marker_of(message), &handed)?
                .into_iter()
                .next()
        };
        let Some((item, text)) = found else {
            return Ok(false);
        };
        let on_its_way = self.seams.ledger.borrow().message(message)?;
        let Some(on_its_way) = on_its_way.filter(|found| found.state == "delivering") else {
            return Ok(false);
        };
        let tell = self.unproven_note(&on_its_way)?;
        self.seams.ledger.borrow_mut().confirm_unproven(
            message,
            &item,
            &header_shape(&text, message),
            &tell,
        )?;
        record.delivery.borrow_mut().delivering = None;
        self.changed();
        Ok(true)
    }

    /// The note that says `message` arrived unproven: to its task's requester where
    /// that is another than its recipient, else to the chief, who for its own window
    /// is the recipient, and reads it there.
    fn unproven_note(&self, message: &MessageView) -> Result<NewNote, EngineError> {
        let requester = match message.task_number {
            Some(number) => self
                .seams
                .ledger
                .borrow()
                .task(message.project_id, number)?
                .map(|thread| thread.task.requester),
            None => None,
        };
        let to = requester
            .filter(|requester| *requester != message.recipient)
            .unwrap_or_else(|| "chief".to_owned());
        Ok(NewNote {
            from: None,
            to,
            task: message.task_number,
            body: format!(
                "m-{id}'s arrival couldn't be proven from the record; it was not pasted again. @{who} can read it with `cf inbox read m-{id}`.",
                id = message.id,
                who = message.recipient,
            ),
        })
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
        self.give_back(delivering.message, because)?;
        self.changed();
        Ok(())
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
    pub(crate) async fn snapshot_now(&self, pane: &Pane) -> Option<Value> {
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
        if let Some(watched) = record
            .delivery
            .borrow_mut()
            .watched
            .as_mut()
            .filter(|watched| watched.message == message)
        {
            watched.output_quiet_ms = snapshot
                .get("outputQuietMs")
                .and_then(Value::as_f64)
                .map(|quiet| quiet as i64);
        }
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
