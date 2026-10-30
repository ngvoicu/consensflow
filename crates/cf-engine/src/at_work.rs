//! A window at work is no window that lost what it was given (T-901). A Codex window
//! opened again on a long thread compacted it before it took its first message, its
//! record an open turn and nothing more, its screen "• Compacting context (2m 57s • esc
//! to interrupt)", and the first message's time ran out under it: the window was closed
//! as one that "never showed its first message", which cut the compaction, and the
//! chief's words in it reached nobody (field, 3.0.6-alpha.2, 2026-10-28).
//!
//! So a window is at work by its record (a turn in flight, or the turn its harness holds
//! the message in) or by its screen (its harness's status that it compacts,
//! [`cf_harness::contract::Window::compacting`]: Claude Code's and Pi's records say
//! nothing of a compaction until it is over), and no delivery's time runs out while it
//! is: a launch's first message found so at the end of its time is given its whole time
//! again from that look, and a later one is never pasted again while its window
//! compacts (`arrival`'s `read_once_more`); the rest its record has to show it counts
//! from the compaction's end. The wait is bounded by the harness finishing, never by a
//! clock. A message that has waited [`TOLD_AFTER_MS`] for a window that compacts still,
//! or a first message for a window at work on something else all that time, is told
//! once, to whoever sent it and to the human, in words that name what it waits on:
//! nothing is given up or pasted again for it.
//!
//! Limits. The screen is looked at for this only where the message waits at a window
//! that reads at work, past [`TOLD_AFTER_MS`], once a [`LOOK_EVERY_MS`] at most, and at
//! the moments a delivery would be given up or pasted again: a compaction that ends
//! between two looks is not seen. A status line drawn in words its reader does not
//! know (a harness that words it otherwise since an update) reads as no compaction:
//! where its record reads at rest the message is then given up, or pasted again, as
//! before. A window at work that never ends holds what waits for it until the human
//! ends the turn or closes the window.
//!
//! The record's sign is not always there (Calliope's P3-3 and P3-4 of T-917). A window
//! opened again on a conversation whose last turn the quit cut reads that turn settled
//! while its record holds nothing new of the window's own (`LeftOpen`, in each adapter);
//! a Codex that compacts at the start of the turn it takes there writes an open turn and
//! nothing more, so only its screen says it is at work. And a window reads at work for as
//! long as its harness's live status says it is busy: a Claude Code opened again whose
//! status file never comes to rest holds its first message for good, told of once at
//! [`TOLD_AFTER_MS`], where before it was given up in its launch's time.

use std::rc::Rc;

use cf_harness::contract::Observed;
use cf_harness::records::Reading;
use cf_ledger::NewNote;

use crate::deliveries::{described, Delivering};
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::shown::Shown;

/// How long a message may wait on its window's work before whoever sent it and the
/// human are told: well past the minutes a compaction takes (Codex's at xhigh in the
/// field, three and more), so that only one that is stuck, or a first message behind
/// work that is not its own, is told of.
pub(crate) const TOLD_AFTER_MS: i64 = 15 * 60_000;

/// How often at most the screen of a window at work is looked at for a compaction once
/// a message has waited [`TOLD_AFTER_MS`] on it.
const LOOK_EVERY_MS: i64 = 60_000;

/// What the looks found of the work of the window a message waits on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct OnWork {
    /// The last look at its screen found its harness compacting.
    pub(crate) compacting: bool,
    /// When its screen was last looked at for it.
    pub(crate) looked: Option<i64>,
    /// Since when a launch's first message has waited past its time on the window's
    /// work: its time is given again at each look that finds it at work.
    pub(crate) since: Option<i64>,
}

impl Dispatcher {
    /// Whether the look reads the window's record at work: a turn in flight in the
    /// conversation the window shows, which holds what was said before it (the field's
    /// thread, opened again: Codex compacts only a thread that has had turns), or the turn
    /// its harness holds the message on its way in (`Window::held`). A record that holds
    /// nothing at all says nothing was written, the first message least of all, however
    /// busy its window reads; and a record that could not be read, a window that waits
    /// on a dialog, one that has not named its conversation and one that shows another
    /// read as no work.
    pub(crate) fn reads_at_work(&self, record: &Record, observed: &Observed) -> bool {
        self.window_of(record).0.is_some_and(|window| window.held())
            || (matches!(observed.reading.as_deref(), Some(Reading::Known(_)))
                && !observed.items().is_empty()
                && !observed.settled
                && observed.waiting.is_none()
                && !observed.unnamed
                && observed.switched.is_none())
    }

    /// Whether `shown`, a look at the window's screen, says its harness compacts its
    /// conversation now. Kept for `message`, while it is the one on its way: since when
    /// the looks have found it compacting, none once one does not.
    pub(crate) fn compacting(&self, record: &Record, message: i64, shown: &Shown) -> bool {
        let compacting = self
            .window_of(record)
            .0
            .is_some_and(|window| window.compacting(shown.lines()));
        let now = self.now();
        on_its_way(record, message, |delivering| {
            delivering.work.looked = Some(now);
            delivering.work.compacting = compacting;
        });
        compacting
    }

    /// A launch's first message whose time ran out while its window is at work (by its
    /// record, or `compacting` by its screen): it is given its whole time again from now,
    /// and told of once it has waited [`TOLD_AFTER_MS`].
    pub(crate) fn wait_on_work(&self, record: &Record, message: i64) -> Result<(), EngineError> {
        let now = self.now();
        on_its_way(record, message, |delivering| {
            delivering.work.since.get_or_insert(delivering.since);
            delivering.since = now;
        });
        self.tell_waits_on_work(record, message)
    }

    /// Looks at the screen of a window that reads at work while the message on its way
    /// (`delivering`) has waited [`TOLD_AFTER_MS`] on it, once a [`LOOK_EVERY_MS`] at
    /// most, until it is told: one that compacts still is told of.
    pub(crate) async fn watch_work(
        &self,
        record: &Rc<Record>,
        delivering: &Delivering,
    ) -> Result<(), EngineError> {
        let now = self.now();
        let waited = now - delivering.work.since.unwrap_or(delivering.since);
        if told_already(record, delivering.message)
            || waited < TOLD_AFTER_MS
            || delivering
                .work
                .looked
                .is_some_and(|looked| now - looked < LOOK_EVERY_MS)
        {
            return Ok(());
        }
        let Some(pane) = record.window.borrow().pane.clone() else {
            return Ok(());
        };
        let shown = self.look_at(&pane).await;
        if on_its_way(record, delivering.message, |_| ()).is_none() {
            return Ok(());
        }
        self.compacting(record, delivering.message, &shown);
        self.tell_waits_on_work(record, delivering.message)
    }

    /// Whoever sent the message on its way and the human are told, once, that it has
    /// waited [`TOLD_AFTER_MS`] or more on its window, which compacts its conversation
    /// still, or, a launch's first message, which is at work on something else. Nothing
    /// is told of a message that waits less, or on a window at rest. Once a message:
    /// pasted again, it is a new hand-over of the same words (P3-7 of T-917).
    pub(crate) fn tell_waits_on_work(
        &self,
        record: &Record,
        message: i64,
    ) -> Result<(), EngineError> {
        let now = self.now();
        let Some((compacting, launch, waited)) = on_its_way(record, message, |delivering| {
            let from = delivering.work.since.unwrap_or(delivering.since);
            (delivering.work.compacting, delivering.launch, now - from)
        }) else {
            return Ok(());
        };
        if told_already(record, message) {
            return Ok(());
        }
        if waited < TOLD_AFTER_MS || !(compacting || launch) {
            return Ok(());
        }
        let Some(found) = self.seams.ledger.borrow().message(message)? else {
            return Ok(());
        };
        record.delivery.borrow_mut().told_work = Some(message);
        let window = &found.recipient;
        let what = described(&found);
        let minutes = TOLD_AFTER_MS / 60_000;
        let body = if compacting {
            format!(
                "@{window}'s window has been compacting its conversation for {minutes} min or more, \
                 and m-{message}, {what}, waits for it to finish: it goes in once the window takes \
                 it, and nothing is pasted again meanwhile. If the compaction is stuck, look at \
                 that window."
            )
        } else {
            format!(
                "@{window}'s window has been at work for {minutes} min or more without taking \
                 m-{message}, {what}, its first message, which waits for it: it goes in once the \
                 window takes it, and nothing is pasted again meanwhile. If the window is stuck, \
                 look at it."
            )
        };
        let project = self.known_project(found.project_id)?;
        let sender = found.sender.as_deref().filter(|sender| *sender != "human");
        for to in sender.into_iter().chain(["human"]) {
            self.seams.ledger.borrow_mut().note(
                project.id,
                &NewNote {
                    from: None,
                    to: to.to_owned(),
                    task: found.task_number,
                    body: body.clone(),
                },
            )?;
        }
        self.changed();
        Ok(())
    }
}

/// Whether whoever sent `message` and the human were told it waits on its window's work.
fn told_already(record: &Record, message: i64) -> bool {
    record.delivery.borrow().told_work == Some(message)
}

/// What `change` makes of the message on its way into `record`'s window, where it is
/// `message`: none where it is not.
fn on_its_way<T>(
    record: &Record,
    message: i64,
    change: impl FnOnce(&mut Delivering) -> T,
) -> Option<T> {
    record
        .delivery
        .borrow_mut()
        .delivering
        .as_mut()
        .filter(|delivering| delivering.message == message)
        .map(change)
}
