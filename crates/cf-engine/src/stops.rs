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
//! **A pause never closes a window** (the owner's rule of 2026-11-03: "pause should never
//! close the window"; T-1117 before it: a worker's database tests in WSL were ended by the
//! close of a window that ignored its pause). A window that ignored every round is kept
//! open, its task paused and nothing of it ended, and shown on the board. What it does is
//! said as far as ConsensFlow can tell: the host is asked what the window's turn runs
//! (`pane.background`), and its harness's record what jobs and agents it runs
//! ([`close_would_end`]). Whoever gave the task and the human are told once that it did
//! not stop, what it runs, or that ConsensFlow can see nothing it runs (a harness hung in
//! a model request: the zeewin long run's Pi of 2026-10-19), and that only the human can
//! end it, by deleting its session. One that runs work is asked again every ten seconds,
//! and once its work has ended and its turn is still at work, three fresh rounds are
//! pressed, the turn's own safe point; one that runs nothing ConsensFlow can see is asked
//! again every minute, three fresh rounds each time. Either pays the stop at rest when its
//! turn ends, and what waits for it goes in then. One exception, which is no pause's: the
//! stop ConsensFlow asked of a window that began no turn on a message (`never_began`), which
//! that window ignores running nothing, relaunches it with that very message again, and the
//! task goes on by itself (T-1130): nothing of a message never begun can be lost. The
//! relaunch is bound to that message and that window, asked again before the window is
//! retired and after every look the host or the record is asked for (a word of an answer
//! since, the human's unsent words, a dialog, a screen or a record unread, a delivery moved
//! on: it is kept instead), resumes that stall alone, is told as it happened (on its
//! conversation, or afresh), and is tried once for a message (T-1220). A tell or a pause
//! takes a stalled or a held task over, and asks its stop (`cf_ledger`'s
//! `paused_by_consensflow`). A stop the task was asked of its window, owed still when the
//! chief's Finish makes it done, is settled there: a reopen after presses nothing for it,
//! whatever look comes between (T-1220's P2-4). Only a pause asks a stop: the chief's or
//! the human's, a tell's, ConsensFlow's own of a window that began nothing, or a project's
//! Close; a hold for quota and a resume whose words never went in ask none (`cf_ledger`'s
//! `pause`), and a reopen, a resume, a follow-up and an answer are words that wait for the
//! turn to end, as any message does. An agent still at work on a turn about a task
//! cancelled under it is interrupted the same way, three rounds at most, and never closed
//! for it.
//! A harness that writes no record of a turn it was interrupted in (Claude,
//! before a word of its answer) is told of each round whose keys the host took
//! ([`cf_harness::contract::Window::interrupted`]), and reads the looks that
//! follow by it.

use std::rc::Rc;
use std::sync::Arc;

use cf_base::js;
use cf_harness::contract::{Observed, Over};
use cf_harness::records::{Reading, Role};
use cf_ledger::{LedgerError, NewNote, ParticipantView, ProjectView, Stop};
use serde_json::Value;

use crate::background::close_would_end;
use crate::delivery_text::marker_of;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::EngineError;
use crate::windows::press_interrupt;

/// How many rounds of Escape a stop gets before its window is kept as one that did not
/// stop, and how soon one round follows another.
const ROUNDS: u32 = 3;
const ROUND_EVERY_MS: i64 = 3_000;
/// How often a window that ignored every round, running nothing ConsensFlow can see, is
/// asked again to stop: three fresh rounds.
const KEPT_EVERY_MS: i64 = 60_000;
/// Whether the host's look at a window's screen, the last before its kill, shows it untouched
/// for longer than `span` ms (since the record's fresh read was asked): no paste on its way,
/// none of the human's words unsent, no key of the human's, nothing printed. A host that says
/// nothing of what the window printed gives no such evidence (T-1223's P1-1).
fn untouched_since(snapshot: &Value, span: i64) -> bool {
    if js::truthy(snapshot.get("unsent")) || js::truthy(snapshot.get("pasteInFlight")) {
        return false;
    }
    let span = span.max(0) as f64;
    let quiet = snapshot.get("outputQuietMs").and_then(Value::as_f64);
    if !quiet.is_some_and(|quiet| quiet > span) {
        return false;
    }
    let keys = snapshot.get("humanWrites").and_then(Value::as_array);
    !keys.is_some_and(|keys| {
        keys.iter().any(|key| {
            key.get("agoMs")
                .and_then(Value::as_f64)
                .is_none_or(|ago| ago <= span)
        })
    })
}

/// How often a window kept running its work is asked again what it runs.
const RECHECK_EVERY_MS: i64 = 10_000;

/// How a window that ignored every round of a stop is kept: no window is closed for a
/// stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kept {
    /// It is not: its rounds go on, or are over.
    No,
    /// It runs work, which it is kept running.
    ForWork,
    /// It runs nothing ConsensFlow can see: its harness may be hung.
    Ignored,
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
/// (or when its window was last kept), whether and how the window ignored
/// them all and is kept, whether whoever was to be told was told so, and the
/// turn its first round was pressed into, by the message of the user's that began
/// it ([`last_message`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Interrupted {
    key: StopKey,
    rounds: u32,
    at: i64,
    kept: Kept,
    /// Told that the window did not stop and is kept: said once for the stop, however
    /// often it is kept.
    told: bool,
    turn: Option<Arc<str>>,
}

/// A stop a window ignored in every round, and still has not paid, the window
/// kept: the number of its task, and how many times it has been ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unstopped {
    pub task: i64,
    pub rounds: u32,
}

/// The stall ConsensFlow's own pause made of a window for a message it began no turn on
/// (`never_began`): the stop it asked (its task, the task's number and the stop's
/// sequence), the message by its id and by its item in the window's record, and the
/// window's generation. A relaunch is this stall's alone (T-1220's P1-2, P1-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stalled {
    pub(crate) task_id: i64,
    pub(crate) number: i64,
    pub(crate) seq: i64,
    pub(crate) message: i64,
    pub(crate) item: Arc<str>,
    pub(crate) generation: u64,
}

/// A relaunch done, to be told as the window comes back: the task's number, the message
/// given again, whoever gave the task, and how many rounds the window ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Relaunching {
    pub(crate) number: i64,
    pub(crate) message: i64,
    pub(crate) requester: String,
    pub(crate) rounds: u32,
}

/// What came of a stall's relaunch.
enum Relaunch {
    /// The window was retired and the stall dealt with, as its note says.
    Done,
    /// Nothing done: the window is kept as any window that ignores a pause.
    Not,
    /// Nothing done, its message relaunched once already: kept, and the note says so.
    Once,
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
    /// Every round was ignored, or a window kept is due a look: what it runs is asked,
    /// and it is kept, or given fresh rounds; no key is pressed at this look.
    Keep,
    Wait,
}

/// What the human and whoever gave the task read of a window kept for a stop it ignored
/// `rounds` times: @`handle`'s, on T-`number`, which runs `runs` (none: nothing ConsensFlow
/// can see), the task `state` as they are told (none: deleted). The task is said paused only
/// while it is; one resumed meanwhile has words that go in once the turn ends, and one
/// called off is said so; one in another window's hands now (`ours` false) is that one's,
/// and nothing of it is promised. Nothing is closed, nothing of it is ended, and only the
/// human ends it sooner. A window whose message `again` began nothing after ConsensFlow
/// relaunched it once is relaunched no more (T-1220's P3-7), and the notes say so.
fn kept_words(
    handle: &str,
    number: i64,
    rounds: u32,
    runs: Option<&str>,
    state: Option<&str>,
    ours: bool,
    again: Option<i64>,
) -> (String, String) {
    let state = match state {
        Some(_) if !ours => Some("elsewhere"),
        other => other,
    };
    let paused = state == Some("paused");
    let task = match state {
        Some("paused") => format!(" and T-{number} paused"),
        _ => String::new(),
    };
    let after = match state {
        Some("elsewhere") => format!(" T-{number} is in another window's hands now;"),
        Some("paused") => format!(
            " `cf task resume T-{number} \"…\"` gives the window your words once its turn ends;"
        ),
        Some("queued" | "working" | "waiting") => {
            " Your words go in once its turn ends;".to_owned()
        }
        Some("cancelled") => format!(" T-{number} was cancelled;"),
        Some(state) => format!(" T-{number} is {state};"),
        None => format!(" T-{number} was deleted;"),
    };
    let held = matches!(state, Some("queued" | "working" | "waiting"));
    let result = if paused || held {
        format!(" until then what it writes is not taken as T-{number}'s result.")
    } else {
        String::new()
    };
    // A session that holds work the human must pause first: its delete says so too.
    let end = if held {
        format!("pause T-{number} and delete @{handle}'s session")
    } else {
        format!("delete @{handle}'s session")
    };
    let (human, to_requester) = match runs {
        Some(runs) => (
            format!(
                "@{handle} did not stop for T-{number}: its turn went on through the interrupt {rounds} times, and its window runs work that closing it would end: {runs}. ConsensFlow keeps the window open for that work{task}, and ends none of it; once the work has ended, it interrupts the turn again. To end the work now, {end}."
            ),
            format!(
                "T-{number}'s window (@{handle}) did not stop: its turn went on through the interrupt, and it runs work that closing it would end: {runs}. ConsensFlow keeps the window open for it{task}, ends none of it, and interrupts the turn again once that work has ended;{result}{after} only the human can end the work sooner, by deleting its session."
            ),
        ),
        None => (
            format!(
                "@{handle} did not stop for T-{number}: its turn went on through the interrupt {rounds} times, and ConsensFlow can see nothing it runs: the host lists no work of its turn, and its harness no job or agent, so its harness may be hung (in a model request, or a tool it does not name). ConsensFlow keeps the window open{task}, closes nothing, and interrupts the turn again every minute. To end it now, {end}."
            ),
            format!(
                "T-{number}'s window (@{handle}) did not stop: its turn went on through the interrupt, and ConsensFlow can see nothing it runs (the host lists no work of its turn, its harness no job or agent): its harness may be hung. ConsensFlow keeps the window open{task}, closes nothing, and interrupts the turn again every minute;{result}{after} only the human can end it sooner, by deleting its session."
            ),
        ),
    };
    match again {
        Some(message) => {
            let once = format!(" Its window began no turn on m-{message} again after ConsensFlow relaunched it once, so it is relaunched no more.");
            (format!("{human}{once}"), format!("{to_requester}{once}"))
        }
        None => (human, to_requester),
    }
}

/// The cadence: the first press of a stop is at once; a round follows three
/// seconds after the last, up to three; three seconds after the third, the
/// window of a pause's stop is kept; one kept for its work is looked at again every ten
/// seconds, and one kept running nothing ConsensFlow can see once a minute; a cancelled
/// task's turn gets its three rounds and no more.
fn next_round(done: Option<&Interrupted>, key: StopKey, now: i64) -> Round {
    let Some(done) = done.filter(|done| done.key == key) else {
        return Round::Press;
    };
    let since = now - done.at;
    let every = match done.kept {
        Kept::No => None,
        Kept::ForWork => Some(RECHECK_EVERY_MS),
        Kept::Ignored => Some(KEPT_EVERY_MS),
    };
    if let Some(every) = every {
        return if since >= every {
            Round::Keep
        } else {
            Round::Wait
        };
    }
    if since < ROUND_EVERY_MS {
        Round::Wait
    } else if done.rounds < ROUNDS {
        Round::Press
    } else if matches!(key, StopKey::Pause { .. }) {
        Round::Keep
    } else {
        Round::Wait
    }
}

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
    /// A stop that a window ignored in every round, the window kept: none while it is
    /// paid, was never asked, or its window has gone.
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

    /// The window is at work on a turn the stop is for: a round of Escape if the cadence
    /// says so; or, every round ignored, the window is kept, never closed. What it runs is
    /// asked: one that runs work is kept for it, and gets fresh rounds once it has ended;
    /// one that runs nothing ConsensFlow can see is kept, and gets fresh rounds a minute
    /// on. Whoever is to be told is told once.
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
            Round::Keep => {
                let rounds = done.as_ref().map_or(0, |done| done.rounds);
                let kept = done.as_ref().map_or(Kept::No, |done| done.kept);
                let told = done.as_ref().is_some_and(|done| done.told);
                let runs = self.close_would_end(record, observed).await;
                let mut again = None;
                let now = match (&runs, kept) {
                    (Some(_), _) => Kept::ForWork,
                    // What it was kept for has ended, or a minute went by with nothing it
                    // runs in view, and its turn is still at work: the turn may take a key
                    // now (its tool call returned). Three fresh rounds.
                    (None, Kept::ForWork | Kept::Ignored) => {
                        self.fresh_rounds(record, key, observed);
                        self.press(record).await;
                        return Ok(());
                    }
                    // ConsensFlow's own stop of a message the window never began: nothing of it
                    // can be lost, and the window is relaunched with that message again
                    // (T-1130), where every check of `relaunch_unbegun` holds; else it is
                    // kept as any window that ignores a pause.
                    (None, Kept::No) => {
                        if let Some(stalled) = self.stalled_for(record, stop) {
                            match self
                                .relaunch_unbegun(
                                    project,
                                    participant,
                                    record,
                                    &stalled,
                                    rounds,
                                    observed,
                                )
                                .await?
                            {
                                Relaunch::Done => return Ok(()),
                                Relaunch::Once => again = Some(stalled.message),
                                Relaunch::Not => {}
                            }
                        }
                        Kept::Ignored
                    }
                };
                self.keep(record, key, stop.number, rounds, now);
                if told {
                    return Ok(());
                }
                self.say_kept(project, participant, stop, rounds, runs.as_deref(), again)
            }
        }
    }

    /// The stall `stop` is, where it is the one ConsensFlow's own pause asked of the window
    /// for a message it began no turn on (`never_began`), and not a pause of the chief's, the
    /// human's or a tell's, which ask stops of their own.
    fn stalled_for(&self, record: &Record, stop: &Stop) -> Option<Stalled> {
        record
            .window
            .borrow()
            .stalled
            .clone()
            .filter(|stalled| stalled.task_id == stop.task_id && stalled.seq == stop.seq)
    }

    /// Whether the window is still the stall's, as `observed` shows it: the window of the
    /// stall's generation, nothing being pasted into it, no dialog of its own, its record read
    /// and its own, nothing in it after the message (no word of an answer since, T-1220's
    /// P1-2), no job or agent of its harness's running (T-1223's P1-1); and in the ledger the
    /// task paused at the stall's stop, in its hands, the message its latest.
    fn still_unbegun(
        &self,
        participant: &ParticipantView,
        record: &Record,
        stalled: &Stalled,
        observed: &Observed,
    ) -> Result<bool, EngineError> {
        {
            let part = record.window.borrow();
            if part.retiring
                || part.pane.as_ref().map(|pane| pane.generation) != Some(stalled.generation)
                || part.stalled.as_ref() != Some(stalled)
            {
                return Ok(false);
            }
        }
        if record.delivery.borrow().delivering.is_some()
            || observed.waiting.is_some()
            || observed.failed
            || observed.shared
            || observed.reading.as_deref().is_none_or(Reading::read_failed)
        {
            return Ok(false);
        }
        let pending = observed.pending();
        if pending.jobs > 0 || pending.agents > 0 {
            return Ok(false);
        }
        let items = observed.items();
        let Some(at) = items.iter().rposition(|item| item.id == stalled.item) else {
            return Ok(false);
        };
        if items[at + 1..].iter().any(|item| item.role != Role::Custom) {
            return Ok(false);
        }
        let ledger = self.seams.ledger.borrow();
        let stop = Stop {
            task_id: stalled.task_id,
            number: stalled.number,
            seq: stalled.seq,
        };
        if ledger.stop_of(participant.id)? != Some(stop) {
            return Ok(false);
        }
        let paused = ledger
            .task(participant.project_id, stalled.number)?
            .is_some_and(|thread| {
                thread.task.state == "paused"
                    && thread.task.assignee.as_ref() == Some(&participant.handle)
            });
        let latest = ledger.last_pasted(participant.id, stalled.task_id)?;
        Ok(paused && latest.is_some_and(|message| message.id == stalled.message))
    }

    /// The window that began no turn on a message ignored every round of ConsensFlow's
    /// stop of it, running nothing. It is relaunched only on evidence taken after the last
    /// await before its kill (T-1223's P1-1), and where that message was never relaunched
    /// before (one relaunch for a delivery, T-1220's P3-7): its record read afresh, still the
    /// stall's ([`Dispatcher::still_unbegun`]); nothing it runs that a close would end, as the
    /// host and its harness say after that read; and last the host's look at its screen,
    /// which has printed nothing, taken no key of the human's and no paste, and holds none of
    /// the human's words unsent, since that read was asked ([`untouched_since`]). Between
    /// that look and the kill nothing is awaited, and every check of the window and the
    /// ledger is asked again. Where any of it cannot be had, the window is kept, as any window
    /// that ignores a pause, and the human told so. Then the window is retired, and the
    /// ledger, in one write that asks every condition of the stall again (a newer pause, a
    /// tell, a hold or a resume since, T-1220's P1-3), gives that very message back to the
    /// window's keeping and queues the task for it: the window opened again is given it, as
    /// it was, once (T-1220's P2-5, P2-6), and its requester is told once it has arrived
    /// ([`Dispatcher::tell_relaunched`]). A ledger that refuses leaves the task as it is,
    /// and the requester is told so.
    async fn relaunch_unbegun(
        self: &Rc<Self>,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Rc<Record>,
        stalled: &Stalled,
        rounds: u32,
        observed: &Observed,
    ) -> Result<Relaunch, EngineError> {
        if self
            .seams
            .ledger
            .borrow()
            .was_relaunched(project.id, stalled.message)?
        {
            return Ok(Relaunch::Once);
        }
        if !self.still_unbegun(participant, record, stalled, observed)? {
            return Ok(Relaunch::Not);
        }
        let (pane, window) = {
            let part = record.window.borrow();
            (part.pane.clone(), part.window.clone())
        };
        let (Some(pane), Some(window)) = (pane, window) else {
            return Ok(Relaunch::Not);
        };
        // The record read afresh: the window may have begun since the look.
        let asked = self.now();
        let Ok(fresh) = window.observe().await else {
            return Ok(Relaunch::Not);
        };
        if self.forgotten(record) || !self.still_unbegun(participant, record, stalled, &fresh)? {
            return Ok(Relaunch::Not);
        }
        // What it runs, as the host and its harness say it now.
        if self.close_would_end(record, &fresh).await.is_some() {
            return Ok(Relaunch::Not);
        }
        // Last, the screen: untouched since the fresh read was asked, or kept. Nothing is
        // awaited after it before the kill.
        let snapshot = self.snapshot_now(&pane, false).await;
        let Some(snapshot) = snapshot.filter(Value::is_object) else {
            return Ok(Relaunch::Not);
        };
        if self.forgotten(record)
            || !untouched_since(&snapshot, self.now() - asked)
            || !self.still_unbegun(participant, record, stalled, &fresh)?
        {
            return Ok(Relaunch::Not);
        }
        if !self.retire(record).await? {
            return Ok(Relaunch::Not);
        }
        let handle = &participant.handle;
        let (number, message) = (stalled.number, stalled.message);
        let relaunched = self.seams.ledger.borrow_mut().relaunch_stalled(
            project.id,
            number,
            participant.id,
            message,
            stalled.seq,
        );
        match relaunched {
            Ok(task) => {
                *record.relaunching.borrow_mut() = Some(Relaunching {
                    number,
                    message,
                    requester: task.requester,
                    rounds,
                });
            }
            Err(LedgerError::Refused(refusal)) => {
                let task = self
                    .seams
                    .ledger
                    .borrow()
                    .task(project.id, number)?
                    .map(|thread| thread.task);
                if let Some(task) = task {
                    let now = match task.state.as_str() {
                        "paused" => format!("T-{number} stays paused, and its window opens again on its conversation when it is resumed."),
                        state => format!("T-{number} is {state}."),
                    };
                    self.seams.ledger.borrow_mut().note(
                        project.id,
                        &NewNote {
                            from: None,
                            to: task.requester,
                            task: Some(number),
                            body: format!(
                                "T-{number}'s window (@{handle}) began no turn on m-{message} and went on through the interrupt {rounds} times with nothing of its own running. ConsensFlow was relaunching it, but {}, so it gave the window nothing again. {now}",
                                refusal.message
                            ),
                        },
                    )?;
                }
            }
            Err(failed) => return Err(failed.into()),
        }
        self.changed();
        Ok(Relaunch::Done)
    }

    /// The window relaunched for a message it began no turn on is opened again, `resumed` on
    /// its conversation or afresh where its harness held it no more: its requester is told
    /// once, as it happened.
    pub(crate) fn tell_relaunched(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        resumed: bool,
    ) -> Result<(), EngineError> {
        let Some(relaunching) = record.relaunching.borrow_mut().take() else {
            return Ok(());
        };
        let Relaunching {
            number,
            message,
            requester,
            rounds,
        } = relaunching;
        let how = if resumed {
            "relaunched it on its conversation"
        } else {
            "opened it afresh, as its harness held its conversation no more,"
        };
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: requester,
                task: Some(number),
                body: format!(
                    "T-{number}'s window (@{}) began no turn on m-{message} and went on through the interrupt {rounds} times with nothing of its own running, so ConsensFlow {how} and gave it m-{message} again. T-{number} goes on by itself.",
                    participant.handle
                ),
            },
        )?;
        Ok(())
    }

    /// The task leaves the window's hands at a step that made it so (the chief's Finish or
    /// the human's Done, ConsensFlow's give-up of a failed turn): a stop it was asked before,
    /// still owed, is settled there, whatever look comes next or never comes between it and a
    /// reopen (T-1220's P2-4, T-1223's P2-4). A delivery given up for good fails its task
    /// with no stop owed: told words go in only at rest, after the stop is paid, a brief
    /// fails only a task still queued for it, and the words that resume a task pause it
    /// again. The stop is the one read before that step ([`Dispatcher::owed_at_departure`]),
    /// never whatever is latest later: a stop asked after has a greater sequence, and is owed.
    pub(crate) fn settle_at_departure(&self, record: &Record, stop: Option<&Stop>) {
        if let Some(stop) = stop.filter(|stop| self.owes(record, stop)) {
            self.pay(record, stop);
        }
    }

    /// The stop `participant`'s window was asked for T-`number`, read before a step that may
    /// take the task out of its hands: what [`Dispatcher::settled_at_departure`] settles.
    pub(crate) fn owed_at_departure(
        &self,
        participant: i64,
        number: i64,
    ) -> Result<Option<Stop>, EngineError> {
        Ok(self
            .seams
            .ledger
            .borrow()
            .stop_of(participant)?
            .filter(|stop| stop.number == number))
    }

    /// [`Dispatcher::settle_at_departure`] on `participant`'s window, where it has one.
    pub(crate) fn settled_at_departure(&self, participant: i64, stop: Option<Stop>) {
        if let (Some(record), Some(stop)) = (self.record(participant), stop) {
            self.settle_at_departure(&record, Some(&stop));
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
            let told = part
                .interrupted
                .as_ref()
                .is_some_and(|done| done.key == key && done.told);
            part.interrupted = Some(Interrupted {
                key,
                rounds: rounds + 1,
                at: now,
                kept: Kept::No,
                told,
                turn,
            });
            part.unstopped.take().is_some()
        };
        if changed {
            self.changed();
        }
    }

    /// The work a window was kept for has ended, or a minute went by: its stop's rounds
    /// begin again, the first pressed now, into the same turn, and what was said of it
    /// stays said.
    fn fresh_rounds(&self, record: &Record, key: StopKey, observed: &Observed) {
        if let Some(done) = record.window.borrow_mut().interrupted.as_mut() {
            done.rounds = 0;
            done.kept = Kept::No;
        }
        self.press_round(record, key, observed);
    }

    /// What closing the window would end of what it started, as a note says it: what the
    /// host lists of its turn's work, what its harness says it keeps running
    /// (`Window::kept_commands`), and what its harness's record says it runs
    /// ([`close_would_end`], which says what neither can see); none where it would end
    /// nothing.
    async fn close_would_end(&self, record: &Record, observed: &Observed) -> Option<String> {
        let (pane, window) = {
            let part = record.window.borrow();
            (part.pane.clone(), part.window.clone())
        };
        let left = match pane {
            Some(pane) => self.seams.background.left_running(&pane).await,
            None => Some(Default::default()),
        };
        let kept = match window {
            Some(window) => window.kept_commands().await,
            None => Some(Vec::new()),
        };
        close_would_end(left.as_ref(), observed.pending(), kept.as_deref())
    }

    /// Every round of the stop was ignored, and the window is kept, as `kept` says: it is
    /// marked, and the board shows it, until it pays the stop; whoever is to be told is
    /// told now if they were not, so it is marked told.
    fn keep(&self, record: &Record, key: StopKey, number: i64, rounds: u32, kept: Kept) {
        {
            let mut part = record.window.borrow_mut();
            let turn = part.interrupted.as_ref().and_then(|done| done.turn.clone());
            part.interrupted = Some(Interrupted {
                key,
                rounds,
                at: self.now(),
                kept,
                told: true,
                turn,
            });
            part.unstopped = Some(Unstopped {
                task: number,
                rounds,
            });
        }
        self.changed();
    }

    /// Whoever gave the task, whoever asked the stop and the human are told once that the
    /// window did not stop and is kept, what it runs as far as ConsensFlow can tell (`runs`;
    /// none: nothing it can see), which nothing ended, and that only the human can end it.
    /// What they read of the task is what it is as they are told ([`kept_words`]).
    fn say_kept(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        stop: &Stop,
        rounds: u32,
        runs: Option<&str>,
        again: Option<i64>,
    ) -> Result<(), EngineError> {
        let task = self
            .seams
            .ledger
            .borrow()
            .task(project.id, stop.number)?
            .map(|thread| thread.task);
        let ours = task
            .as_ref()
            .is_some_and(|task| task.assignee.as_ref() == Some(&participant.handle));
        let (human, to_requester) = kept_words(
            &participant.handle,
            stop.number,
            rounds,
            runs,
            task.as_ref().map(|task| task.state.as_str()),
            ours,
            again,
        );
        self.tell_stop(
            project,
            stop,
            task.map(|task| task.requester),
            human,
            to_requester,
        )
    }

    /// A note of a window kept for `stop` to each who needs it, once each: whoever gave
    /// the task, `requester`, else the human; whoever asked the stop, where that is
    /// someone else (the human's pause or the chief's tell of a task the other gave); and
    /// the human, who alone can end it. The human reads `human`, any other `to_requester`.
    /// Two notes to one reader of one event said it twice (T-502's, 2026-10-25).
    fn tell_stop(
        &self,
        project: &ProjectView,
        stop: &Stop,
        requester: Option<String>,
        human: String,
        to_requester: String,
    ) -> Result<(), EngineError> {
        let asked_by =
            self.seams
                .ledger
                .borrow()
                .stop_asked_by(project.id, stop.number, stop.seq)?;
        let mut readers = vec![requester.unwrap_or_else(|| "human".to_owned())];
        readers.extend(asked_by.filter(|by| !readers.contains(by)));
        if !readers.iter().any(|reader| reader == "human") {
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
            told: false,
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
    fn three_rounds_ignored_keep_the_window_of_a_pause_and_a_cancelled_turn_just_stops() {
        assert_eq!(
            next_round(Some(&done(2, 0, Kept::No)), KEY, 3_000),
            Round::Press,
            "two rounds ignored are no reason to keep it"
        );
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::No)), KEY, 2_999),
            Round::Wait
        );
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::No)), KEY, 3_000),
            Round::Keep
        );
        let cancelled = StopKey::Cancelled { task: 4 };
        let last = Interrupted {
            key: cancelled,
            ..done(3, 0, Kept::No)
        };
        assert_eq!(next_round(Some(&last), cancelled, 600_000), Round::Wait);
    }

    #[test]
    fn a_window_kept_running_nothing_in_view_is_asked_again_every_minute_with_no_ceiling() {
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::Ignored)), KEY, 59_999),
            Round::Wait
        );
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::Ignored)), KEY, 60_000),
            Round::Keep
        );
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::Ignored)), KEY, 40 * 60_000),
            Round::Keep,
            "a hung harness is the human's to end, however long"
        );
    }

    #[test]
    fn a_window_kept_for_its_work_is_looked_at_again_every_ten_seconds_with_no_key() {
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::ForWork)), KEY, 9_999),
            Round::Wait
        );
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::ForWork)), KEY, 10_000),
            Round::Keep
        );
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::ForWork)), KEY, 40 * 60_000),
            Round::Keep,
            "with no ceiling: the work may run for hours"
        );
    }

    #[test]
    fn a_kept_windows_notes_say_what_its_task_is_as_they_are_told() {
        // To the chief, after what ConsensFlow does: the task as it is, and what the chief
        // can do; to the human, how to end it, a session that holds work paused first.
        let tail = |state| kept_words("zeus", 1, 3, None, state, true, None).1;
        let human = |state| kept_words("zeus", 1, 3, None, state, true, None).0;
        assert!(tail(Some("paused")).contains("keeps the window open and T-1 paused,"));
        assert!(tail(Some("paused")).ends_with("until then what it writes is not taken as T-1's result. `cf task resume T-1 \"…\"` gives the window your words once its turn ends; only the human can end it sooner, by deleting its session."));
        assert!(human(Some("paused")).ends_with("To end it now, delete @zeus's session."));
        for state in ["queued", "working", "waiting"] {
            let said = tail(Some(state));
            assert!(!said.contains("paused"), "{state}: {said}");
            assert!(
                said.ends_with("until then what it writes is not taken as T-1's result. Your words go in once its turn ends; only the human can end it sooner, by deleting its session."),
                "{state}: {said}"
            );
            assert!(
                human(Some(state))
                    .ends_with("To end it now, pause T-1 and delete @zeus's session."),
                "{state}"
            );
        }
        for (state, said) in [
            (Some("cancelled"), " T-1 was cancelled;"),
            (Some("done"), " T-1 is done;"),
            (Some("failed"), " T-1 is failed;"),
            (Some("accepted"), " T-1 is accepted;"),
            (Some("open"), " T-1 is open;"),
            (None, " T-1 was deleted;"),
        ] {
            let told = tail(state);
            assert!(
                told.contains(&format!("every minute;{said} only the human")),
                "{state:?}: {told}"
            );
            assert!(
                !told.contains("cf task resume") && !told.contains("go in"),
                "{state:?}"
            );
        }
        // A task in another window's hands now (its owner changed: given to its tier again)
        // is that window's: no words of it are promised, nor is it said paused.
        for state in ["paused", "queued", "working"] {
            let (human, told) = kept_words("zeus", 1, 3, None, Some(state), false, None);
            assert!(
                told.contains("every minute; T-1 is in another window's hands now; only the human"),
                "{state}: {told}"
            );
            assert!(
                !told.contains("cf task resume")
                    && !told.contains("go in")
                    && !told.contains("paused"),
                "{state}: {told}"
            );
            assert!(
                human.ends_with("To end it now, delete @zeus's session."),
                "{state}: {human}"
            );
        }
        // A message relaunched once that began nothing again: relaunched no more, both told.
        let (human, told) = kept_words("zeus", 1, 3, None, Some("paused"), true, Some(42));
        for said in [&human, &told] {
            assert!(said.ends_with("Its window began no turn on m-42 again after ConsensFlow relaunched it once, so it is relaunched no more."), "{said}");
        }
        let work = kept_words("zeus", 1, 3, Some("node job"), Some("paused"), true, None);
        assert!(work
            .0
            .contains("runs work that closing it would end: node job."));
        assert!(work
            .1
            .contains("keeps the window open for it and T-1 paused, ends none of it"));
    }

    #[test]
    fn another_stop_starts_its_rounds_afresh() {
        let later = StopKey::Pause { task: 4, seq: 2 };
        assert_eq!(
            next_round(Some(&done(3, 0, Kept::Ignored)), later, 1),
            Round::Press
        );
    }
}
