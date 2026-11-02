//! A window whose task stopped stops too. A pause asks a stop of its task's
//! window (the ledger counts them: a stop is a task and its sequence), and a
//! stop is owed until the window has paid it, in the engine's own record of
//! that window, which dies with it. It is paid when the window is seen at
//! rest: its turn is over, nothing is asked of it and nothing is being
//! pasted into it; no key is pressed into a window at rest, and a window
//! that was opened after the pause owes nothing for it. A turn is over where
//! its harness's record says so, whatever the window's live status says
//! (`Observed::over`: Claude says busy while a background agent runs), and
//! where a refusal for quota ended it: such a window was never interrupted,
//! and never closed for keys nothing in it could heed, which ended what it
//! left running in the background (T-502's window, held for quota, and
//! T-604's, told mid-turn, on 2026-10-25). Nor is a window whose record shows a
//! message of the user's after the one the turn its first round was pressed into
//! began with: that turn is over (Claude writes its own message of the interrupt,
//! and begins a turn by itself for a notice of its background agents, both before
//! a record's word that the turn is over has stood its moment, T-807), and what is
//! at work there now is no turn the stop was asked of. While the window is
//! at work, read from what its harness's record shows and never from what
//! the board says, it is interrupted: Escape, as many presses as its harness
//! asks for, again a few seconds later, three rounds in all. A window that
//! ignores them all has not stopped.
//!
//! **A pause never ends a window's work** (T-1117: a worker's database tests in WSL
//! were ended by the close of a window that ignored its pause). So a window that
//! ignored every round is closed only where its close would end nothing it started:
//! the host is asked what the window's turn runs (`pane.background`), and its
//! harness's record what jobs and agents it runs ([`close_would_end`]). One that runs
//! anything, or of which the host could not say, is kept open, its task paused and none
//! of its work ended; whoever gave the task and the human are told once what runs, and
//! how to end it (only the human can, by deleting its session). It is asked again every
//! ten seconds, and pays the stop at rest if its turn ends meanwhile (its tool call
//! returned). Once its work has ended and its turn is still at work, three fresh rounds
//! are pressed, the turn's own safe point. A window that ignores every round with
//! nothing of its own running has nothing to keep it open (a harness hung in a model
//! request: the zeewin long run's Pi of 2026-10-19, whose resume waited 1 h 48 min with
//! the board behind it), and ConsensFlow closes it, which ends its process as any close
//! does; the task keeps its state, paused or queued behind the words that resume it, and
//! resumed it reopens on its conversation. Whoever gave the task is told why, in one note
//! that says what was checked. A window the host would not close is shown on the board,
//! it is told that once, and the close is tried again once a minute while it works; it
//! pays the stop if its turn ends meanwhile. An agent still at work on a turn about a task cancelled under it
//! is interrupted the same way, three rounds at most, and never closed for it.
//! A harness that writes no record of a turn it was interrupted in (Claude,
//! before a word of its answer) is told of each round whose keys the host took
//! ([`cf_harness::contract::Window::interrupted`]), and reads the looks that
//! follow by it.

use std::rc::Rc;
use std::sync::Arc;

use cf_harness::contract::{Observed, Over};
use cf_harness::records::Role;
use cf_ledger::{NewNote, ParticipantView, ProjectView, Stop};

use crate::background::close_would_end;
use crate::delivery_text::marker_of;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::windows::press_interrupt;

/// How many rounds of Escape a stop gets before its window is closed, and how
/// soon one round follows another.
const ROUNDS: u32 = 3;
const ROUND_EVERY_MS: i64 = 3_000;
/// How often the close of a window that ignored every round, which the host
/// refused, is tried again.
const KEPT_EVERY_MS: i64 = 60_000;
/// How often a window kept open for its work is asked again what it runs.
const RECHECK_EVERY_MS: i64 = 10_000;

/// Why a window that ignored every round of a stop is still open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kept {
    /// It is not: its rounds go on, or are over.
    No,
    /// Closing it would end work it runs: it is kept for that work.
    ForWork,
    /// The host refused to close it.
    Refused,
}

/// What a window was interrupted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopKey {
    /// A pause of its task: the task and the sequence of the stop it asked.
    Pause { task: i64, seq: i64 },
    /// A turn about a task that was cancelled, which the agent has not left.
    Cancelled { task: i64 },
}

/// The rounds one stop has had: how many were pressed, when the last was
/// (or when its window was last kept open), whether and why the window ignored
/// them all and is kept open, whether whoever was to be told was told so, and the
/// turn its first round was pressed into, by the message of the user's that began
/// it ([`last_message`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Interrupted {
    key: StopKey,
    rounds: u32,
    at: i64,
    kept: Kept,
    /// Told that the window is kept for its work: said once for the stop, however
    /// often it is kept.
    told_work: bool,
    turn: Option<Arc<str>>,
}

/// A stop a window ignored in every round, and still has not paid, the host
/// having refused to close it: the number of its task, and how many times it
/// has been ignored.
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
    /// Every round was ignored: the window is closed where that ends nothing it
    /// started, and kept open for its work otherwise; no key is pressed.
    Close,
    Wait,
}

/// What the notes of a window closed for a stop it ignored say became of its
/// task T-`number`, as it is once the close was taken (`state`, none for one
/// deleted): to the human, and to whoever gave it. The close is awaited, and the
/// task may move meanwhile. Only a task still paused waits for its resume, and
/// only one queued for this window (`ours`: resumed while it closed) has words
/// that go in as it reopens; one cancelled, finished or given elsewhere has none.
fn became_of(number: i64, state: Option<&str>, ours: bool) -> (String, String) {
    let both = |said: String| (said.clone(), said);
    match state {
        Some("paused") => both(format!(
            "T-{number} stays paused, and its window reopens on its conversation when it is resumed."
        )),
        Some("queued") if ours => (
            format!("The words that resume T-{number} go in once its window reopens on its conversation."),
            "Your words go in once it reopens on its conversation.".to_owned(),
        ),
        Some("cancelled") => both(format!("T-{number} was cancelled.")),
        Some("failed") => both(format!("T-{number} failed.")),
        Some("accepted") => both(format!("T-{number} was accepted.")),
        Some(state) => both(format!("T-{number} is {state}.")),
        None => both(format!("T-{number} was deleted.")),
    }
}

/// The cadence: the first press of a stop is at once; a round follows three
/// seconds after the last, up to three; three seconds after the third, the
/// window of a pause's stop is closed or kept for its work; one kept for its work
/// is looked at again every ten seconds, and where the host refused the close, it
/// is tried again once a minute; a cancelled task's turn gets its three rounds and
/// no more.
fn next_round(done: Option<&Interrupted>, key: StopKey, now: i64) -> Round {
    let Some(done) = done.filter(|done| done.key == key) else {
        return Round::Press;
    };
    let since = now - done.at;
    let every = match done.kept {
        Kept::No => None,
        Kept::ForWork => Some(RECHECK_EVERY_MS),
        Kept::Refused => Some(KEPT_EVERY_MS),
    };
    if let Some(every) = every {
        return if since >= every {
            Round::Close
        } else {
            Round::Wait
        };
    }
    if since < ROUND_EVERY_MS {
        Round::Wait
    } else if done.rounds < ROUNDS {
        Round::Press
    } else if matches!(key, StopKey::Pause { .. }) {
        Round::Close
    } else {
        Round::Wait
    }
}

/// What the note of a window closed for a stop it ignored says was checked first.
const CHECKED: &str = "First ConsensFlow checked what it ran: the host listed no work of its turn, and its harness no job or agent, so the close ended none.";

/// What a pause's `stop` is pressed for.
fn pause_key(stop: &Stop) -> StopKey {
    StopKey::Pause {
        task: stop.task_id,
        seq: stop.seq,
    }
}

/// The message of the user's the look's record shows last, by its id: what began the
/// turn at work, or what came after it. A compaction's summary, which the harness writes
/// in the user's place within a turn, is none.
fn last_message(observed: &Observed) -> Option<Arc<str>> {
    observed
        .items()
        .iter()
        .rev()
        .find(|item| item.role == Role::User && !item.compaction)
        .map(|item| Arc::clone(&item.id))
}

/// Whether the window is past the turn the first round of `stop` was pressed into:
/// its record shows a message of the user's after the one that turn began with. The
/// turn is over, whatever its status says: Claude writes the interrupt as a message of
/// its own, and begins a turn by itself for a notice of its background agents, a second
/// after the Escape and before its record's word that the turn is over has stood
/// (T-807); a message the human typed began one too. None of those is a turn the stop
/// was asked of, and a key pressed into them, or a close, ends what the window does on
/// its own (T-604's agents).
fn past_its_turn(record: &Record, stop: &Stop, observed: &Observed) -> bool {
    let key = pause_key(stop);
    let pressed = record
        .window
        .borrow()
        .interrupted
        .as_ref()
        .filter(|done| done.key == key)
        .and_then(|done| done.turn.clone());
    pressed.is_some_and(|pressed| last_message(observed).is_some_and(|now| now != pressed))
}

impl Dispatcher {
    /// A stop that a window ignored in every round, whose close the host
    /// refused: none while it is paid, was never asked, or closed its window.
    pub fn unstopped(&self, participant: i64) -> Option<Unstopped> {
        self.record(participant)
            .and_then(|record| record.window.borrow().unstopped)
    }

    /// One look at a member's window, as far as the stops of its task go. At
    /// rest, and while the window shows a dialog of its own, what a door
    /// claimed and nobody acknowledged is the ledger's again (a door that
    /// handed its answer over has no dialog open: it gave up, or it is gone,
    /// and nothing else would free the claim); a stop owed is paid at rest
    /// with no key; at work, it is interrupted by the cadence. `starting` is
    /// whether the look found the window still drawing its screen or
    /// unnamed: it is neither.
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
        } else if observed.settled || observed.over == Over::Held || self.cut_short(observed) {
            // A turn its record says is over, whatever the window's status says, and one
            // a refusal for quota ended, have nothing left for an interrupt to stop: no
            // key, and no close for keys nothing heeded (T-502's and T-604's windows,
            // closed with their background work on 2026-10-25).
            Look::Rest
        } else if observed.over == Over::Pending {
            // The record's word has not stood a moment yet: a turn may be beginning, or
            // the one the last key stopped is over. The next look tells.
            Look::Neither
        } else {
            Look::Work
        };
        let unclaim = match look {
            Look::Rest => Some("its window is at rest"),
            _ if observed.waiting.is_some() => Some("its window shows a dialog of its own"),
            _ => None,
        };
        if let Some(because) = unclaim {
            self.seams
                .ledger
                .borrow_mut()
                .release_claims(participant.id, because)?;
        }
        // A take-back the record writes a moment after the turn's end, read once the stop
        // was paid between the two, is a take-back still (T-1056).
        if look == Look::Rest && observed.took_back {
            self.taken_back_since_paid(participant, record, observed)?;
        }
        let stop = self.seams.ledger.borrow().stop_of(participant.id)?;
        if let Some(stop) = stop.filter(|stop| self.owes(record, stop)) {
            let look = if look == Look::Work && past_its_turn(record, &stop, observed) {
                Look::Rest
            } else {
                look
            };
            return match look {
                Look::Rest => {
                    // Not taken back yet: the record may say so at the next look.
                    record.window.borrow_mut().paid_untaken = (!observed.took_back).then_some(stop);
                    // What the ledger is told is written first: a failure leaves
                    // the stop owed, and the next look at rest tells it again.
                    if observed.took_back {
                        self.take_back(participant, &stop, observed)?;
                    }
                    self.pay(record, &stop);
                    // The window stays: what its turn left runs on, named and never ended.
                    self.tell_paused_left_running(project, record, stop.number)
                        .await
                }
                Look::Work => {
                    self.interrupt_for(project, participant, record, &stop, observed)
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

    /// The look at rest reads the window's turn taken back (`Observed::took_back`) after a
    /// look paid the stop it was pressed for with the turn not yet taken back: Muse writes
    /// `run_retracted` 5 to 11 ms after the `terminal cancelled` of a turn stopped before a
    /// word, a line of its own, and a look between the two read the turn over and nothing
    /// taken back (T-995's A/B: `muse_window`'s tell case red 2 runs in 5 on main). Its
    /// brief stayed delivered while Muse put it back in its box, which the next paste
    /// empties: the resume never gave it again. So it is taken back now, by the rule the
    /// paying look's is ([`Dispatcher::take_back`]): the record's last message of the
    /// user's is the task's message still delivered, so one taken back already, or a turn
    /// some other message began, takes nothing back.
    fn taken_back_since_paid(
        &self,
        participant: &ParticipantView,
        record: &Record,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let paid = record.window.borrow_mut().paid_untaken.take();
        match paid {
            Some(stop) => self.take_back(participant, &stop, observed),
            None => Ok(()),
        }
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
    /// the cadence says so; or, every round ignored, the window is closed.
    async fn interrupt_for(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        stop: &Stop,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let key = pause_key(stop);
        let done = record.window.borrow().interrupted.clone();
        match next_round(done.as_ref(), key, self.now()) {
            Round::Wait => Ok(()),
            Round::Press => {
                self.press_round(record, key, observed);
                self.press(record).await;
                Ok(())
            }
            Round::Close => {
                let rounds = done.as_ref().map_or(0, |done| done.rounds);
                let kept = done.as_ref().map_or(Kept::No, |done| done.kept);
                let told_work = done.as_ref().is_some_and(|done| done.told_work);
                if let Some(runs) = self.close_would_end(record, observed).await {
                    self.keep(record, key, stop.number, rounds, Kept::ForWork);
                    if told_work {
                        return Ok(());
                    }
                    return self.say_kept_for_work(project, participant, stop, rounds, &runs);
                }
                if kept == Kept::ForWork {
                    // What it was kept for has ended, and its turn is still at work: the turn
                    // may take a key now (its tool call returned). Three fresh rounds.
                    self.fresh_rounds(record, key, observed);
                    self.press(record).await;
                    return Ok(());
                }
                if self.retire(record).await? {
                    return self.say_closed(project, participant, stop, rounds);
                }
                self.keep(record, key, stop.number, rounds, Kept::Refused);
                if kept == Kept::Refused {
                    return Ok(());
                }
                self.say_kept(project, participant, stop, rounds)
            }
        }
    }

    /// The window is at work and the task it was last given was cancelled:
    /// when its turn is the one about it, three rounds of Escape, and no more.
    /// A task queued for the window since (what waited on the board for the
    /// session, released by the cancel) is not given yet: it waits for the
    /// window to be at rest.
    async fn interrupt_cancelled(
        self: &Rc<Self>,
        participant: &ParticipantView,
        record: &Rc<Record>,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let cancelled = self.seams.ledger.borrow().task_given(participant.id)?;
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
            self.press_round(record, key, observed);
            self.press(record).await;
        }
        Ok(())
    }

    /// A round is pressed into the turn `observed` shows: counted. A stop that
    /// begins shows none of what the one before it ignored, and keeps the turn
    /// its first round is pressed into.
    fn press_round(&self, record: &Record, key: StopKey, observed: &Observed) {
        let now = self.now();
        let changed = {
            let mut part = record.window.borrow_mut();
            let (rounds, turn) = match part.interrupted.as_ref().filter(|done| done.key == key) {
                Some(done) => (done.rounds, done.turn.clone()),
                None => (0, last_message(observed)),
            };
            let told_work = part
                .interrupted
                .as_ref()
                .is_some_and(|done| done.key == key && done.told_work);
            part.interrupted = Some(Interrupted {
                key,
                rounds: rounds + 1,
                at: now,
                kept: Kept::No,
                told_work,
                turn,
            });
            part.unstopped.take().is_some()
        };
        if changed {
            self.changed();
        }
    }

    /// The work a window was kept open for has ended: its stop's rounds begin again, the
    /// first pressed now, into the same turn, and what was said of it stays said.
    fn fresh_rounds(&self, record: &Record, key: StopKey, observed: &Observed) {
        if let Some(done) = record.window.borrow_mut().interrupted.as_mut() {
            done.rounds = 0;
            done.kept = Kept::No;
        }
        self.press_round(record, key, observed);
    }

    /// What closing the window would end of what it started, as a note says it: what the
    /// host lists of its turn's work, and what its harness's record says it runs
    /// ([`close_would_end`], which says what neither can see); none where it would end
    /// nothing.
    async fn close_would_end(&self, record: &Record, observed: &Observed) -> Option<String> {
        let pane = record.window.borrow().pane.clone();
        let left = match pane {
            Some(pane) => self.seams.background.left_running(&pane).await,
            None => Some(Default::default()),
        };
        close_would_end(left.as_ref(), observed.pending())
    }

    /// Every round of the stop was ignored, and the window is kept open, for the
    /// work it runs or because the host would not close it: it is marked, and the
    /// board shows it, until it is looked at again.
    fn keep(&self, record: &Record, key: StopKey, number: i64, rounds: u32, kept: Kept) {
        {
            let mut part = record.window.borrow_mut();
            let (turn, told_work) = part
                .interrupted
                .as_ref()
                .map_or((None, false), |done| (done.turn.clone(), done.told_work));
            part.interrupted = Some(Interrupted {
                key,
                rounds,
                at: self.now(),
                kept,
                told_work: told_work || kept == Kept::ForWork,
                turn,
            });
            part.unstopped = Some(Unstopped {
                task: number,
                rounds,
            });
        }
        self.changed();
    }

    /// Whoever gave the task, and whoever asked the stop, are told that the window
    /// did not stop and was closed, and what became of the task once the close was
    /// taken ([`became_of`]).
    fn say_closed(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        stop: &Stop,
        rounds: u32,
    ) -> Result<(), EngineError> {
        let (handle, number) = (&participant.handle, stop.number);
        let task = self
            .seams
            .ledger
            .borrow()
            .task(project.id, number)?
            .map(|thread| thread.task);
        let (human, words) = became_of(
            number,
            task.as_ref().map(|task| task.state.as_str()),
            task.as_ref()
                .is_some_and(|task| task.assignee.as_ref() == Some(handle)),
        );
        self.tell_stop(
            project,
            stop,
            task.map(|task| task.requester),
            format!(
                "@{handle} did not stop for T-{number}: it ignored the interrupt {rounds} times, so ConsensFlow closed its window. {CHECKED} {human}"
            ),
            format!(
                "T-{number}'s window (@{handle}) did not stop: it ignored the interrupt, so ConsensFlow closed it. {CHECKED} {words}"
            ),
            false,
        )
    }

    /// Whoever gave the task, whoever asked the stop and the human are told once that the
    /// window did not stop, that it is kept open for the work it runs (`runs`), which
    /// nothing ended, and how to end it.
    fn say_kept_for_work(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        stop: &Stop,
        rounds: u32,
        runs: &str,
    ) -> Result<(), EngineError> {
        let (handle, number) = (&participant.handle, stop.number);
        let requester = self
            .seams
            .ledger
            .borrow()
            .task(project.id, number)?
            .map(|thread| thread.task.requester);
        self.tell_stop(
            project,
            stop,
            requester,
            format!(
                "@{handle} did not stop for T-{number}: its turn went on through the interrupt {rounds} times, and its window runs work that closing it would end: {runs}. ConsensFlow keeps the window open for that work and T-{number} paused, and ends none of it; once the work has ended, it interrupts the turn again. To end the work now, delete @{handle}'s session."
            ),
            format!(
                "T-{number}'s window (@{handle}) did not stop: its turn went on through the interrupt, and it runs work that closing it would end: {runs}. ConsensFlow keeps the window open for it and T-{number} paused, ends none of it, and interrupts the turn again once that work has ended; until then what it writes is not taken as T-{number}'s result. `cf task resume T-{number} \"…\"` gives the window your words once its turn ends; only the human can end the work sooner, by deleting its session."
            ),
            true,
        )
    }

    /// Whoever gave the task, and whoever asked the stop, are told once that the
    /// window did not stop and would not close, and what that means.
    fn say_kept(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        stop: &Stop,
        rounds: u32,
    ) -> Result<(), EngineError> {
        let (handle, number) = (&participant.handle, stop.number);
        let requester = self
            .seams
            .ledger
            .borrow()
            .task(project.id, number)?
            .map(|thread| thread.task.requester);
        self.tell_stop(
            project,
            stop,
            requester,
            format!(
                "@{handle} did not stop for T-{number}: it ignored the interrupt {rounds} times, and its window would not close. ConsensFlow tries to close it again every minute; until then what is for it waits, and what it writes is not T-{number}'s result."
            ),
            format!(
                "T-{number}'s window (@{handle}) did not stop: it ignored the interrupt, and would not close. ConsensFlow tries again every minute; until then your words wait, and what it writes is not taken as T-{number}'s result (cf task get T-{number} --transcript shows it)."
            ),
            false,
        )
    }

    /// A note of `stop`'s end to each who needs it, once each: whoever gave the
    /// task, `requester`, else the human; whoever asked the stop, where that
    /// is someone else (the human's pause or the chief's tell of a task the other
    /// gave): their own act closed the window and ended its work; and the human,
    /// where `board` (a window kept for its work, which only the human can end). The
    /// human reads `human`, any other `to_requester`. Two notes to one reader of one
    /// event said it twice (T-502's, 2026-10-25).
    fn tell_stop(
        &self,
        project: &ProjectView,
        stop: &Stop,
        requester: Option<String>,
        human: String,
        to_requester: String,
        board: bool,
    ) -> Result<(), EngineError> {
        let asked_by =
            self.seams
                .ledger
                .borrow()
                .stop_asked_by(project.id, stop.number, stop.seq)?;
        let mut readers = vec![requester.unwrap_or_else(|| "human".to_owned())];
        readers.extend(asked_by.filter(|by| !readers.contains(by)));
        if board && !readers.iter().any(|reader| reader == "human") {
            readers.push("human".to_owned());
        }
        let mut ledger = self.seams.ledger.borrow_mut();
        for to in readers {
            let body = if to == "human" {
                human.clone()
            } else {
                to_requester.clone()
            };
            ledger.note(
                project.id,
                &NewNote {
                    from: None,
                    to,
                    task: Some(stop.number),
                    body,
                },
            )?;
        }
        Ok(())
    }

    /// The keys that interrupt the window's turn are pressed into its pane,
    /// and its harness is told once the host took them: what the keys do is
    /// read from the looks that follow, and a harness that writes nothing of
    /// a turn it was interrupted in reads them by the press (Claude, before a
    /// word of its answer). Keys the host refused were no press, and it is
    /// not told.
    async fn press(self: &Rc<Self>, record: &Rc<Record>) {
        let (window, pane, keys) = {
            let part = record.window.borrow();
            (part.window.clone(), part.pane.clone(), part.keys)
        };
        if let (Some(window), Some(pane), Some(keys)) = (window, pane, keys) {
            if press_interrupt(&*self.seams.host, &*self.seams.time, &pane, keys).await {
                window.interrupted();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: StopKey = StopKey::Pause { task: 4, seq: 1 };

    fn done(rounds: u32, at: i64, kept: Kept) -> Interrupted {
        Interrupted {
            key: KEY,
            rounds,
            at,
            kept,
            told_work: false,
            turn: None,
        }
    }

    #[test]
    fn the_first_round_is_at_once_and_each_next_one_three_seconds_after() {
        assert_eq!(next_round(None, KEY, 0), Round::Press);
        assert_eq!(
            next_round(Some(&done(1, 0, Kept::No)), KEY, 2_999),
            Round::Wait
        );
        assert_eq!(
            next_round(Some(&done(1, 0, Kept::No)), KEY, 3_000),
            Round::Press
        );
        assert_eq!(
            next_round(Some(&done(2, 0, Kept::No)), KEY, 3_000),
            Round::Press
        );
    }

    #[test]
    fn three_rounds_ignored_close_the_window_of_a_pause_and_a_cancelled_turn_just_stops() {
        assert_eq!(
            next_round(Some(&done(2, 0, Kept::No)), KEY, 3_000),
            Round::Press,
            "two rounds ignored are no reason to close it"
        );
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::No)), KEY, 2_999),
            Round::Wait
        );
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::No)), KEY, 3_000),
            Round::Close
        );
        let cancelled = StopKey::Cancelled { task: 4 };
        let last = Interrupted {
            key: cancelled,
            ..done(3, 0, Kept::No)
        };
        assert_eq!(next_round(Some(&last), cancelled, 600_000), Round::Wait);
    }

    #[test]
    fn a_close_the_host_refused_is_tried_again_every_minute_with_no_key_and_no_ceiling() {
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::Refused)), KEY, 59_999),
            Round::Wait
        );
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::Refused)), KEY, 60_000),
            Round::Close
        );
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::Refused)), KEY, 40 * 60_000),
            Round::Close
        );
    }

    #[test]
    fn a_closed_windows_notes_promise_words_only_for_a_task_queued_for_it_and_say_how_any_other_ended(
    ) {
        let said = |state, ours| became_of(1, state, ours);
        let both = |text: &str| (text.to_owned(), text.to_owned());
        assert_eq!(
            said(Some("paused"), true),
            both(
                "T-1 stays paused, and its window reopens on its conversation when it is resumed."
            )
        );
        assert_eq!(
            said(Some("queued"), true),
            (
                "The words that resume T-1 go in once its window reopens on its conversation."
                    .to_owned(),
                "Your words go in once it reopens on its conversation.".to_owned()
            )
        );
        for (state, ours, text) in [
            (Some("queued"), false, "T-1 is queued."),
            (Some("cancelled"), true, "T-1 was cancelled."),
            (Some("failed"), true, "T-1 failed."),
            (Some("done"), true, "T-1 is done."),
            (Some("accepted"), true, "T-1 was accepted."),
            (Some("open"), false, "T-1 is open."),
            (None, false, "T-1 was deleted."),
        ] {
            let (human, requester) = said(state, ours);
            assert_eq!(
                (human.as_str(), requester.as_str()),
                (text, text),
                "{state:?}"
            );
            assert!(!human.contains("go in"), "{state:?}");
        }
    }

    #[test]
    fn a_window_kept_for_its_work_is_looked_at_again_every_ten_seconds_with_no_key() {
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::ForWork)), KEY, 9_999),
            Round::Wait
        );
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::ForWork)), KEY, 10_000),
            Round::Close
        );
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::ForWork)), KEY, 40 * 60_000),
            Round::Close,
            "with no ceiling: the work may run for hours"
        );
    }

    #[test]
    fn another_stop_starts_its_rounds_afresh() {
        let later = StopKey::Pause { task: 4, seq: 2 };
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::Refused)), later, 1),
            Round::Press
        );
    }
}
