//! A window whose task stopped stops too. A pause asks a stop of its task's
//! window (the ledger counts them: a stop is a task and its sequence), and a
//! stop is owed until the window has paid it, in the engine's own record of
//! that window, which dies with it. It is paid when the window is seen at
//! rest: its turn is over, nothing is asked of it and nothing is being
//! pasted into it; no key is pressed into a window at rest, and a window
//! that was opened after the pause owes nothing for it. While the window is
//! at work, read from what its harness's record shows and never from what
//! the board says, it is interrupted: Escape, as many presses as its harness
//! asks for, again a few seconds later, three rounds in all. A window that
//! ignores them all has not stopped, and ConsensFlow says so, to the human
//! and to whoever gave the task, and goes on trying once a minute for as long
//! as it works: it never kills the window, and what is for it waits until
//! its turn ends, when it is paid. An agent still at work on a turn about a
//! task cancelled under it is interrupted the same way, three rounds at most.

use std::rc::Rc;

use cf_harness::contract::Observed;
use cf_harness::records::Role;
use cf_ledger::{NewNote, ParticipantView, ProjectView, Stop};

use crate::delivery_text::marker_of;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::windows::press_interrupt;

/// How many rounds of Escape a stop gets before it is given up on for a
/// while, and how soon one round follows another.
const ROUNDS: u32 = 3;
const ROUND_EVERY_MS: i64 = 3_000;
/// How often a stop that was ignored in every round is tried again.
const EXHAUSTED_EVERY_MS: i64 = 60_000;

/// What a window was interrupted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopKey {
    /// A pause of its task: the task and the sequence of the stop it asked.
    Pause { task: i64, seq: i64 },
    /// A turn about a task that was cancelled, which the agent has not left.
    Cancelled { task: i64 },
}

/// The rounds one stop has had: how many were pressed, when the last was
/// (or when the stop was given up on), and whether it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Interrupted {
    key: StopKey,
    rounds: u32,
    at: i64,
    exhausted: bool,
}

/// A stop a window ignored in every round, and still has not paid: the
/// number of its task, and how many times it has been ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unstopped {
    pub task: i64,
    pub rounds: u32,
}

/// What a look at a window found it to be, as far as a stop goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    /// Its turn is over, and nothing is asked of it or on its way into it.
    Rest,
    /// It is at work.
    Work,
    /// Starting, waiting on its own dialog or being pasted into: nothing yet.
    Neither,
}

/// What the cadence says to do at a look at a window at work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Round {
    Press,
    /// Every round was ignored: say so, once, and press none this look.
    Exhaust,
    Wait,
}

/// The cadence: the first press of a stop is at once; a round follows three
/// seconds after the last, up to three; three seconds after the third, a
/// stop of a pause is exhausted, and tried again once a minute; a cancelled
/// task's turn gets its three rounds and no more.
fn next_round(done: Option<&Interrupted>, key: StopKey, now: i64) -> Round {
    let Some(done) = done.filter(|done| done.key == key) else {
        return Round::Press;
    };
    let since = now - done.at;
    if done.exhausted {
        return if since >= EXHAUSTED_EVERY_MS {
            Round::Press
        } else {
            Round::Wait
        };
    }
    if since < ROUND_EVERY_MS {
        Round::Wait
    } else if done.rounds < ROUNDS {
        Round::Press
    } else if matches!(key, StopKey::Pause { .. }) {
        Round::Exhaust
    } else {
        Round::Wait
    }
}

impl Dispatcher {
    /// A stop that a window ignored in every round: none while it is paid
    /// or was never asked.
    pub fn unstopped(&self, participant: i64) -> Option<Unstopped> {
        self.record(participant)
            .and_then(|record| record.window.borrow().unstopped)
    }

    /// One look at a member's window, as far as the stops of its task go. At
    /// rest, what a door claimed and nobody acknowledged is the ledger's
    /// again, and a stop owed is paid with no key; at work, it is
    /// interrupted by the cadence. `starting` is whether the look found the
    /// window still drawing its screen or unnamed: it is neither.
    pub(crate) async fn settle_stop(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        observed: &Observed,
        starting: bool,
    ) -> Result<(), EngineError> {
        let look = if starting
            || observed.waiting.is_some()
            || record.delivery.borrow().delivering.is_some()
        {
            Look::Neither
        } else if observed.settled {
            Look::Rest
        } else {
            Look::Work
        };
        if look == Look::Rest {
            self.seams
                .ledger
                .borrow_mut()
                .release_claims(participant.id, "its window is at rest")?;
        }
        let stop = self.seams.ledger.borrow().stop_of(participant.id)?;
        if let Some(stop) = stop.filter(|stop| self.owes(record, stop)) {
            return match look {
                Look::Rest => {
                    self.pay(record, &stop);
                    Ok(())
                }
                Look::Work => {
                    self.interrupt_for(project, participant, record, &stop)
                        .await
                }
                Look::Neither => Ok(()),
            };
        }
        if look == Look::Work {
            self.interrupt_cancelled(participant, record, observed)
                .await?;
        }
        Ok(())
    }

    /// Whether the window owes the stop: it has not paid one of this task
    /// that came as late.
    fn owes(&self, record: &Record, stop: &Stop) -> bool {
        let paid = record
            .window
            .borrow()
            .stopped
            .filter(|(task, _)| *task == stop.task_id)
            .map_or(0, |(_, seq)| seq);
        stop.seq > paid
    }

    /// The words that begin a task for a window at rest are being begun: they
    /// answer every pause asked of that task until now, with no key. A pause
    /// of a task none of whose words had reached the window was one no look at
    /// rest could pay (a window works on the task its newest words were about)
    /// and no turn of the window was for it to stop; left owed, it would
    /// interrupt the very turn these words begin. A pause that comes while
    /// they are pasted has a greater sequence, and is still owed.
    pub(crate) fn pay_with_words(&self, record: &Record) -> Result<(), EngineError> {
        let stop = self.seams.ledger.borrow().stop_of(record.id)?;
        if let Some(stop) = stop {
            self.pay(record, &stop);
        }
        Ok(())
    }

    /// The window is at rest, and the stop is paid with no key. What it had
    /// of the rounds is over, and so is what the board said of it.
    fn pay(&self, record: &Record, stop: &Stop) {
        let unstopped = {
            let mut part = record.window.borrow_mut();
            part.stopped = Some((stop.task_id, stop.seq));
            part.interrupted = None;
            part.unstopped.take()
        };
        if unstopped.is_some() {
            self.changed();
        }
    }

    /// The window is at work on a turn the stop is for: a round of Escape if
    /// the cadence says so; or the stop is given up on this look.
    async fn interrupt_for(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        stop: &Stop,
    ) -> Result<(), EngineError> {
        let key = StopKey::Pause {
            task: stop.task_id,
            seq: stop.seq,
        };
        let done = record.window.borrow().interrupted.clone();
        match next_round(done.as_ref(), key, self.now()) {
            Round::Wait => Ok(()),
            Round::Press => {
                self.press_round(record, key, stop.number);
                self.press(record).await;
                Ok(())
            }
            Round::Exhaust => {
                let rounds = self.exhaust(record, key, stop.number);
                self.say_ignored(project, participant, stop.number, rounds)
            }
        }
    }

    /// The window is at work and the task it was last given was cancelled:
    /// when its turn is the one about it, three rounds of Escape, and no more.
    async fn interrupt_cancelled(
        self: &Rc<Self>,
        participant: &ParticipantView,
        record: &Rc<Record>,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let cancelled = self.seams.ledger.borrow().last_task(participant.id)?;
        let Some(cancelled) = cancelled.filter(|thread| thread.task.state == "cancelled") else {
            return Ok(());
        };
        let turn = observed
            .items()
            .iter()
            .rev()
            .find(|item| item.role == Role::User);
        let about = cancelled.messages.iter().any(|message| {
            message.recipient_id == participant.id
                && turn.is_some_and(|turn| turn.text.contains(&marker_of(message.id)))
        });
        if !about {
            return Ok(());
        }
        let key = StopKey::Cancelled {
            task: cancelled.task.id,
        };
        let done = record.window.borrow().interrupted.clone();
        if next_round(done.as_ref(), key, self.now()) == Round::Press {
            self.press_round(record, key, cancelled.task.number);
            self.press(record).await;
        }
        Ok(())
    }

    /// A round is pressed: counted. A stop that was given up on already shows
    /// its new count to the board; a stop that begins shows none of what the
    /// one before it ignored.
    fn press_round(&self, record: &Record, key: StopKey, number: i64) {
        let now = self.now();
        let changed = {
            let mut part = record.window.borrow_mut();
            let before = part.interrupted.clone().filter(|done| done.key == key);
            let rounds = before.as_ref().map_or(0, |done| done.rounds) + 1;
            let exhausted = before.is_some_and(|done| done.exhausted);
            part.interrupted = Some(Interrupted {
                key,
                rounds,
                at: now,
                exhausted,
            });
            let shown = exhausted.then_some(Unstopped {
                task: number,
                rounds,
            });
            std::mem::replace(&mut part.unstopped, shown) != shown
        };
        if changed {
            self.changed();
        }
    }

    /// Every round of the stop was ignored: it is marked, once, and the
    /// board shows it. How many times it was ignored.
    fn exhaust(&self, record: &Record, key: StopKey, number: i64) -> u32 {
        let rounds = {
            let mut part = record.window.borrow_mut();
            let rounds = part.interrupted.as_ref().map_or(0, |done| done.rounds);
            part.interrupted = Some(Interrupted {
                key,
                rounds,
                at: self.now(),
                exhausted: true,
            });
            part.unstopped = Some(Unstopped {
                task: number,
                rounds,
            });
            rounds
        };
        self.changed();
        rounds
    }

    /// The human, and whoever gave the task if it was not the human, are told
    /// once that the window did not stop and what that means.
    fn say_ignored(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        number: i64,
        rounds: u32,
    ) -> Result<(), EngineError> {
        let handle = &participant.handle;
        let requester = self
            .seams
            .ledger
            .borrow()
            .task(project.id, number)?
            .map(|thread| thread.task.requester);
        let mut ledger = self.seams.ledger.borrow_mut();
        ledger.note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: Some(number),
                body: format!(
                    "@{handle} did not stop for T-{number}: it ignored the interrupt {rounds} times and is still on its earlier turn. What is for it waits until that turn ends, and what it writes before then is not T-{number}'s result. To stop it now, cancel T-{number}, or reassign it if it was given by tier."
                ),
            },
        )?;
        if let Some(requester) = requester.filter(|requester| requester != "human") {
            ledger.note(
                project.id,
                &NewNote {
                    from: None,
                    to: requester,
                    task: Some(number),
                    body: format!(
                        "T-{number}'s window (@{handle}) did not stop: it ignored the interrupt and is still on its earlier turn. Your words wait until that turn ends; what it writes before then is not taken as T-{number}'s result (cf task get T-{number} --transcript shows it). To stop it now: cf task cancel T-{number}."
                    ),
                },
            )?;
        }
        Ok(())
    }

    /// The keys that interrupt the window's turn are pressed into its pane.
    async fn press(self: &Rc<Self>, record: &Rc<Record>) {
        let (pane, keys) = {
            let part = record.window.borrow();
            (part.pane.clone(), part.keys)
        };
        if let (Some(pane), Some(keys)) = (pane, keys) {
            press_interrupt(&*self.seams.host, &*self.seams.time, &pane, keys).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: StopKey = StopKey::Pause { task: 4, seq: 1 };

    fn done(rounds: u32, at: i64, exhausted: bool) -> Interrupted {
        Interrupted {
            key: KEY,
            rounds,
            at,
            exhausted,
        }
    }

    #[test]
    fn the_first_round_is_at_once_and_each_next_one_three_seconds_after() {
        assert_eq!(next_round(None, KEY, 0), Round::Press);
        assert_eq!(
            next_round(Some(&done(1, 0, false)), KEY, 2_999),
            Round::Wait
        );
        assert_eq!(
            next_round(Some(&done(1, 0, false)), KEY, 3_000),
            Round::Press
        );
        assert_eq!(
            next_round(Some(&done(2, 0, false)), KEY, 3_000),
            Round::Press
        );
    }

    #[test]
    fn three_rounds_ignored_exhaust_the_stop_of_a_pause_and_a_cancelled_turn_just_stops() {
        assert_eq!(
            next_round(Some(&done(3, 0, false)), KEY, 2_999),
            Round::Wait
        );
        assert_eq!(
            next_round(Some(&done(3, 0, false)), KEY, 3_000),
            Round::Exhaust
        );
        let cancelled = StopKey::Cancelled { task: 4 };
        let last = Interrupted {
            key: cancelled,
            ..done(3, 0, false)
        };
        assert_eq!(next_round(Some(&last), cancelled, 600_000), Round::Wait);
    }

    #[test]
    fn an_exhausted_stop_is_tried_again_every_minute_with_no_ceiling() {
        assert_eq!(
            next_round(Some(&done(3, 0, true)), KEY, 59_999),
            Round::Wait
        );
        assert_eq!(
            next_round(Some(&done(3, 0, true)), KEY, 60_000),
            Round::Press
        );
        assert_eq!(
            next_round(Some(&done(40, 0, true)), KEY, 60_000),
            Round::Press
        );
    }

    #[test]
    fn another_stop_starts_its_rounds_afresh() {
        let later = StopKey::Pause { task: 4, seq: 2 };
        assert_eq!(next_round(Some(&done(3, 0, true)), later, 1), Round::Press);
    }
}
